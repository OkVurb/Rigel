<#
.SYNOPSIS
Launch the P7.9 acceptance in Windows Sandbox — the clean machine this checkout can actually get.

.DESCRIPTION
P7.9 asks for a clean Windows where Rigel has never run, because an install on the developer's own
machine only proves that msiexec works. This script builds a throwaway `.wsb` with absolute paths
(so no machine-specific path is ever committed), maps this checkout read-only into the sandbox and
`docs\qa\records` read-write, and starts `scripts/sandbox/p7-in-sandbox.ps1` at first logon.

Prerequisites, once per machine:
  Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
  # then restart: the feature does not take effect until the machine boots

Usage:
  pwsh scripts/p7-sandbox.ps1 -DryRun     # print the config, touch nothing
  pwsh scripts/p7-sandbox.ps1             # launch it
#>
[CmdletBinding()]
param(
  [string]$Msi = "target\release\bundle\msi\Rigel_0.1.0_x64_en-US.msi",
  [string]$Rigel = "target\release\rigel.exe",
  [string]$Out = "docs\qa\records",
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Resolve-InRepo([string]$path, [string]$what) {
  $full = if ([IO.Path]::IsPathRooted($path)) { $path } else { Join-Path $root $path }
  if (-not (Test-Path $full)) {
    throw "$what not found: $full`nBuild it first: cargo tauri build (from crates/rigel-app) and cargo build --release -p rigel-cli"
  }
  return (Resolve-Path $full).Path
}

$msiFull = Resolve-InRepo $Msi "the MSI"
$rigelFull = Resolve-InRepo $Rigel "the CLI"
$outFull = (Resolve-Path (Join-Path $root $Out)).Path

# The sandbox sees this checkout at C:\rigel (read-only) and the record folder at C:\out (writable).
function To-Sandbox([string]$full, [string]$mount) {
  if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$full is outside this checkout, so the sandbox cannot see it"
  }
  return $mount + $full.Substring($root.Length)
}

$msiIn = To-Sandbox $msiFull "C:\rigel"
$rigelIn = To-Sandbox $rigelFull "C:\rigel"

$xml = @"
<Configuration>
  <!-- Networking stays on: the WebView2 bootstrapper is one of the checks, and it downloads.
       The CLI's own no-network claim is proved separately, behind a dead proxy. -->
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$root</HostFolder>
      <SandboxFolder>C:\rigel</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$outFull</HostFolder>
      <SandboxFolder>C:\out</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\rigel\scripts\sandbox\p7-in-sandbox.ps1 -Msi "$msiIn" -Rigel "$rigelIn"</Command>
  </LogonCommand>
</Configuration>
"@

Write-Host "p7-sandbox: $rigelFull"
Write-Host "p7-sandbox: $msiFull"
Write-Host "p7-sandbox: record -> $outFull"

$config = Join-Path ([IO.Path]::GetTempPath()) ("rigel-p7-{0}.wsb" -f (Get-Date -Format "HHmmss"))
Set-Content -Path $config -Value $xml -Encoding UTF8

# DryRun is deliberately checked before the sandbox binary: the config is worth inspecting on a
# machine where the feature is still waiting for its reboot, and that is exactly when it is wanted.
if ($DryRun) {
  Write-Host "p7-sandbox: wrote $config (dry run, not launched)"
  Write-Host $xml
  exit 0
}

$exe = Join-Path $env:SystemRoot "System32\WindowsSandbox.exe"
if (-not (Test-Path $exe)) {
  Remove-Item $config -ErrorAction SilentlyContinue
  Write-Host ""
  Write-Host "Windows Sandbox is not installed yet." -ForegroundColor Yellow
  Write-Host "  Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All"
  Write-Host "  then restart; the feature only appears after a boot."
  Write-Host ""
  Write-Host "The config that will run, once it is there:"
  Write-Host $xml
  exit 2
}

Start-Process $exe -ArgumentList "`"$config`""
Write-Host ""
Write-Host "Sandbox starting. Inside it, the acceptance runs at first logon and prints one line per check."
Write-Host "Read the window while it runs: the app is started for 20 s, which is when you can see it."
Write-Host "The transcript lands in docs/qa/records/ as p7-sandbox-<stamp>.txt."
