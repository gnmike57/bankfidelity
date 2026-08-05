# ─────────────────────────────────────────────────────────────────────────────
# BankFidelity - MCP + Dependency Auto-Installer (Windows PowerShell)
# Run as: powershell -ExecutionPolicy Bypass -File install_mcp.ps1
# Optional flags: -NoRust  -NoPython  -NoClaude
# ─────────────────────────────────────────────────────────────────────────────
param(
    [switch]$NoRust,
    [switch]$NoPython,
    [switch]$NoClaude
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

function Write-Ok   { param($msg) Write-Host "[OK]  $msg" -ForegroundColor Green }
function Write-Warn { param($msg) Write-Host "[WARN] $msg" -ForegroundColor Yellow }
function Write-Step { param($msg) Write-Host "`n▶ $msg" -ForegroundColor Cyan }
function Write-Fail { param($msg) Write-Host "[FAIL] $msg" -ForegroundColor Red; exit 1 }

# ── Check for winget / Chocolatey ─────────────────────────────────────────────
Write-Step "Checking package managers"
$HasWinget = Get-Command winget -ErrorAction SilentlyContinue
$HasChoco  = Get-Command choco  -ErrorAction SilentlyContinue

# ── Rust toolchain ────────────────────────────────────────────────────────────
if (-not $NoRust) {
    Write-Step "Installing Rust toolchain"
    $RustupExe = "$env:TEMP\rustup-init.exe"
    if (Get-Command rustup -ErrorAction SilentlyContinue) {
        Write-Ok "rustup already installed - updating"
        rustup update stable
    } else {
        Write-Warn "Downloading rustup-init.exe..."
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $RustupExe
        & $RustupExe -y --default-toolchain stable
        $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
        $env:PATH = "$($env:PATH);$cargoBin"
    }
    Write-Ok "Rust $(rustc --version)"
}

# ── Visual C++ Build Tools ────────────────────────────────────────────────────
Write-Step "Checking Visual C++ Build Tools"
$VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $VsWhere) {
    Write-Ok "Visual Studio / Build Tools found"
} else {
    Write-Warn "Visual C++ Build Tools not found"
    if ($HasWinget) {
        Write-Warn "Installing via winget (this may take several minutes)..."
        winget install --id Microsoft.VisualStudio.2022.BuildTools --silent --override "--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
        Write-Ok "Build Tools installed"
    } else {
        Write-Warn "Please install manually: https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    }
}

# ── Python dependencies ───────────────────────────────────────────────────────
if (-not $NoPython) {
    Write-Step "Installing Python MCP server dependencies"
    if (-not (Get-Command python3 -ErrorAction SilentlyContinue) -and -not (Get-Command python -ErrorAction SilentlyContinue)) {
        if ($HasWinget) {
            winget install --id Python.Python.3.11 --silent
            $env:PATH += ";$env:LOCALAPPDATA\Programs\Python\Python311;$env:LOCALAPPDATA\Programs\Python\Python311\Scripts"
        } else {
            Write-Warn "Python not found. Install from https://python.org/downloads and re-run."
        }
    }
    if (Get-Command pip3 -ErrorAction SilentlyContinue) {
        pip3 install --quiet --upgrade mcp google-genai mistralai llama-parse requests scikit-image pdf2image pillow python-dotenv
    } else {
        # Check if Python is in PATH, if not fallback to the AppData installation path just in case
        $PythonPath = if (Get-Command python -ErrorAction SilentlyContinue) { "python" } else { "C:\Users\zbook\AppData\Local\Programs\Python\Python312\python.exe" }
        & $PythonPath -m pip install --quiet --upgrade mcp google-genai mistralai llama-parse requests scikit-image pdf2image pillow python-dotenv
    }
    Write-Ok "Python packages installed"
}

# ── poppler (for pdf2image) ───────────────────────────────────────────────────
Write-Step "Checking poppler (required for PDF screenshot generation)"
$PopplerPath = "$env:USERPROFILE\poppler\Library\bin"
if (Test-Path "$PopplerPath\pdftoppm.exe") {
    Write-Ok "poppler already installed at $PopplerPath"
} else {
    if ($HasChoco) {
        choco install poppler -y
        Write-Ok "poppler installed via Chocolatey"
    } elseif ($HasWinget) {
        Write-Warn "poppler not available via winget. Downloading pre-built binary..."
        $PopplerZip = "$env:TEMP\poppler.zip"
        Invoke-WebRequest -Uri "https://github.com/oschwartz10612/poppler-windows/releases/download/v24.02.0-0/Release-24.02.0-0.zip" -OutFile $PopplerZip
        Expand-Archive -Path $PopplerZip -DestinationPath "$env:USERPROFILE\poppler" -Force
        $env:PATH += ";$PopplerPath"
        Write-Ok "poppler installed at $PopplerPath"
        Write-Warn "Add $PopplerPath to your system PATH permanently via System Properties → Environment Variables"
    } else {
        Write-Warn "poppler not installed. PDF screenshot generation will not work."
        Write-Warn "Install manually: https://github.com/oschwartz10612/poppler-windows/releases"
    }
}

# ── .env file ─────────────────────────────────────────────────────────────────
Write-Step "Checking .env file"
$EnvFile    = Join-Path $ScriptDir ".env"
$ExampleEnv = Join-Path $ScriptDir ".env.example"
if (-not (Test-Path $EnvFile)) {
    if (Test-Path $ExampleEnv) {
        Copy-Item $ExampleEnv $EnvFile
        Write-Warn ".env created from .env.example - please fill in your API keys"
        Write-Warn "  Edit: $EnvFile"
        Write-Warn "  Or use the GUI: Settings -> API Keys -> Save and apply keys"
        Write-Warn "  Or use the web configurator: https://aikeyconfig-uqastysg.manus.space"
    } else {
        Write-Warn "No .env found - app will run in offline mode only"
    }
} else {
    Write-Ok ".env already exists"
}

# ── Claude Desktop MCP config ─────────────────────────────────────────────────
if (-not $NoClaude) {
    Write-Step "Configuring Claude Desktop MCP integration"
    $McpScript = Join-Path $ScriptDir "scripts\mcp_server.py"
    if (-not (Test-Path $McpScript)) {
        Write-Warn "MCP server script not found at $McpScript - skipping Claude config"
    } else {
        $ClaudeConfigDir = Join-Path $env:APPDATA "Claude"
        $ClaudeConfig    = Join-Path $ClaudeConfigDir "claude_desktop_config.json"
        if (-not (Test-Path $ClaudeConfigDir)) { New-Item -ItemType Directory -Path $ClaudeConfigDir | Out-Null }

        $McpEntry = @{
            mcpServers = @{
                bankfidelity = @{
                    command = "python3"
                    args    = @($McpScript)
                    env     = @{ BANKFIDELITY_ENV = $EnvFile }
                }
            }
        }

        if (Test-Path $ClaudeConfig) {
            $existing = Get-Content $ClaudeConfig -Raw | ConvertFrom-Json
            if ($existing.mcpServers -and $existing.mcpServers.bankfidelity) {
                Write-Ok "Claude Desktop already has bankfidelity MCP entry"
            } else {
                if (-not $existing.mcpServers) { $existing | Add-Member -MemberType NoteProperty -Name mcpServers -Value @{} }
                $existing.mcpServers | Add-Member -MemberType NoteProperty -Name bankfidelity -Value $McpEntry.mcpServers.bankfidelity
                $existing | ConvertTo-Json -Depth 10 | Set-Content $ClaudeConfig
                Write-Ok "bankfidelity MCP added to Claude Desktop config"
            }
        } else {
            $McpEntry | ConvertTo-Json -Depth 10 | Set-Content $ClaudeConfig
            Write-Ok "Claude Desktop config created at $ClaudeConfig"
        }
        Write-Ok "Restart Claude Desktop to activate the bankfidelity MCP tools"
    }
}

# ── Cursor / VS Code MCP config ───────────────────────────────────────────────
Write-Step "Generating Cursor/VS Code MCP config snippet"
$CursorSnippet = Join-Path $ScriptDir "docs\cursor_mcp_config.json"
$McpSnippetContent = @{
    mcpServers = @{
        bankfidelity = @{
            command = "python3"
            args    = @((Join-Path $ScriptDir "scripts\mcp_server.py"))
            env     = @{ BANKFIDELITY_ENV = $EnvFile }
        }
    }
} | ConvertTo-Json -Depth 10
Set-Content -Path $CursorSnippet -Value $McpSnippetContent
Write-Ok "Cursor/VS Code snippet saved to $CursorSnippet"
Write-Warn "Add the above JSON to: %USERPROFILE%\.cursor\mcp.json  or  .vscode\mcp.json"

# ── Build the Rust binary ─────────────────────────────────────────────────────
if (-not $NoRust) {
    Write-Step "Building BankFidelity (release) - this takes 5-15 min on first run"
    Push-Location $ScriptDir
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    $env:PATH = "$($env:PATH);$cargoBin"
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Write-Fail "Build failed - check output above" }
    Write-Ok "Build complete: $ScriptDir\target\release\dual-core-pdf-pipeline.exe"
    Pop-Location
}

# ── Done ──────────────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Green
Write-Host "  BankFidelity installation complete!" -ForegroundColor Green
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Green
Write-Host ""
Write-Host "  Launch GUI:        .\target\release\dual-core-pdf-pipeline.exe"
Write-Host "  Launch server:     .\target\release\dual-core-pdf-pipeline.exe serve"
Write-Host "  MCP server only:   python3 scripts\mcp_server.py"
Write-Host "  Chat mode:         .\target\release\dual-core-pdf-pipeline.exe chat --pdf statement.pdf"
Write-Host "  Verify API keys:   .\target\release\dual-core-pdf-pipeline.exe verify-api-keys"
Write-Host "  Doctor check:      .\target\release\dual-core-pdf-pipeline.exe doctor"
Write-Host ""
