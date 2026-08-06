---
name: bankfidelity
description: Instructions and architectural context for developing the BankFidelity Dual-Core Orchestrator (Rust + UFO + Qwen 7B). Load this skill to understand how to build, run, and orchestrate the project.
---

# BankFidelity + UFO Dual-Core Orchestrator

You are working on **BankFidelity**, a secure, local, dual-core AI system designed for parsing, auditing, and balancing PDF bank statements with 100% cryptographic visual fidelity.

## 1. Architectural Overview
The architecture is composed of two natively bridged halves:
1. **The Rust Orchestrator (`C:\bankfidelity\bankfidelity`)**: The high-performance backend.
   - **UI**: Uses `egui` (immediate mode GUI).
   - **Async**: Uses `tokio` for background jobs.
   - **Math**: Uses `rust_decimal` for exact cryptographic statement balancing.
   - **Role**: Primary data processor, semantic intent router, and MCP Server.
2. **The UFO UI Agent (`C:\UFO`)**: Microsoft UFO (Python) configured as the visual desktop agent. It executes natively and securely, pulling semantic context directly from the Rust orchestrator via stdio.

## 2. Local AI Integration (Qwen 2.5 Coder 7B)
The entire pipeline is migrated to a locally hosted **Qwen 2.5 Coder 7B** GGUF model running on `127.0.0.1:11434`.

- **BankFidelity NLU**: `src/app/nlp_router.rs` routes all NLP intents directly to the local Qwen model. 
  - **Command**: Run natural language edits from the CLI via `cargo run -- chat -i <pdf> "<intent>"`.
- **Forensic UI**: When a mathematical balance breaks during a manual edit, BankFidelity queries Qwen 7B in the background to explain the imbalance. The forensic output streams directly into the `egui` interface.
- **UFO Alignment**: UFO's `agents.yaml`, `system.yaml`, and `mcp.yaml` are strictly targeted to the `qwen2.5-coder-7b-instruct-q4_k_m` local model.

## 3. The MCP Context Bridge
To **maximize common understanding**, the Rust backend operates a fully robust Model Context Protocol (MCP) server (`src/ai/mcp.rs`) that UFO natively connects to via `C:\UFO\ufo\client\mcp\configs\bankfidelity.json`.

The MCP Server natively provides:
1. **Semantic Knowledge**: `prompts/get` -> `bankfidelity_agent_instructions`
   - *Directive 1*: Always prioritize flawless typography (sequence `modify_text` + `verify_layout`).
   - *Directive 2*: Balance rapid data extraction (`extract_data`).
   - *Directive 3*: **Automatic Bank Statement Protocol**: When directed to process a bank statement, strictly ingest using `extract_data`, verify using `local_ai_chat`, and format using Directive 1. Never read PDFs manually by eye.
   - *Directive 4*: **Multimodal Vision**: If you need visual context of a page, request `pdf-page://<absolute_path>?page=<N>` via `resources/read`.
2. **Dynamic Resources**: `resources/list` natively resolves and reads BankFidelity's active `task.md` and `walkthrough.md` from the IDE's dynamically resolved brain directory (using `.gemini/antigravity/brain`).
3. **Execution Tools**: UFO natively executes `balance_statement`, `modify_text`, `extract_data`, and `verify_layout` directly against the Rust binary.

## 4. Operational Commands
- **Run GUI**: `cargo run --release -- gui`
- **Run CLI NLU**: `cargo run -- chat -i statement.pdf "your command"`
- **Compile/Check**: `cargo check`

## 5. Security Posture
- Secrets (e.g., `PYMUPDF_PRO_KEY`, `GEMINI_API_KEY`) are managed in `.env` and piped dynamically into UFO's local JSON config. Never hardcode keys into the repository.
