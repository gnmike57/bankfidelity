# Requires RunAsAdministrator
$action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoExit -Command \"cd C:\ufo\ufo; Write-Host 'UFO Desktop Terminal - Full Access';\""
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
$task = New-ScheduledTask -Action $action -Principal $principal
Register-ScheduledTask -TaskName "UFODesktopTerminal" -InputObject $task -Force
Start-ScheduledTask -TaskName "UFODesktopTerminal"
Unregister-ScheduledTask -TaskName "UFODesktopTerminal" -Confirm:$false
