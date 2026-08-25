$RepoRoot = $PSScriptRoot
if (-not $RepoRoot) { $RepoRoot = "C:\bankfidelity\bankfidelity" }

$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path

if (Test-Path "C:\ufo\ufo\python_env\python.exe") {
    $env:PYTHON_EXECUTABLE = "C:\ufo\ufo\python_env\python.exe"
} elseif (Test-Path "C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe") {
    $env:PYTHON_EXECUTABLE = "C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe"
} else {
    $env:PYTHON_EXECUTABLE = "python.exe"
}

cargo build --release

$CandidatePaths = @(
    (Join-Path $RepoRoot "target\release\dual-core-pdf-pipeline.exe"),
    (Join-Path $RepoRoot "target\x86_64-pc-windows-msvc\release\dual-core-pdf-pipeline.exe"),
    (Join-Path $RepoRoot "target\x86_64-pc-windows-gnu\release\dual-core-pdf-pipeline.exe")
)

$ExePath = $CandidatePaths | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $ExePath) {
    $ExePath = Join-Path $RepoRoot "target\release\dual-core-pdf-pipeline.exe"
}

$WshShell = New-Object -comObject WScript.Shell
$Desktop = [Environment]::GetFolderPath("Desktop")
$ShortcutPath = Join-Path $Desktop "BankFidelity.lnk"
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = $ExePath
$Shortcut.Arguments = "gui"
$Shortcut.WorkingDirectory = $RepoRoot
$Shortcut.Description = "BankFidelity Dual-Core PDF Pipeline GUI"
$Shortcut.Save()
Write-Host "Desktop shortcut created at $ShortcutPath -> $ExePath (Exists: $(Test-Path $ExePath))"
