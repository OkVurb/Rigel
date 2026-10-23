<#
.SYNOPSIS
Sign and verify the Rigel installers with an Authenticode certificate.

.DESCRIPTION
P7.5 keeps the release certificate off this repository: the operator holds it, not the checkout.
This script is the only place the two meet, so the steps are the same with a CA-issued certificate
and with a throwaway test one.

It signs every MSI and EXE under the bundle directory, then verifies each signature. A certificate
that is present but not trusted (a self-signed test certificate) is reported as such; a file with
no signature at all is a failure.

.EXAMPLE
cargo tauri build
pwsh scripts/sign-installers.ps1 -Thumbprint A1B2...   # or RIGEL_CERT_THUMBPRINT

.EXAMPLE
# Prove the build produced installers without a certificate present.
pwsh scripts/sign-installers.ps1 -VerifyOnly
#>
[CmdletBinding()]
param(
  [string]$BundleDir = "",
  [string]$Thumbprint = $env:RIGEL_CERT_THUMBPRINT,
  [string]$TimestampUrl = "http://timestamp.digicert.com",
  [switch]$VerifyOnly,
  [switch]$NoTimestamp
)

$ErrorActionPreference = "Continue"

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

# $PSScriptRoot is empty while parameters are being bound, so the default is resolved here.
if (-not $BundleDir) {
  $BundleDir = Join-Path $PSScriptRoot "../target/release/bundle"
}
if (-not (Test-Path $BundleDir)) {
  Write-Host "  FAIL  no bundle directory at ${BundleDir}; run 'cargo tauri build' first" -ForegroundColor Red
  exit 2
}

$files = Get-ChildItem -Path $BundleDir -Recurse -File |
  Where-Object { $_.Extension -in ".msi", ".exe" } | Sort-Object FullName
if (-not $files) {
  Write-Host "  FAIL  no MSI or EXE under ${BundleDir}" -ForegroundColor Red
  exit 2
}

$signtool = Find-SignTool
if (-not $signtool) {
  Write-Host "  FAIL  signtool.exe not found; install the Windows 10 SDK" -ForegroundColor Red
  exit 2
}

if (-not $VerifyOnly) {
  $normalized = ($Thumbprint -replace '\s', '').ToUpperInvariant()
  if (-not $normalized) {
    Write-Host "  FAIL  no certificate: set RIGEL_CERT_THUMBPRINT or pass -Thumbprint" -ForegroundColor Red
    Write-Host "        the certificate never belongs in this repository; see docs/PLATFORMS.md" -ForegroundColor Yellow
    exit 2
  }
  $cert = @(Get-ChildItem Cert:/CurrentUser/My, Cert:/LocalMachine/My |
    Where-Object { $_.Thumbprint -eq $normalized }) | Select-Object -First 1
  if (-not $cert) {
    Write-Host "  FAIL  ${normalized} is not in this machine's certificate stores" -ForegroundColor Red
    exit 2
  }
  if (-not $cert.HasPrivateKey) {
    Write-Host "  FAIL  ${normalized} has no private key here" -ForegroundColor Red
    exit 2
  }

  foreach ($file in $files) {
    $signArgs = @("sign", "/sha1", $normalized, "/fd", "sha256")
    if (-not $NoTimestamp) { $signArgs += @("/tr", $TimestampUrl, "/td", "sha256") }
    $signArgs += $file.FullName
    & $signtool @signArgs 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) {
      Write-Host "  FAIL  could not sign $($file.Name)" -ForegroundColor Red
      exit 1
    }
    Write-Host "  ok    signed $($file.Name)"
  }
}

$failed = 0
foreach ($file in $files) {
  $signature = Get-AuthenticodeSignature $file.FullName
  if (-not $signature.SignerCertificate) {
    Write-Host "  FAIL  $($file.Name) has no signature" -ForegroundColor Red
    $failed = 1
    continue
  }

  & $signtool verify /pa /all $file.FullName 2>$null | Out-Null
  $chain = $LASTEXITCODE -eq 0
  $subject = $signature.SignerCertificate.Subject
  $stamp = if ($signature.TimeStamperCertificate) { "timestamped" } else { "no timestamp" }
  if ($signature.Status -eq "Valid") {
    Write-Host "  ok    $($file.Name) trusted ($stamp) $subject"
  } elseif ($chain) {
    Write-Host "  ok    $($file.Name) signed, chain trusted ($stamp) $subject"
  } else {
    Write-Host "  note  $($file.Name) signed but chain untrusted ($stamp) $subject" -ForegroundColor Yellow
    Write-Host "        a release needs a CA-issued or trusted certificate; see docs/PLATFORMS.md" -ForegroundColor Yellow
  }
}

exit $failed
