# BankFidelity MCP Server — Setup Guide

The BankFidelity MCP (Model Context Protocol) server exposes every function of the editor as an AI-callable tool. Once connected, any MCP-compatible AI client (Claude Desktop, Cursor, Cline, Continue.dev, custom agents) can control the editor entirely through natural language.

---

## What the MCP Server Provides

The server exposes **23 tools** covering every operation in the editor:

| Category | Tools |
| :--- | :--- |
| **Document** | `load_document`, `get_document_info`, `list_available_pdfs`, `render_page` |
| **Extraction** | `extract_transactions`, `analyze_fonts` |
| **Editing** | `natural_language_edit`, `apply_change`, `balance_statement`, `adjust_dates`, `undo`, `redo` |
| **Workflows** | `workflow_full`, `transfer_transactions`, `categorize_transactions`, `generate_visual_alternatives` |
| **Verification** | `verify_fidelity`, `run_stress_test` |
| **History** | `export_change_history` |
| **Config** | `verify_api_keys`, `update_api_key`, `doctor`, `health_check` |

---

## Prerequisites

```bash
# Python 3.10+ required
python3 --version

# Install MCP server dependencies
pip3 install httpx pymupdf pdf2image scikit-image aiohttp
```

---

## Quick Start

### 1. Start the BankFidelity backend

```bash
# From the repo root
./dual-core-pdf-pipeline serve
# Backend is now running at http://localhost:8080
```

### 2. Start the MCP server (stdio mode — for Claude Desktop / Cursor)

```bash
python3 scripts/mcp_server.py
```

### 3. Start the MCP server (SSE mode — for web agents)

```bash
python3 scripts/mcp_server.py --transport sse --port 8765
# MCP endpoint: http://localhost:8765/mcp
# Tools list:   http://localhost:8765/tools
```

---

## Connecting to Claude Desktop

Add the following to your Claude Desktop configuration file:

**macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`  
**Windows:** `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "bankfidelity": {
      "command": "python3",
      "args": ["/path/to/bankfidelity/scripts/mcp_server.py"],
      "env": {
        "BANKFIDELITY_API_URL": "http://localhost:8080"
      }
    }
  }
}
```

Replace `/path/to/bankfidelity` with the actual path to your repo. Restart Claude Desktop after saving.

---

## Connecting to Cursor

Add to `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "bankfidelity": {
      "command": "python3",
      "args": ["scripts/mcp_server.py"],
      "env": {
        "BANKFIDELITY_API_URL": "http://localhost:8080"
      }
    }
  }
}
```

---

## Connecting to Cline / Continue.dev

Use the SSE transport:

```json
{
  "mcpServers": {
    "bankfidelity": {
      "url": "http://localhost:8765/mcp",
      "transport": "sse"
    }
  }
}
```

---

## Example Natural Language Commands

Once connected, you can ask your AI assistant:

```
"Load the ANZ statement from ~/Documents/anz_jan2026.pdf"
"Extract all transactions and show me the ones over $500"
"Change the account holder name to John Smith"
"Set the closing balance to $12,500.00"
"Shift all dates forward by 30 days"
"Transfer transactions to the Westpac template"
"Run a pixel-perfect fidelity check between the original and edited versions"
"Verify all my API keys are working"
"Run the full 42-pair transfer stress test"
"Show me page 3 of the statement as an image"
"Export the change history as a CSV"
```

---

## Offline Mode (No Backend Required)

The MCP server can operate without the running Rust backend for read-only operations. It uses PyMuPDF directly for:

- `get_document_info` — metadata extraction
- `extract_transactions` — raw text extraction
- `render_page` — page rendering
- `analyze_fonts` — font analysis
- `list_available_pdfs` — directory listing
- `verify_fidelity` — SSIM comparison (requires `scikit-image`)
- `verify_api_keys` — live API key testing
- `doctor` — system health check

Write operations (`natural_language_edit`, `apply_change`, `balance_statement`, etc.) require the backend to be running.

---

## Environment Variables

| Variable | Default | Description |
| :--- | :--- | :--- |
| `BANKFIDELITY_API_URL` | `http://localhost:8080` | Backend URL |
| `BANKFIDELITY_MCP_PORT` | `8765` | SSE transport port |
| `GEMINI_API_KEY` | — | Google Gemini API key |
| `MISTRAL_API_KEY` | — | Mistral AI API key |
| `LLAMAPARSE_API_KEY` | — | LlamaParse API key |
| `PYMUPDF_PRO_KEY` | — | PyMuPDF Pro key |

All variables are loaded automatically from `bank-statement-fidelity-editor.env` or `.env` in the repo root.

---

## Troubleshooting

**MCP server not connecting:**
```bash
# Test the MCP server directly
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | python3 scripts/mcp_server.py
```

**Backend not ready:**
```bash
# Check backend health
curl http://localhost:8080/health
curl http://localhost:8080/readyz
```

**API key issues:**
```bash
# Run the doctor check
python3 -c "
import asyncio, sys
sys.path.insert(0, 'scripts')
from mcp_server import tool_doctor
print(asyncio.run(tool_doctor()))
"
```
