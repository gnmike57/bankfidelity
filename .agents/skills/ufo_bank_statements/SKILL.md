---
name: ufo_bank_statements
description: Instructions and orchestration logic for utilizing the Microsoft UFO agent to autonomously interact with and edit bank statements via the BankFidelity Dual-Core pipeline.
---

# UFO Bank Statement Orchestration

This skill provides the required knowledge and orchestration patterns to successfully instruct Microsoft UFO to work on bank statements through BankFidelity.

## Core Concepts

BankFidelity has a native, two-way integration with the Microsoft UFO UI agent:
1. **Rust to UFO (Telemetry)**: BankFidelity invokes UFO natively via `src/ai/ufo.rs`. The backend injects a `[BANKFIDELITY CONTEXT]` payload into the prompt, giving UFO immediate awareness of the current workspace.
2. **UFO to Rust (MCP)**: UFO connects back to BankFidelity's native JSON-RPC 2.0 MCP server (`src/ai/mcp.rs`) to retrieve credentials and access semantic tools.

## Using UFO for Bank Statements

When an agent needs to automate the editing or verification of a bank statement using UFO, they should follow this flow:

### 1. Launching the Orchestrator
UFO is invoked using BankFidelity's CLI `chat` subcommand or programmatically through `UfoClient::dispatch_task`.

Example CLI trigger:
```bash
cargo run -- chat -i statement.pdf "Analyze the transactions on page 1 and verify visual fidelity"
```

### 2. Available MCP Resources & Tools
UFO has access to the **100% complete** BankFidelity ecosystem via MCP:
- **Vision (`resources/read`)**: UFO can read `pdf-page://<path_to_pdf>?page=<number>` to receive a 150 DPI Base64 PNG image of the document, allowing it to "see" the statement before editing.
- **Semantic Guidance (`prompts/get`)**: UFO automatically pulls the `bankfidelity_agent_instructions` prompt. This forces UFO to balance high-speed data extraction (`extract_data`) against pixel-perfect typography (`modify_text` followed immediately by `verify_layout`).
- **Declarative Layout Reflowing (`typst_reconstruct`)**: If UFO detects that an edit has overflowed the physical bounding box, it MUST use `typst_reconstruct` to natively rebuild the PDF.
- **Batch Processing (`extract_batch`)**: UFO can autonomously chew through entire directories of statements at high speed using `extract_batch`.
- **Local AI Delegation (`local_ai_chat`)**: For highly complex financial analysis or intent routing, UFO can use `local_ai_chat` to delegate the reasoning directly to the local offline Qwen 7B model.
- **Transaction Transferring (`transfer_transactions`)**: UFO can instruct the backend to extract transactions from a source PDF and map them onto a totally different visual target layout.
- **Audit Extraction (`export_history`)**: UFO can pull the cryptographic `.audit` log to verify past manipulations on the statement.

### 3. Execution & Logs
UFO stores its logs in `C:\UFO\logs\<task_id>\output.md`. 
BankFidelity's `UfoClient` automatically awaits this file and streams it back to the Rust backend upon completion. 

To debug a failed UFO session, always check the `output.md` file in the `C:\UFO\logs\` directory corresponding to the specific execution ID.
