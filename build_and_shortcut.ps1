$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path

if (Test-Path "C:\ufo\ufo\python_env\python.exe") {
    $env:PYTHON_EXECUTABLE = "C:\ufo\ufo\python_env\python.exe"
} elseif (Test-Path "C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe") {
    $env:PYTHON_EXECUTABLE = "C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe"
} else {
    $env:PYTHON_EXECUTABLE = "python.exe"
}

cargo build --release

$WshShell = New-Object -comObject WScript.Shell
$Desktop = [Environment]::GetFolderPath("Desktop")
$ShortcutPath = Join-Path $Desktop "BankFidelity.lnk"
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = "C:\bankfidelity\bankfidelity\target\x86_64-pc-windows-gnu\release\dual-core-pdf-pipeline.exe"
$Shortcut.Arguments = "gui"
$Shortcut.WorkingDirectory = "C:\bankfidelity\bankfidelity"
$Shortcut.Save()
Write-Host "Desktop shortcut created at $ShortcutPath"
