# Detached IDE restart helper.
# Waits briefly so the launching session can return, then stops all Kiro
# processes and relaunches the IDE on this workspace.
Start-Sleep -Seconds 3

$exe = "$env:LOCALAPPDATA\Programs\Kiro\Kiro.exe"
$workspace = "C:\Users\home\Desktop\combined bank statement and metadata cloner\bank statement modifier"

# Stop every running Kiro process.
Get-Process -Name 'Kiro' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# Give the OS a moment to release window/handles.
Start-Sleep -Seconds 3

# Relaunch the IDE on the same workspace folder.
if (Test-Path $exe) {
    Start-Process -FilePath $exe -ArgumentList "`"$workspace`""
}
