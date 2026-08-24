param(
  [Parameter(Mandatory = $true)][string]$LeaveBinary,
  [Parameter(Mandatory = $true)][string]$WorkspaceId
)

$action = New-ScheduledTaskAction -Execute $LeaveBinary -Argument "serve --workspace $WorkspaceId"
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "LeaveHost" -Action $action -Trigger $trigger -Principal $principal -Settings $settings
