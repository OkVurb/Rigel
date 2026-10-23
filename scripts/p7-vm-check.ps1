<#
.SYNOPSIS
The machine-checkable half of the Phase 7 exit test (P7.9), for a clean Windows VM.

.DESCRIPTION
`scripts/p7-exit-test.sh` proves what a developer machine can prove. This script is the same
acceptance on the machine that matters: a Windows 10 1809+ VM where Rigel has never run.

It prints one line per check and exits 1 if any of them failed. Two things still need a person and
are printed as `manual` rather than claimed:
  * SmartScreen, which only judges a file that arrived with a browser's zone identifier;
  * whether the window looks right on a display nobody has configured.

Usage (an elevated prompt: the MSI installs for all users):
  .\scripts\p7-vm-check.ps1 -Msi .\target\release\bundle\msi\Rigel_0.1.0_x64_en-US.msi `
                            -Rigel C:\tools\rigel.exe

Without -Msi it still checks the machine and the CLI (`-SkipInstall` is implied).
#>
[CmdletBinding()]
param(
  [string]$Msi = "",
  [string]$Rigel = "",
  [string]$WorkDir = (Join-Path $env:TEMP "rigel-vm-check"),
  [switch]$SkipInstall,
  [switch]$NoUninstall
)

$ErrorActionPreference = "Continue"
$script:failed = 0
$script:manual = 0

function Check([string]$name, [bool]$ok, [string]$detail = "") {
  if ($ok) {
    Write-Host ("  ok    {0}{1}" -f $name, $(if ($detail) { " ($detail)" } else { "" }))
  } else {
    Write-Host ("  FAIL  {0}{1}" -f $name, $(if ($detail) { " ($detail)" } else { "" })) -ForegroundColor Red
    $script:failed = 1
  }
}

function Manual([string]$name) {
  Write-Host "  manual $name" -ForegroundColor Yellow
  $script:manual++
}

function IsElevated {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal $identity
  $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function WebView2Version {
  $keys = @(
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
  )
  foreach ($key in $keys) {
    $value = (Get-ItemProperty -Path $key -ErrorAction SilentlyContinue).pv
    if ($value) { return $value }
  }
  return $null
}

function InstalledAppExe {
  # The MSI installs the crate's binary name (`rigel-app.exe`) under the product directory; the
  # NSIS installer may name it after the product. Accept either, and prefer anything with "rigel".
  $candidates = @(
    (Join-Path $env:ProgramFiles "Rigel"),
    (Join-Path ${env:ProgramFiles(x86)} "Rigel"),
    (Join-Path $env:LOCALAPPDATA "Rigel")
  )
  foreach ($dir in $candidates) {
    if (-not $dir -or -not (Test-Path $dir)) { continue }
    $exe = Get-ChildItem -Path $dir -Filter *.exe -File -ErrorAction SilentlyContinue |
      Sort-Object { $_.Name -like "*rigel*" } -Descending |
      Select-Object -First 1
    if ($exe) { return $exe.FullName }
  }
  return $null
}

New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$env:RIGEL_INDEX_DIR = Join-Path $WorkDir "data"
New-Item -ItemType Directory -Force -Path $env:RIGEL_INDEX_DIR | Out-Null
$env:HTTP_PROXY = "http://127.0.0.1:9"
$env:HTTPS_PROXY = "http://127.0.0.1:9"

Write-Host "p7-vm: this machine"
$os = [Environment]::OSVersion.Version
$release = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -ErrorAction SilentlyContinue).DisplayVersion
$webviewBefore = WebView2Version
Write-Host ("  info  Windows {0} (build {1}), WebView2 {2}" -f $release, $os.Build, $(if ($webviewBefore) { $webviewBefore } else { "absent" }))
Check "Windows 10 1809 or newer (build 17763+)" ($os.Build -ge 17763) ("build " + $os.Build)

$node = Get-Command node -ErrorAction SilentlyContinue
Check "no Node.js on PATH" (-not $node) $(if ($node) { $node.Source } else { "absent" })

$nodeModules = Get-ChildItem -Path $WorkDir -Recurse -Directory -Filter node_modules -ErrorAction SilentlyContinue
Check "the check's own work dir needs no node_modules" (@($nodeModules).Count -eq 0)

$installed = $null
if ($Msi) {
  Write-Host "p7-vm: the installer"
  Check "the MSI exists" (Test-Path $Msi) $Msi
  $signature = Get-AuthenticodeSignature $Msi
  $signed = $null -ne $signature.SignerCertificate
  Check "the MSI carries an Authenticode signature" $signed $(if ($signed) { $signature.SignerCertificate.Subject } else { "unsigned" })
  if ($signed -and $signature.Status -ne "Valid") {
    Manual "SmartScreen and the chain: status is $($signature.Status); only a downloaded copy is judged"
  }

  if (-not $SkipInstall) {
    Check "elevated prompt" (IsElevated) "msiexec installs for all users"
    $log = Join-Path $WorkDir "install.log"
    $process = Start-Process msiexec -Wait -PassThru -ArgumentList @("/i", "`"$Msi`"", "/qn", "/norestart", "/l*v", "`"$log`"")
    Check "msiexec finished" ($process.ExitCode -in 0, 3010) ("exit " + $process.ExitCode)
    $installed = InstalledAppExe
    Check "the app is installed" ($null -ne $installed) $installed
    $webviewAfter = WebView2Version
    Check "the WebView2 bootstrapper ran" ($null -ne $webviewAfter) $(if ($webviewAfter) { $webviewAfter } else { "still absent" })

    if ($installed) {
      $app = Start-Process $installed -PassThru
      Start-Sleep -Seconds 20
      $alive = -not $app.HasExited
      Check "the window starts on this machine" $alive $(if ($alive) { "pid " + $app.Id } else { "exit " + $app.ExitCode })
      if ($alive) { Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue }
      Manual "look at the window: one project tab, the Shelf, and no missing-runtime error"
    }
  }
} else {
  Write-Host "p7-vm: no -Msi given, so the installer and window checks were skipped"
}

