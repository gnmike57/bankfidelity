$ErrorActionPreference = "Continue"
Write-Host "Running tests for skips..."
cargo test -- --nocapture > local_audit_evidence/test_output.log 2>&1
Select-String -Path "local_audit_evidence/test_output.log" -Pattern "\[skip\]" -SimpleMatch | Out-File "local_audit_evidence/skips_found.log"

Write-Host "Running linters..."
cargo fmt --check > local_audit_evidence/fmt_check.log 2>&1
cargo clippy --all-targets -- -D warnings > local_audit_evidence/clippy_check.log 2>&1

Write-Host "Running CLI traversal..."
.\local_audit_evidence\traverse_cli.ps1
Write-Host "All tasks complete."
