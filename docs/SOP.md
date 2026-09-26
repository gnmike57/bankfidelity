# BankFidelity Standard Operating Procedure (SOP)

This document outlines the standard, foolproof workflow for operating the BankFidelity Dual-Core AI architecture alongside the Microsoft UFO visual agent. 

## 1. System Initialization
Before initiating any workflows, ensure the architecture is running securely.

1. **Start the Local LLM**: Ensure `llama-server.exe` (Qwen 2.5 Coder 7B GGUF) is running on `127.0.0.1:11434`. This handles all NLU routing, offline forensics, and JSON structure parsing without leaking data to the cloud.
2. **Start the Rust Orchestrator**: Run `cargo run --release -- gui` to launch the immediate-mode Egui dashboard, or use the CLI commands (`cargo run -- <command>`).

## 2. Ingestion & Preflight (Dual-Core)
All documents must pass through the Dual-Core ingestion engine.

1. **Load PDF**: Drop the target Bank Statement into BankFidelity.
2. **First-Pass (Heuristics)**: The `offline_heuristic` engine runs a rapid Rust-native regex and bounding box extraction.
3. **Second-Pass (AI Vision)**: The system verifies the data using PyMuPdf. If structural anomalies are detected, the system will transparently fallback to Document AI or LlamaParse if authorized via Backend Preferences.

## 3. Automation via Microsoft UFO
To automate manual GUI interactions or complex web-to-PDF downloads, utilize the UFO UI agent.

1. **Direct Telemetry Shelling**: From BankFidelity, trigger UFO via the CLI:
   `cargo run -- ufo --request "Download the Chase PDF from Chrome and save to Desktop"`
   BankFidelity will automatically prepend the `[BANKFIDELITY CONTEXT]` block, giving UFO awareness of the local workspace.
2. **MCP Autonomous Tooling**: UFO will connect natively to the BankFidelity MCP Server to execute tools. UFO has been strictly instructed via `prompts/get` to balance extraction speed (`extract_batch`) with perfect layout aesthetics (`modify_text` + `verify_layout`).
3. **Visual Verification**: UFO will natively pull `pdf-page://<path>?page=<num>` to receive a 150 DPI rendering of the target document, allowing it to "see" exactly what the Rust backend sees.

### MCP Surface & Security Note
The **canonical MCP server** for the UFO loop is the native Rust server (`cargo run -- mcp`, `src/ai/mcp.rs`). It speaks JSON-RPC 2.0 over **stdio only** — it never binds a network port — so only local processes you launch can invoke its tools. Treat any process that can spawn this binary as trusted with full document-editing capability.

`scripts/mcp_server.py` (HTTP/SSE on :8765) is an auxiliary bridge for remote/web agents and is **not** part of the UFO loop; avoid running both surfaces with divergent expectations.

Run `scripts/setup_ufo.ps1` once after building: it clones/patches UFO **and registers the BankFidelity MCP server** into UFO's config (`ufo/config/mcp_servers.json`) so the Rust → UFO → MCP → Rust loop works without manual steps.

## 4. Troubleshooting & Fallbacks

> [!CAUTION]
> If math or layout integrity fails, ALWAYS follow these fallbacks.

- **Mathematical Imbalance**: If an edit breaks the ledger math, trigger the "Ask Local AI to Explain" forensic button in the UI. BankFidelity will securely query Qwen 7B offline to explain the discrepancy.
- **Layout Overflows**: If UFO performs a `modify_text` operation that exceeds the physical dimensions of the document (causing catastrophic visual overlap), UFO will autonomously trigger `typst_reconstruct` to declaratively rebuild the entire PDF from scratch.
- **NLU Hanging**: The local AI bridge has a strict 90-second timeout. If the AI hangs, restart the `llama-server.exe` process.
