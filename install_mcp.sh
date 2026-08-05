#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# BankFidelity — MCP + Dependency Auto-Installer
# Supports: Ubuntu/Debian, macOS (Homebrew)
# Usage:  bash install_mcp.sh [--no-rust] [--no-python] [--no-claude]
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_RUST=true
INSTALL_PYTHON=true
CONFIGURE_CLAUDE=true

for arg in "$@"; do
  case $arg in
    --no-rust)    INSTALL_RUST=false ;;
    --no-python)  INSTALL_PYTHON=false ;;
    --no-claude)  CONFIGURE_CLAUDE=false ;;
  esac
done

# ── Colour helpers ────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "${GREEN}[OK]${NC}  $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }
step() { echo -e "\n${YELLOW}▶ $*${NC}"; }

# ── Detect OS ─────────────────────────────────────────────────────────────────
if [[ "$OSTYPE" == "darwin"* ]]; then
  OS=macos
elif [[ -f /etc/debian_version ]]; then
  OS=debian
elif [[ -f /etc/redhat-release ]]; then
  OS=redhat
else
  OS=unknown
fi
ok "Detected OS: $OS"

# ── System dependencies ───────────────────────────────────────────────────────
step "Installing system dependencies"
if [[ $OS == "debian" ]]; then
  sudo apt-get update -qq
  sudo apt-get install -y -q \
    build-essential pkg-config libssl-dev \
    libfontconfig1-dev libfreetype6-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libgtk-3-dev \
    poppler-utils python3 python3-pip curl git
  ok "System packages installed"
elif [[ $OS == "macos" ]]; then
  if ! command -v brew &>/dev/null; then
    warn "Homebrew not found — installing..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  fi
  brew install pkg-config openssl@3 freetype fontconfig poppler python3 git
  ok "Homebrew packages installed"
else
  warn "Unknown OS — skipping system package install. Install manually: build-essential, libssl-dev, libfontconfig1-dev, poppler-utils"
fi

# ── Rust toolchain ────────────────────────────────────────────────────────────
if $INSTALL_RUST; then
  step "Installing Rust toolchain"
  if command -v rustup &>/dev/null; then
    ok "rustup already installed — updating"
    rustup update stable
  else
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi
  rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true
  ok "Rust $(rustc --version)"
fi

# ── Python dependencies ───────────────────────────────────────────────────────
if $INSTALL_PYTHON; then
  step "Installing Python MCP server dependencies"
  PIP="pip3"
  if ! command -v pip3 &>/dev/null; then
    PIP="python3 -m pip"
  fi
  $PIP install --quiet --upgrade \
    mcp \
    google-genai \
    mistralai \
    llama-parse \
    requests \
    scikit-image \
    pdf2image \
    pillow \
    python-dotenv
  ok "Python packages installed"
fi

# ── .env file ─────────────────────────────────────────────────────────────────
step "Checking .env file"
ENV_FILE="$SCRIPT_DIR/.env"
EXAMPLE_FILE="$SCRIPT_DIR/.env.example"
if [[ ! -f "$ENV_FILE" ]]; then
  if [[ -f "$EXAMPLE_FILE" ]]; then
    cp "$EXAMPLE_FILE" "$ENV_FILE"
    warn ".env created from .env.example — please fill in your API keys"
    warn "  Edit: $ENV_FILE"
    warn "  Or use the GUI: Settings → API Keys → Save & apply keys"
    warn "  Or use the web configurator: https://aikeyconfig-uqastysg.manus.space"
  else
    warn "No .env found — the app will run in offline mode only"
  fi
else
  ok ".env already exists"
fi

