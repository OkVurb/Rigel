# Keep the Rigel API up at every Windows sign-in.
#
# The Startup folder runs a hidden VBS wrapper; the wrapper starts a small cmd loop that
# restarts `rigel serve` if it exits. `--tailscale` is retried by that loop, so a login where
# Tailscale comes up a few seconds later still converges. The binary is copied out of target/
# so a running API does not lock the build output.
param(
    [string]$Repo = "",
    [string]$Source = ""
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($Repo)) {
    $Repo = Split-Path -Parent $PSScriptRoot
}
$Repo = (Resolve-Path $Repo).Path
if ([string]::IsNullOrWhiteSpace($Source)) {
    $Source = Join-Path $Repo "target\debug\rigel.exe"
}
if (-not (Test-Path $Source)) {
    throw "No rigel.exe at $Source. Build it with 'cargo build -p rigel-cli' first."
}

$bin = Join-Path $env:LOCALAPPDATA "Rigel\bin"
$logs = Join-Path $env:LOCALAPPDATA "Rigel\logs"
New-Item -ItemType Directory -Force -Path $bin, $logs | Out-Null
$exe = Join-Path $bin "rigel.exe"
try {
    Copy-Item $Source $exe -Force
} catch {
    throw "Could not replace $exe. If the API is running from it, stop that process and rerun. ($_)"
}

# The API adopts the sidecar beside its executable. Copy it here too, or a logon-started API
# would fall back to vendor/agentchattr while the room runs from the installed app-data copy,
# and every assignment would land in a queue its wrappers never read.
$sidecar = Join-Path (Split-Path -Parent $Source) "sidecar"
if (Test-Path (Join-Path $sidecar "agentchattr\run.py")) {
    $sidecarCopy = Join-Path $bin "sidecar"
    if (Test-Path $sidecarCopy) {
        Remove-Item $sidecarCopy -Recurse -Force
    }
    Copy-Item $sidecar $sidecarCopy -Recurse -Force
}

$apiLog = Join-Path $logs "api.log"
$cmdPath = Join-Path $bin "rigel-serve.cmd"
$cmdBody = @"
@echo off
:loop
"$exe" serve --project "$Repo" --tailscale --full-access --json >> "$apiLog" 2>&1
timeout /t 10 /nobreak >nul
goto loop
"@
Set-Content -Path $cmdPath -Value $cmdBody -Encoding ASCII

$vbsPath = Join-Path $bin "rigel-serve.vbs"
$vbsBody = @"
Set shell = CreateObject("WScript.Shell")
shell.Run ""$cmdPath"", 0, False
"@
Set-Content -Path $vbsPath -Value $vbsBody -Encoding ASCII

$startup = [Environment]::GetFolderPath("Startup")
$startupLink = Join-Path $startup "Rigel API.vbs"
Copy-Item $vbsPath $startupLink -Force

Write-Output "Installed: $startupLink"
Write-Output "Command:   $cmdPath"
Write-Output "Log:       $apiLog"
Write-Output "The next sign-in starts the API hidden; Tailscale is retried until it is reachable."
