$ErrorActionPreference = "Stop"
Write-Host "== Project Doctor =="

if (-not (Test-Path "Cargo.toml")) {
    Write-Host "ERROR: Cargo.toml not found. Run from the project root."
    exit 1
}

@'
[toolchain]
channel = "1.89.0"
components = ["rustfmt", "clippy"]
'@ | Set-Content "rust-toolchain.toml"

if (Get-Command rustup -ErrorAction SilentlyContinue) {
    rustup install 1.89.0
    rustup override unset
} else {
    Write-Host "WARNING: rustup not found. Install from https://rustup.rs/"
}

if (Get-Command rustc -ErrorAction SilentlyContinue) { rustc --version }
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    cargo --version
    cargo check
} else {
    Write-Host "WARNING: cargo not found. Skipping cargo check."
}

Write-Host "Project doctor completed."
