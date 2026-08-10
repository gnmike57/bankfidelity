$ErrorActionPreference = "Continue"

Write-Host "Running cargo test and capturing output..."
# Using --nocapture so passed tests still output their eprintln! skips
# 2>&1 merges stderr into stdout
cargo test -- --nocapture > local_audit_evidence/test_output.log 2>&1

Write-Host "Extracting [skip] markers..."
Select-String -Path "local_audit_evidence/test_output.log" -Pattern "\[skip\]" -SimpleMatch | Out-File "local_audit_evidence/skips_found.log"

Write-Host "Done. See local_audit_evidence/skips_found.log"
