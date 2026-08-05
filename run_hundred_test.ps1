$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$env:PYTHON_EXECUTABLE = 'C:\Users\zbook\Downloads\python-3.15.0rc1-embed-amd64\python.exe'
cargo test hundred_real_pdf_operations -- --nocapture
