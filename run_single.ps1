$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$pythonDir = "C:\bankfidelity\bankfidelity\.bin\python-3.12"
$env:PYTHON_EXECUTABLE = "$pythonDir\python.exe"

if (Test-Path ".env") {
    Get-Content .env | Where-Object { $_ -match "^[^#].*=" } | ForEach-Object {
        $name, $value = $_ -split '=', 2
        $name = $name.Trim()
        $value = $value.Trim().Trim('"').Trim("'")
        [Environment]::SetEnvironmentVariable($name, $value)
    }
}

cargo run -- transfer-transactions --source-pdf "AU Bank Statements/bankwest_example.pdf" --target-pdf "AU Bank Statements/fallback.pdf" --output "audit/single_transfer_output.pdf"
