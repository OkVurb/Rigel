<#
.SYNOPSIS
The P7.9 acceptance as it runs *inside* Windows Sandbox.

.DESCRIPTION
`scripts/p7-sandbox.ps1` maps this checkout read-only to `C:\rigel` and `docs\qa\records` read-write
to `C:\out`, then runs this file as the sandbox's first logon command. Nothing here is special:
it calls `scripts/p7-vm-check.ps1` exactly as an operator would on a VM, and copies the transcript
somewhere the host can read it.

The sandbox is a clean Windows: Rigel has never run here, nothing of this checkout was ever
installed here, and the only Node.js is whatever the image ships (none, on a stock image).
That is what makes the install checks mean something — on the developer machine they are a smoke
test of msiexec, not of a first install.

Not proved here, and not claimed: the Windows 10 1809 floor (the sandbox is the host's build) and
SmartScreen (nothing arrived with a browser's zone identifier).
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$Msi,
  [Parameter(Mandatory = $true)][string]$Rigel,
  [string]$Check = "C:\rigel\scripts\p7-vm-check.ps1",
  [string]$OutDir = "C:\out"
)

$ErrorActionPreference = "Continue"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$log = Join-Path $OutDir "p7-sandbox-$stamp.txt"

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }
try { Start-Transcript -Path $log -Force | Out-Null } catch { }

Write-Host "p7-sandbox: Windows Sandbox"
$os = [Environment]::OSVersion.Version
Write-Host ("  info  build {0}, host name {1}, transcript {2}" -f $os.Build, $env:COMPUTERNAME, $log)

& $Check -Msi $Msi -Rigel $Rigel
$code = $LASTEXITCODE

Write-Host ""
if ($code -eq 0) {
  Write-Host "RESULT: ok — paste $log into docs/qa/p7-acceptance.md" -ForegroundColor Green
} else {
  Write-Host "RESULT: FAILED (exit $code) — $log holds the failing lines" -ForegroundColor Red
}
Write-Host "  the two lines that still need a person: SmartScreen, and how the window looked."
Write-Host "  close this window when you have read it; the sandbox throws everything away."

try { Stop-Transcript | Out-Null } catch { }
Read-Host "Press Enter to close" | Out-Null
exit $code
