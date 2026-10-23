<#
.SYNOPSIS
Arm the P7.9 sandbox acceptance to run once, at the next sign-in.

.DESCRIPTION
`scripts/p7-sandbox.ps1` needs `WindowsSandbox.exe`, which only exists after the machine has really
restarted with `Containers-DisposableClientVM` enabled. This registers a one-shot logon task for the
current user: 45 s after the next sign-in it runs that launcher, writes
`docs/qa/records/p7-sandbox-launch.log`, and deletes itself. If the sandbox is still missing the exit
code is 2 and the log says so; once it launches the exit code is 0 and the guest writes its own
transcript into `docs/qa/records/`.

Usage:
  powershell -File scripts/p7-queue-sandbox.ps1            # arm it for the next sign-in
  powershell -File scripts/p7-queue-sandbox.ps1 -Show      # show the task without changing it
  powershell -File scripts/p7-queue-sandbox.ps1 -Remove    # cancel it
#>
[CmdletBinding()]
param(
  [int]$DelaySeconds = 45,
  [switch]$Show,
  [switch]$Remove
)

$ErrorActionPreference = "Stop"
$name = "Rigel P7.9 sandbox acceptance"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if ($Show) {
  $task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
  if (-not $task) { Write-Host "not registered"; exit 1 }
  Write-Host ("{0} / {1}" -f $task.TaskName, $task.State)
  Write-Host ("  action : {0} {1}" -f $task.Actions.Execute, $task.Actions.Arguments)
  Write-Host ("  trigger: {0}, delay {1}" -f $task.Triggers[0].CimClass.CimClassName, $task.Triggers[0].Delay)
  exit 0
}

if ($Remove) {
  Unregister-ScheduledTask -TaskName $name -Confirm:$false
  Write-Host "removed: $name"
  exit 0
}

$log = Join-Path $root "docs\qa\records\p7-sandbox-launch.log"

# Single-quoted so nothing expands here: the task must see $LASTEXITCODE literally. It deletes
# itself after the attempt, so a sign-in weeks later cannot open a sandbox nobody asked for.
$inner = '& ''{0}\scripts\p7-sandbox.ps1'' *> ''{1}''; $c = $LASTEXITCODE; schtasks /delete /tn ''{2}'' /f | Out-Null; exit $c' -f $root, $log, $name
$arg = '-NoProfile -ExecutionPolicy Bypass -Command "' + $inner + '"'

$action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $arg -WorkingDirectory $root
$trigger = New-ScheduledTaskTrigger -AtLogOn -User ("{0}\{1}" -f $env:USERDOMAIN, $env:USERNAME)
$trigger.Delay = "PT{0}S" -f $DelaySeconds
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
  -ExecutionTimeLimit (New-TimeSpan -Minutes 15) -StartWhenAvailable

Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger -Settings $settings `
  -Description "Runs the P7.9 Windows Sandbox acceptance once at the next sign-in; the transcript lands in docs\qa\records. Delete this task to cancel." `
  -Force | Out-Null

Write-Host ("armed: {0} (runs {1}s after the next sign-in, then removes itself)" -f $name, $DelaySeconds)
Write-Host "  log  : $log"
Write-Host "  cancel: powershell -File scripts/p7-queue-sandbox.ps1 -Remove"
