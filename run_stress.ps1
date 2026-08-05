$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path

$pythonDir = "C:\bankfidelity\bankfidelity\.bin\python-3.12"
if (-not (Test-Path "$pythonDir\python.exe")) {
    Write-Host "Bootstrapping isolated Python 3.12..."
    New-Item -ItemType Directory -Force -Path $pythonDir | Out-Null
    Invoke-WebRequest -Uri "https://www.python.org/ftp/python/3.12.5/python-3.12.5-embed-amd64.zip" -OutFile "python.zip"
    Expand-Archive -Path "python.zip" -DestinationPath $pythonDir -Force
    Remove-Item "python.zip"
    
    # Enable site-packages for pip
    (Get-Content "$pythonDir\python312._pth") -replace '#import site', 'import site' | Set-Content "$pythonDir\python312._pth"
    
    Invoke-WebRequest -Uri "https://bootstrap.pypa.io/get-pip.py" -OutFile "get-pip.py"
    & "$pythonDir\python.exe" get-pip.py
    & "$pythonDir\python.exe" -m pip install pymupdf pymupdfpro pillow numpy
    Remove-Item "get-pip.py"
}

$env:PYTHON_EXECUTABLE = "$pythonDir\python.exe"

Remove-Item Env:\AI_PROVIDER -ErrorAction SilentlyContinue
Remove-Item Env:\GEMINI_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:\APPLITOOLS_API_KEY -ErrorAction SilentlyContinue
Remove-Item Env:\DUAL_CORE_PASSPHRASE -ErrorAction SilentlyContinue
cargo test --test au_transfer_stress -- --nocapture --ignored
