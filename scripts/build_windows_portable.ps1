$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root

$Version = (Get-Content Cargo.toml | Select-String '^version\s*=\s*"(.*?)"$').Matches.Groups[1].Value
$Revision = if ($env:GITHUB_SHA) { $env:GITHUB_SHA } else { (git rev-parse HEAD).Trim() }
$Output = if ($args.Count -gt 0) { $args[0] } else { "target/release/portable/windows-x86_64" }

cargo build --locked --release --bin dual-core-pdf-pipeline
$UFO_ROOT = "C:\ufo\ufo"
$BF_DIR = "C:\bankfidelity\bankfidelity"
$PYTHON_EXE = "$UFO_ROOT\python_env\python.exe"
$env:PYTHONIOENCODING = "utf-8"
$env:PYTHONPATH = "$UFO_ROOT;$BF_DIR"

& $PYTHON_EXE scripts/build_portable_bundle.py `
  --platform windows-x86_64 `
  --binary target/release/dual-core-pdf-pipeline.exe `
  --output $Output `
  --revision $Revision `
  --version $Version

$Artifacts = "target/release/artifacts"
New-Item -ItemType Directory -Force -Path $Artifacts | Out-Null
$Archive = Join-Path $Artifacts "BankStatementFidelityEditor-$Version-windows-x86_64.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path (Join-Path $Output "BankStatementFidelityEditor") -DestinationPath $Archive -CompressionLevel Optimal
$Hash = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
"$Hash  $(Split-Path -Leaf $Archive)" | Set-Content -Encoding ascii "$Archive.sha256"
Write-Host "Created unsigned portable bundle: $Archive"
