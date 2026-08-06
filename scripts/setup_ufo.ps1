$ufoPath = Join-Path $HOME "UFO"

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
    python $patcherScript $ufoPath
}

Write-Host "UFO Setup Complete."
