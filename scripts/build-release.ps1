param(
  [switch]$SkipBuild,
  [switch]$NoInstall,
  [string]$Destination = 'C:\Program Files\Rigel'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

if (-not $SkipBuild) {
  # CI=1 makes the Tauri CLI parse `--ci 1` and abort, so force it false for this invocation.
  $env:CI = 'false'
  Push-Location (Join-Path $root 'crates\rigel-app')
  try {
    cargo tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
  } finally {
    Pop-Location
  }
}

if (-not $NoInstall) {
  # Run the copy as its own process: an `exit` inside a dot-sourced script does not set
  # $LASTEXITCODE, and a failed install must fail the build loudly.
  $sync = Join-Path $PSScriptRoot 'sync-install.ps1'
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $sync -Destination $Destination
  if ($LASTEXITCODE -ne 0) { throw "install step failed with exit code $LASTEXITCODE" }
}
