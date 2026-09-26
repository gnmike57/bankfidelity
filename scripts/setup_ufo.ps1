$ufoPath = "C:\ufo\ufo"

if (-Not (Test-Path $ufoPath)) {
    Write-Host "Cloning Microsoft UFO into $ufoPath..."
    git clone https://github.com/microsoft/UFO.git $ufoPath
}

$systemYamlPath = Join-Path $ufoPath "ufo\config\system.yaml"
if (Test-Path $systemYamlPath) {
    Write-Host "Updating system.yaml for optimal local LLM performance..."
    $content = Get-Content $systemYamlPath -Raw
    
    # Overwrite configs using RegEx to match existing properties or just append if simple
    $content = $content -replace 'SLEEP_TIME:\s*.*', 'SLEEP_TIME: 0.2'
    $content = $content -replace 'SAVE_EXPERIENCE:\s*.*', 'SAVE_EXPERIENCE: "always_not"'
    $content = $content -replace 'VISUAL_MODE:\s*.*', 'VISUAL_MODE: False'
    
    Set-Content -Path $systemYamlPath -Value $content
    Write-Host "Successfully patched system.yaml"
} else {
    Write-Host "Warning: $systemYamlPath not found. You may need to run python -m ufo to generate config first."
}

# Run the python patcher for the JSON parser robustness
$patcherScript = Join-Path $PWD "scripts\patch_ufo_parser.py"
if (Test-Path $patcherScript) {
    Write-Host "Patching UFO JSON parsers for Local LLM PascalCase issues..."
    $PYTHON_EXE = Join-Path $ufoPath "python_env\python.exe"
    $env:PYTHONIOENCODING = "utf-8"
    $env:PYTHONPATH = "$ufoPath;$PWD"
    & $PYTHON_EXE $patcherScript $ufoPath
}

Write-Host "UFO Setup Complete."

# ---------------------------------------------------------------------------
# Deploy the BankFidelity E2E architecture audit script into UFO's scripts/
# directory so the desktop launchers (01[9], 02[6], 04[4]) can invoke it.
# ---------------------------------------------------------------------------
$auditSource = Join-Path $PWD "scripts\audit_e2e_sequential.py"
$auditTarget = Join-Path $ufoPath "scripts\audit_e2e_sequential.py"
if (Test-Path $auditSource) {
    if (-Not (Test-Path (Join-Path $ufoPath "scripts"))) {
        New-Item -ItemType Directory -Force -Path (Join-Path $ufoPath "scripts") | Out-Null
    }
    Copy-Item -Force $auditSource $auditTarget
    Write-Host "Deployed E2E audit script -> $auditTarget"
} else {
    Write-Warning "audit_e2e_sequential.py not found at $auditSource; launchers will show a graceful error."
}



# ---------------------------------------------------------------------------
# Register the BankFidelity MCP Server so UFO can call back natively.
# Without this step the Rust->UFO leg works, but UFO cannot reach BankFidelity
# tools (modify_text, verify_layout, pdf-page:// vision, etc.). Idempotent.
# ---------------------------------------------------------------------------
$exeCandidates = @(
    Join-Path $PWD "target\release\dual-core-pdf-pipeline.exe"
    Join-Path $PWD "target\debug\dual-core-pdf-pipeline.exe"
)
$bankfidelityExe = $exeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($bankfidelityExe) {
    $mcpConfigDir = Join-Path $ufoPath "ufo\config"
    $mcpConfigPath = Join-Path $mcpConfigDir "mcp_servers.json"
    if (-Not (Test-Path $mcpConfigDir)) { New-Item -ItemType Directory -Force -Path $mcpConfigDir | Out-Null }

    $config = if (Test-Path $mcpConfigPath) {
        Get-Content $mcpConfigPath -Raw | ConvertFrom-Json
    } else {
        [PSCustomObject]@{ mcpServers = [PSCustomObject]@{} }
    }

    $entry = [PSCustomObject]@{
        command   = $bankfidelityExe
        args      = @("mcp")
        transport = "stdio"
    }

    if ($config.PSObject.Properties["mcpServers"]) {
        if ($config.mcpServers.PSObject.Properties["bankfidelity"]) {
            $config.mcpServers.bankfidelity = $entry
        } else {
            $config.mcpServers | Add-Member -MemberType NoteProperty -Name "bankfidelity" -Value $entry
        }
    } else {
        $config | Add-Member -MemberType NoteProperty -Name "mcpServers" -Value ([PSCustomObject]@{ bankfidelity = $entry })
    }

    $config | ConvertTo-Json -Depth 10 | Set-Content -Path $mcpConfigPath
    Write-Host "Registered BankFidelity MCP server ($bankfidelityExe) -> $mcpConfigPath"
} else {
    Write-Warning "BankFidelity binary not found in target\. Build first (cargo build --release), then rerun scripts/setup_ufo.ps1 to enable the UFO -> MCP tooling leg."
}
