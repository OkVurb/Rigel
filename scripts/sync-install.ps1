param(
  [string]$Destination = 'C:\Program Files\Rigel',
  [switch]$Check,
  [switch]$NoRestart
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$source = Join-Path $root 'target\release'
$names = @('rigel-app.exe', 'rigel.exe')

foreach ($name in $names) {
  if (-not (Test-Path (Join-Path $source $name))) {
    throw "missing build output: $(Join-Path $source $name). Run scripts/build-release.ps1 first."
  }
}

function Get-Hash([string]$path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash }

function Test-Writable([string]$directory) {
  try {
    if (-not (Test-Path $directory)) { New-Item -ItemType Directory -Path $directory -Force | Out-Null }
    $probe = Join-Path $directory ('.rigel-write-test-' + [guid]::NewGuid().ToString('N'))
    [System.IO.File]::WriteAllText($probe, 'x')
    Remove-Item -LiteralPath $probe -Force
    $true
  } catch {
    $false
  }
}

$pending = @()
foreach ($name in $names) {
  $built = Join-Path $source $name
  $installed = Join-Path $Destination $name
  if ((Test-Path $installed) -and ((Get-Hash $built) -eq (Get-Hash $installed))) {
    "current: $name"
  } else {
    $pending += $name
    "stale: $name"
  }
}

if ($pending.Count -eq 0) {
  'install is current'
  exit 0
}
if ($Check) {
  'would copy: ' + ($pending -join ', ')
  exit 2
}

if (-not (Test-Writable $Destination)) {
  $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath, '-Destination', $Destination)
  if ($NoRestart) { $arguments += '-NoRestart' }
  Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments
  "requested elevation for $Destination"
  exit 0
}

$prefix = (Resolve-Path -LiteralPath $Destination).Path
$stopped = $false
Get-CimInstance Win32_Process -Filter "Name='rigel-app.exe'" |
  Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, 'OrdinalIgnoreCase') } |
  ForEach-Object {
    Stop-Process -Id $_.ProcessId -Force
    $stopped = $true
    "stopped app $($_.ProcessId)"
  }
Get-CimInstance Win32_Process -Filter "Name='pythonw.exe'" |
  Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, 'OrdinalIgnoreCase') } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Sleep -Seconds 2

foreach ($name in $pending) {
  Copy-Item (Join-Path $source $name) (Join-Path $Destination $name) -Force
  "copied $name"
}
foreach ($name in $names) {
  if ((Get-Hash (Join-Path $source $name)) -ne (Get-Hash (Join-Path $Destination $name))) {
    throw "hash mismatch after copy: $name"
  }
}
'installed and verified'

if ($stopped -and -not $NoRestart) {
  Start-Process (Join-Path $Destination 'rigel-app.exe') -WorkingDirectory $Destination
  'relaunched the installed app'
}