# ── Claude Desktop MCP config ─────────────────────────────────────────────────
if $CONFIGURE_CLAUDE; then
  step "Configuring Claude Desktop MCP integration"
  MCP_SCRIPT="$SCRIPT_DIR/scripts/mcp_server.py"
  if [[ ! -f "$MCP_SCRIPT" ]]; then
    warn "MCP server script not found at $MCP_SCRIPT — skipping Claude config"
  else
    if [[ $OS == "macos" ]]; then
      CLAUDE_CONFIG_DIR="$HOME/Library/Application Support/Claude"
    else
      CLAUDE_CONFIG_DIR="$HOME/.config/Claude"
    fi
    CLAUDE_CONFIG="$CLAUDE_CONFIG_DIR/claude_desktop_config.json"
    mkdir -p "$CLAUDE_CONFIG_DIR"

    MCP_ENTRY=$(cat <<MCPJSON
{
  "mcpServers": {
    "bankfidelity": {
      "command": "python3",
      "args": ["$MCP_SCRIPT"],
      "env": {
        "BANKFIDELITY_ENV": "$ENV_FILE"
      }
    }
  }
}
MCPJSON
)
    if [[ -f "$CLAUDE_CONFIG" ]]; then
      # Merge — add bankfidelity entry if not already present
      if grep -q '"bankfidelity"' "$CLAUDE_CONFIG" 2>/dev/null; then
        ok "Claude Desktop already has bankfidelity MCP entry"
      else
        # Use Python to merge JSON safely
        python3 - <<PYEOF
import json, sys
with open("$CLAUDE_CONFIG") as f:
    cfg = json.load(f)
cfg.setdefault("mcpServers", {})["bankfidelity"] = {
    "command": "python3",
    "args": ["$MCP_SCRIPT"],
    "env": {"BANKFIDELITY_ENV": "$ENV_FILE"}
}
with open("$CLAUDE_CONFIG", "w") as f:
    json.dump(cfg, f, indent=2)
print("Merged bankfidelity into existing Claude config")
PYEOF
        ok "bankfidelity MCP added to Claude Desktop config"
      fi
    else
      echo "$MCP_ENTRY" > "$CLAUDE_CONFIG"
      ok "Claude Desktop config created at $CLAUDE_CONFIG"
    fi
    ok "Restart Claude Desktop to activate the bankfidelity MCP tools"
  fi
fi

# ── Cursor / VS Code MCP config ───────────────────────────────────────────────
step "Generating Cursor/VS Code MCP config snippet"
MCP_SNIPPET="$SCRIPT_DIR/docs/cursor_mcp_config.json"
cat > "$MCP_SNIPPET" <<CURSORJSON
{
  "mcpServers": {
    "bankfidelity": {
      "command": "python3",
      "args": ["$SCRIPT_DIR/scripts/mcp_server.py"],
      "env": {
        "BANKFIDELITY_ENV": "$SCRIPT_DIR/.env"
      }
    }
  }
}
CURSORJSON
ok "Cursor/VS Code snippet saved to $MCP_SNIPPET"
echo "  Add the above JSON to: ~/.cursor/mcp.json  or  .vscode/mcp.json"

# ── Build the Rust binary ─────────────────────────────────────────────────────
if $INSTALL_RUST; then
  step "Building BankFidelity (release) — this takes 5-15 min on first run"
  cd "$SCRIPT_DIR"
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env" 2>/dev/null || true
  if cargo build --release 2>&1 | tee /tmp/bankfidelity_build.log | grep -E "^error|Compiling bankfidelity|Finished"; then
    ok "Build complete: $SCRIPT_DIR/target/release/dual-core-pdf-pipeline"
  else
    fail "Build failed — see /tmp/bankfidelity_build.log"
  fi
fi

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}  BankFidelity installation complete!${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Launch GUI:        ./target/release/dual-core-pdf-pipeline"
echo "  Launch server:     ./target/release/dual-core-pdf-pipeline serve"
echo "  MCP server only:   python3 scripts/mcp_server.py"
echo "  Chat mode:         ./target/release/dual-core-pdf-pipeline chat --pdf statement.pdf"
echo "  Verify API keys:   ./target/release/dual-core-pdf-pipeline verify-api-keys"
echo "  Doctor check:      ./target/release/dual-core-pdf-pipeline doctor"
echo ""
