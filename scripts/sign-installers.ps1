<#
.SYNOPSIS
Sign and verify the Rigel installers, inside and out, with an Authenticode certificate.

.DESCRIPTION
P7.5 keeps the release certificate off this repository: the operator holds it, not the checkout.
This script is the only place the two meet, so the steps are the same with a CA-issued certificate
and with a throwaway test one.

An MSI is a container. Signing the MSI alone signs the wrapper; the copy of `rigel-app.exe` (and
the WiX helper DLLs) inside stays unsigned, so what lands in Program Files fails verification.

The order matters for a second reason. Tauri patches the app executable with bundle-type
information while it packages (once for MSI, once for NSIS). A signature written *before* that
patch is stale the moment the bundler touches the file, so signing the binary first and then
bundling produces an MSI whose embedded executable has an invalid signature. Tauri signs the
patched binary itself when `bundle.windows.certificateThumbprint` is set. This script does not
commit that machine-specific value; it writes a temporary config with the operator's thumbprint,
passes it with `--config`, and lets the bundler sign:

  1. the patched app executable, once per package type, before packaging;
  2. the WiX extension DLLs that are embedded in the MSI;
  3. the MSI and the NSIS setup after they are built.

It then verifies every one of those signatures, and unpacks the MSI (`msiexec /a`) to check that
each PE file the MSI installs is validly signed and by the same certificate as the wrapper. The
WiX helper DLLs embedded in the MSI are signed by Tauri as well; they are not installed files, so
the MSI's own signature is what covers them.

`-Build` does the build and both signing layers. Without it, the script signs the wrappers of an
existing bundle and verifies it: a bundle that was packaged without Tauri's signing fails the
nested check, which is the point. `-VerifyOnly` needs no certificate.

The certificate is not the update key. Authenticode (this script) and the Ed25519 key that signs
the update manifest (`update-pubkey.txt`, `RIGEL_UPDATE_PUBKEY`) are different keypairs with
different jobs; neither can stand in for the other. Tauri's own updater key
(`TAURI_SIGNING_PRIVATE_KEY`) is not used by Rigel at all.

A certificate that is present but not trusted (a self-signed test certificate) is reported as such;
a file with no signature, or with a signature that no longer matches its bytes, is a failure.

.EXAMPLE
$env:RIGEL_CERT_THUMBPRINT = "A1B2..."
.\scripts\sign-installers.ps1 -Build

.EXAMPLE
# Sign the wrappers of an already built bundle, then prove both layers carry a signature.
.\scripts\sign-installers.ps1

.EXAMPLE
# Prove an existing build produced installers without a certificate present.
.\scripts\sign-installers.ps1 -VerifyOnly
#>
[CmdletBinding()]
param(
  [string]$BundleDir = "",
  [string]$Thumbprint = $env:RIGEL_CERT_THUMBPRINT,
  [string]$TimestampUrl = "http://timestamp.digicert.com",
  [string]$AppDir = "",
  # Build with Tauri's signing enabled: the only order that signs the patched payload.
  [switch]$Build,
  [switch]$VerifyOnly,
  [switch]$NoTimestamp
)

$ErrorActionPreference = "Continue"
$script:failed = 0

function Fail([string]$message) {
  Write-Host "  FAIL  $message" -ForegroundColor Red
  exit 1
}

function Usage([string]$message) {
  Write-Host "  FAIL  $message" -ForegroundColor Red
  exit 2
}

function Ok([string]$message) { Write-Host "  ok    $message" }
function Note([string]$message) { Write-Host "  note  $message" -ForegroundColor Yellow }

