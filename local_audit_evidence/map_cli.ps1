$ErrorActionPreference = "Continue"
Write-Host "Mapping CLI surface area..."
cargo run -- --help > local_audit_evidence/cli_baseline.log 2>&1
Write-Host "Done. See local_audit_evidence/cli_baseline.log"
