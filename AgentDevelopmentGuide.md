# Agent Development Guide

Quick-reference for AI agents working on the Bank Statement Fidelity Editor v1.0.0.

## Project type

Rust desktop/CLI project using Cargo.

The project includes GUI (egui), CLI, Python bridge (PyO3), Node.js bridge (Applitools),
PDF processing, multi-backend AI integrations, tests, scripts, and CI configuration.

## Autonomy level

The agent is allowed to operate with high autonomy for development, debugging, repair, and validation tasks.

The agent may:

- inspect the repository
- inspect build/test/runtime logs
- modify source files
- modify project configuration
- modify tests
- modify scripts
- modify CI workflows
- run terminal commands needed for diagnosis and validation
- install or select the required Rust toolchain
- run formatting, linting, build, and test commands
- repeat the diagnose → fix → validate loop until the issue is resolved or blocked

The agent should prefer durable project-level fixes over local temporary workarounds.

## Preferred Rust toolchain

Use Rust 1.88.0 unless a task explicitly requires another version.

If `rust-toolchain.toml` contains:

```toml
channel = "dev"
```

replace it with:

```toml
[toolchain]
channel = "1.88.0"
components = ["rustfmt", "clippy"]
```

## Key source locations

| Area | Path | Purpose |
|---|---|---|
| Config + API Availability | `src/app/config.rs` | `AppConfig`, `ApiAvailability`, mode enums |
| Backend Preferences UI | `src/app/modals.rs` | `draw_backend_preferences()` |
| Runtime Job Loop | `src/app/runtime.rs` | All `Job::*` handlers, fallback logic |
| GUI State | `src/app/gui.rs` | `MyApp` struct, lifecycle |
| Offline Parser | `src/engine/offline_parser.rs` | Local text extraction fallback |
| Balance Engine | `src/engine/balance.rs` | `process_and_reconcile()` |
| Verification | `src/engine/verification.rs` | Multi-layer visual verification |
| Typst Reconstruct | `src/engine/typst_engine.rs` | Ultimate PDF rebuild fail-safe |
| Gemini Client | `src/ai/gemini_client.rs` | AI validation, balance, vision |
| Document AI | `src/ai/document_ai.rs` | Google ML parsing |
| Mindee Parser | `src/ai/mindee.rs` | Mindee Financial Document API |
| LlamaParse | `src/ai/llamaparse.rs` | LLM-based document parser |
| pdfRest | `src/ai/pdfrest.rs` | Cloud PDF rendering |
| Applitools Bridge | `src/ai/applitools_bridge.js` | Node.js visual AI bridge |
| PyO3 Bridge | `src/ai/pyo3_bridge.rs` | Python actor thread |
| PDF Engine Selector | `src/pdf/selector.rs` | Primary/fallback engine dispatch |

## Adding a new backend

1. Add the API key to `AppConfig` in `src/app/config.rs`
2. Add availability check to `ApiAvailability`
3. Add variant to the relevant mode enum
4. Add UI toggle in `draw_backend_preferences()` in `src/app/modals.rs`
5. Add handler logic in `src/app/runtime.rs` with offline fallback
6. Add to `.env.example` with signup URL
7. Update `README.md` and `CHANGELOG.md`

## Validation commands

```bash
cargo fmt
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