function Find-SignTool {
  $sdk = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
  if (-not (Test-Path $sdk)) { return $null }
  Get-ChildItem $sdk -Directory |
    Where-Object { $_.Name -match '^10\.' } |
    Sort-Object { [version]($_.Name) } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64/signtool.exe" } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
# A deep checkout sets CARGO_TARGET_DIR to keep link.exe under MAX_PATH (see README).
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
if (-not $AppDir) { $AppDir = Join-Path $root "crates/rigel-app" }
# $PSScriptRoot is empty while parameters are being bound, so the default is resolved here.
if (-not $BundleDir) { $BundleDir = Join-Path $target "release/bundle" }

$signtool = Find-SignTool
if (-not $signtool) {
  Usage "signtool.exe not found; install the Windows 10 SDK"
}

$normalized = ($Thumbprint -replace '\s', '').ToUpperInvariant()
if (-not $VerifyOnly) {
  if (-not $normalized) {
    Write-Host "  FAIL  no certificate: set RIGEL_CERT_THUMBPRINT or pass -Thumbprint" -ForegroundColor Red
    Write-Host "        the certificate never belongs in this repository; see docs/PLATFORMS.md" -ForegroundColor Yellow
    exit 2
  }
  $cert = @(Get-ChildItem Cert:/CurrentUser/My, Cert:/LocalMachine/My |
    Where-Object { $_.Thumbprint -eq $normalized }) | Select-Object -First 1
  if (-not $cert) {
    Usage "${normalized} is not in this machine's certificate stores"
  }
  if (-not $cert.HasPrivateKey) {
    Usage "${normalized} has no private key here"
  }
}

function Sign-Path([string]$path) {
  $signArgs = @("sign", "/sha1", $normalized, "/fd", "sha256")
  if (-not $NoTimestamp) { $signArgs += @("/tr", $TimestampUrl, "/td", "sha256") }
  $signArgs += $path
  & $signtool @signArgs 2>$null | Out-Null
  if ($LASTEXITCODE -ne 0) {
    Fail "could not sign $(Split-Path -Leaf $path)"
  }
  Ok "signed $(Split-Path -Leaf $path)"
}

# A self-signed certificate gives a cryptographically valid signature that Windows does not trust:
# PowerShell reports that as `NotTrusted`, or as `UnknownError` with a chain message (localized, so
# the status is what counts, not the text). Changed bytes report `HashMismatch`, which is what a
# signature written before Tauri patched the binary would look like.
function Test-Signature($signature, $expectedThumbprint, [string]$label) {
  if (-not $signature.SignerCertificate) {
    Write-Host "  FAIL  ${label} has no signature" -ForegroundColor Red
    return $false
  }
  if ($signature.Status -notin @("Valid", "NotTrusted", "UnknownError")) {
    Write-Host "  FAIL  ${label} has an invalid signature (status $($signature.Status): $($signature.StatusMessage))" -ForegroundColor Red
    return $false
  }
  if ($expectedThumbprint -and $signature.SignerCertificate.Thumbprint -ne $expectedThumbprint) {
    Write-Host "  FAIL  ${label} is signed by a different certificate ($($signature.SignerCertificate.Subject))" -ForegroundColor Red
    return $false
  }
  return $true
}

if (-not $VerifyOnly) {
  if ($Build) {
    # The certificate is checked above, before spending a build on it.
    $configPath = Join-Path ([System.IO.Path]::GetTempPath()) ("rigel-sign-" + [guid]::NewGuid().ToString("N") + ".json")
    $windows = @{
      certificateThumbprint = $normalized
      digestAlgorithm = "sha256"
    }
    if (-not $NoTimestamp) {
      $windows.timestampUrl = $TimestampUrl
      $windows.tsp = $true
    }
    @{ bundle = @{ windows = $windows } } | ConvertTo-Json -Depth 6 |
      Set-Content -Path $configPath -Encoding ASCII
    try {
      Push-Location $AppDir
      try {
        Write-Host "p7: build, with Tauri signing the patched payload and the bundles"
        & cargo tauri build --config $configPath
        if ($LASTEXITCODE -ne 0) { Fail "cargo tauri build exited $LASTEXITCODE" }
      } finally {
        Pop-Location
      }
    } finally {
      Remove-Item -Path $configPath -Force -ErrorAction SilentlyContinue
    }

  } else {
    Write-Host "p7: sign the bundle wrappers"
    if (-not (Test-Path $BundleDir)) {
      Usage "no bundle directory at ${BundleDir}; run 'cargo tauri build' or this script with -Build first"
    }
    $toSign = Get-ChildItem -Path $BundleDir -Recurse -File |
      Where-Object { $_.Extension -in ".msi", ".exe" } | Sort-Object FullName
    if (-not $toSign) { Usage "no MSI or EXE under ${BundleDir}" }
    foreach ($file in $toSign) { Sign-Path $file.FullName }
  }
}

if (-not (Test-Path $BundleDir)) {
  Usage "no bundle directory at ${BundleDir}; run 'cargo tauri build' or this script with -Build first"
}

$files = Get-ChildItem -Path $BundleDir -Recurse -File |
  Where-Object { $_.Extension -in ".msi", ".exe" } | Sort-Object FullName
if (-not $files) {
  Usage "no MSI or EXE under ${BundleDir}"
}

# An administrative install extracts every file the MSI carries without installing it. The
# extracted PE files are what lands in Program Files, so they are what a signature check must see.
function Test-MsiPayload($msi, $outerSigner) {
  $work = Join-Path ([System.IO.Path]::GetTempPath()) ("rigel-msi-" + [guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $work | Out-Null
  try {
    $msiexec = Join-Path $env:SystemRoot "System32/msiexec.exe"
    $process = Start-Process -FilePath $msiexec `
      -ArgumentList @("/a", "`"$($msi.FullName)`"", "/qn", "TARGETDIR=`"$work`"") `
      -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
      Write-Host "  FAIL  could not unpack $($msi.Name) (msiexec /a exit $($process.ExitCode))" -ForegroundColor Red
      return $false
    }

    $nested = @(Get-ChildItem -Path $work -Recurse -File |
      Where-Object { $_.Extension -in ".exe", ".dll", ".sys", ".node" })
    if ($nested.Count -eq 0) {
      Write-Host "  FAIL  $($msi.Name) carries no PE files to check" -ForegroundColor Red
      return $false
    }

    $ok = $true
    foreach ($file in $nested) {
      $signature = Get-AuthenticodeSignature $file.FullName
      if (-not (Test-Signature $signature $outerSigner.Thumbprint "$($file.Name) inside $($msi.Name)")) {
        $ok = $false
      } else {
        Ok "$($file.Name) inside $($msi.Name) is validly signed by the installer's certificate"
      }
    }
    return $ok
  } finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}

foreach ($file in $files) {
  $signature = Get-AuthenticodeSignature $file.FullName
  if (-not (Test-Signature $signature $null $file.Name)) {
    $script:failed = 1
    continue
  }

  & $signtool verify /pa /all $file.FullName 2>$null | Out-Null
  $chain = $LASTEXITCODE -eq 0
  $subject = $signature.SignerCertificate.Subject
  $stamp = if ($signature.TimeStamperCertificate) { "timestamped" } else { "no timestamp" }
  if ($signature.Status -eq "Valid") {
    Ok "$($file.Name) trusted ($stamp) $subject"
  } elseif ($chain) {
    Ok "$($file.Name) signed, chain trusted ($stamp) $subject"
  } else {
    Note "$($file.Name) signed but chain untrusted ($stamp) $subject"
    Note "a release needs a CA-issued or trusted certificate; see docs/PLATFORMS.md"
  }

  if ($file.Extension -eq ".msi") {
    if (-not (Test-MsiPayload $file $signature.SignerCertificate)) {
      Write-Host "        the MSI embeds the executable as it was when it was bundled." -ForegroundColor Yellow
      Write-Host "        build with -Build so Tauri signs the patched payload; see docs/PLATFORMS.md" -ForegroundColor Yellow
      $script:failed = 1
    }
  }
}

exit $script:failed