if ($Rigel) {
  Write-Host "p7-vm: the CLI and one native task"
  Check "the CLI exists" (Test-Path $Rigel) $Rigel
  $project = Join-Path $WorkDir "project"
  New-Item -ItemType Directory -Force -Path $project | Out-Null

  & $Rigel --data-dir $env:RIGEL_INDEX_DIR doctor --project $project --json *> (Join-Path $WorkDir "doctor.json")
  $doctorCode = $LASTEXITCODE
  $doctorLine = Get-Content (Join-Path $WorkDir "doctor.json") | Where-Object { $_.Trim() } | Select-Object -Last 1
  $doctor = $doctorLine | ConvertFrom-Json
  Check "doctor made no failure row" ($doctor.failures -eq 0) ("failures " + $doctor.failures)
  Check "doctor's exit code matches its rows" ($doctorCode -eq $doctor.exit_code) ("status $doctorCode")

  $script = Join-Path $WorkDir "script.json"
  '{"steps":[{"text":"p7 vm check"}]}' | Set-Content -Path $script -Encoding ASCII
  & $Rigel --data-dir $env:RIGEL_INDEX_DIR agent run --project $project --script $script --prompt "p7 vm check" --json *> (Join-Path $WorkDir "turn.json")
  $turnCode = $LASTEXITCODE
  Check "one native turn completed offline" ($turnCode -eq 0) ("status " + $turnCode)
  Check "the turn is in the transcript" ((Get-Content (Join-Path $WorkDir "turn.json") -Raw) -match '"type":"turn_finished"')

  & $Rigel --data-dir $env:RIGEL_INDEX_DIR skills search "pdf" --json *> (Join-Path $WorkDir "shelf.json")
  Check "the shelf answers offline" ($LASTEXITCODE -eq 0) ("status " + $LASTEXITCODE)
} else {
  Write-Host "p7-vm: no -Rigel given, so the CLI checks were skipped"
}

if ($Msi -and -not $SkipInstall -and -not $NoUninstall) {
  Write-Host "p7-vm: uninstall"
  $process = Start-Process msiexec -Wait -PassThru -ArgumentList @("/x", "`"$Msi`"", "/qn", "/norestart")
  Check "msiexec removed the app" ($process.ExitCode -in 0, 3010) ("exit " + $process.ExitCode)
}

Write-Host ""
if ($script:failed -ne 0) {
  Write-Host "p7-vm: FAILED" -ForegroundColor Red
  exit 1
}
Write-Host ("p7-vm: ok ({0} manual line(s) still need a person)" -f $script:manual)
exit 0
