$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $Root

if (-not $env:PYO3_PYTHON) { $env:PYO3_PYTHON = "python" }
if (-not $env:DUAL_CORE_PASSPHRASE) { $env:DUAL_CORE_PASSPHRASE = "base-state-verification-only" }
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = "2" }

Write-Host "[1/8] rustfmt"
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[2/8] Python production bridge"
python python/smoke_test.py
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[3/8] all host targets"
cargo check --locked --all-targets --message-format=short
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[4/8] production lint"
cargo clippy --locked --lib --bins -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[5/8] library tests"
cargo test --locked --lib --no-fail-fast
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[6/8] portable runtime smoke"
cargo test --locked --test runtime_smoke
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[7/8] configuration-free startup contract"
cargo test --locked --test cli_startup_contract
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "[8/8] production executable"
cargo build --locked --bin dual-core-pdf-pipeline
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& "target/debug/dual-core-pdf-pipeline.exe" --version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& "target/debug/dual-core-pdf-pipeline.exe" --help | Out-Null
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Dirty = git status --porcelain --untracked-files=all
if ($Dirty) {
    Write-Error "verification left the working tree dirty:`n$Dirty"
    exit 1
}

Write-Host "BASE STATE: PASS"
