# Natural Language Control — User Guide

BankFidelity supports three ways to control the editor entirely through natural language:

1. **Command Palette** (in-app) — press `Ctrl+P` / `Cmd+P`
2. **CLI chat mode** — `./dual-core-pdf-pipeline chat`
3. **MCP server** — connect any AI agent (Claude, GPT-4, Cursor) via the MCP protocol

---

## Command Palette (In-App)

Press **Ctrl+P** (Windows/Linux) or **Cmd+P** (macOS) at any time to open the Command Palette. Type any natural language instruction and press **Enter**.

The palette uses a two-pass parser:
1. **Fast pattern matching** for common commands (undo, balance, verify, etc.)
2. **AI fallback** for complex edits — sent to your configured AI provider

### Examples

| What you type | What happens |
| :--- | :--- |
| `undo` | Undoes the last change |
| `balance` | Runs the Smart Balance Engine in preview mode |
| `balance to $5,432.10` | Balances the statement to exactly $5,432.10 |
| `balance apply all` | Runs Smart Balance and applies all changes immediately |
| `verify` | Runs pixel-perfect fidelity check |
| `extract transactions` | Extracts all transactions using the offline engine |
| `extract with gemini` | Extracts transactions using Gemini Vision |
| `shift dates forward 30 days` | Shifts all transaction dates by +30 days |
| `move dates back 14 days` | Shifts all transaction dates by -14 days |
| `transfer to ANZ` | Transfers transactions to an ANZ template |
| `categorize` | AI-categorizes all transactions |
| `doctor` | Runs full system health check |
| `reload keys` | Hot-reloads API keys from `.env` |
| `run stress test` | Runs the 42-pair transfer matrix |

### Complex Edits (AI-Assisted)

For edits that require understanding document context, the palette sends your instruction to the configured AI provider:

| What you type | What happens |
| :--- | :--- |
| `Change the account holder name to John Smith` | AI locates the name field and replaces it |
| `Set the closing balance to $12,500.00` | AI adjusts the final balance row |
| `Remove all transactions over $1000` | AI identifies and removes matching rows |
| `Replace all mentions of WOOLWORTHS with COLES` | AI finds all occurrences and replaces them |
| `Add a new transaction on 15 Jan for Coffee Shop $4.50` | AI inserts a new row with correct formatting |
| `Change the BSB to 062-000` | AI locates the BSB field and updates it |
| `Make this look like a Westpac statement` | AI reformats the layout to match Westpac style |

---

## CLI Chat Mode

Run the app in interactive chat mode from the terminal:

```bash
# Interactive chat (reads PDF path from first argument)
./dual-core-pdf-pipeline chat --pdf ~/Documents/statement.pdf

# Single command (non-interactive)
./dual-core-pdf-pipeline chat --pdf ~/Documents/statement.pdf --cmd "balance to $5000"

# Pipe commands from a file
cat commands.txt | ./dual-core-pdf-pipeline chat --pdf ~/Documents/statement.pdf

# Use a specific AI provider
./dual-core-pdf-pipeline chat --pdf ~/Documents/statement.pdf --provider mistral
```

### Chat Session Example

```
BankFidelity Chat — ANZ_Jan2026.pdf (4 pages, 47 transactions)
Type a command or 'help' for examples. Ctrl+C to exit.

> extract transactions
✓ Extracted 47 transactions (offline engine, 0.02s)

> what is the closing balance?
✓ Closing balance: $8,432.17 (page 4, row 47)

> set closing balance to $12,500.00
⚠ This will modify the PDF. Confirm? [y/N] y
✓ Applied: closing balance updated to $12,500.00

> verify
✓ Fidelity check PASS — 100.00% SSIM (4 pages)

> export change history as csv
✓ Saved: ANZ_Jan2026_history.csv (2 changes)

> undo
✓ Undone: closing balance reverted to $8,432.17
```

---

## HTTP Chat API

When running in `serve` mode, the `/chat` endpoint accepts natural language commands:

```bash
# Start the backend
./dual-core-pdf-pipeline serve

# Send a natural language command
curl -X POST http://localhost:8080/chat \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Change the account holder name to John Smith",
    "pdf_path": "/path/to/statement.pdf",
    "provider": "gemini",
    "auto_apply": false
  }'
```

Response:
```json
{
  "status": "ok",
  "intent": "ai_edit",
  "description": "AI edit via gemini — \"Change the account holder name to John Smith\"",
  "proposed_changes": [
    {
      "page": 1,
      "field": "account_holder",
      "old_value": "JANE DOE",
      "new_value": "John Smith",
      "confidence": 0.98
    }
  ],
  "job_id": "abc123"
}
```

---

## MCP Agent Integration

See [docs/MCP_SETUP.md](MCP_SETUP.md) for full instructions on connecting Claude Desktop, Cursor, or any other MCP-compatible AI agent.

Once connected, the AI can:
- Open and read bank statements
- Extract and analyse transactions
- Make precise edits based on your instructions
- Verify fidelity after every change
- Run stress tests and generate reports
- Manage API keys

---

## Supported Intent Categories

The NLP router (`src/app/nlp_router.rs`) recognises these intent categories:

| Intent | Trigger phrases |
| :--- | :--- |
| **Undo** | undo, revert, go back |
| **Redo** | redo, reapply |
| **Balance** | balance, reconcile, fix balance, auto-balance |
| **Verify** | verify, check fidelity, xray, ssim, pixel check |
| **Extract** | extract, get transactions, parse, read |
| **Transfer** | transfer to [bank], copy to [bank] |
| **Adjust dates** | shift dates, move dates, adjust dates |
| **Categorize** | categorize, classify, label, tag |
| **Doctor** | doctor, health check, diagnose |
| **Reload keys** | reload, refresh keys, hot reload |
| **Stress test** | stress test, transfer matrix, xray suite |
| **AI edit** | change, replace, set, update, edit, modify, add, remove, delete, insert, rename, fix, correct, adjust, rewrite |

---

## Adding Local LLM Support

To use a local Ollama model instead of cloud providers:

```bash
# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# Pull a model
ollama pull llama3.2

# Configure BankFidelity to use it
echo "LOCAL_LLM_URL=http://localhost:11434" >> bank-statement-fidelity-editor.env
echo "LOCAL_LLM_MODEL=llama3.2" >> bank-statement-fidelity-editor.env

# Build with local-llm feature
cargo build --release --features local-llm
```

Then in the Command Palette, append `using local llm` to any AI command:
```
Change the account holder name to John Smith using local llm
```
