$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root

$Version = "1.1.1"
$Revision = if ($env:GITHUB_SHA) { $env:GITHUB_SHA } else { (git rev-parse HEAD).Trim() }
$Output = if ($args.Count -gt 0) { $args[0] } else { "target/release/portable/windows-x86_64" }

cargo build --locked --release --bin dual-core-pdf-pipeline
python scripts/build_portable_bundle.py `
  --platform windows-x86_64 `
  --binary target/release/dual-core-pdf-pipeline.exe `
  --output $Output `
  --revision $Revision

$Artifacts = "target/release/artifacts"
New-Item -ItemType Directory -Force -Path $Artifacts | Out-Null
$Archive = Join-Path $Artifacts "BankStatementFidelityEditor-$Version-windows-x86_64.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path (Join-Path $Output "BankStatementFidelityEditor") -DestinationPath $Archive -CompressionLevel Optimal
$Hash = (Get-FileHash -Algorithm SHA256 $Archive).Hash.ToLowerInvariant()
"$Hash  $(Split-Path -Leaf $Archive)" | Set-Content -Encoding ascii "$Archive.sha256"
Write-Host "Created unsigned portable bundle: $Archive"
