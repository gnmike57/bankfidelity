# Contributing to Bank Statement Fidelity Editor

We welcome contributions to the Bank Statement Fidelity Editor! This project follows strict guidelines to maintain the cryptographic integrity, mathematical accuracy, and pixel-perfect visual fidelity of the editing engine.

## Development Setup

Follow [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) from a clean checkout. The executable base state does not require cloud credentials. Install `requirements-ci.txt`, then run the platform verification command:

```bash
./scripts/verify-base-state.sh
```

On Windows PowerShell, run:

```powershell
./scripts/verify-base-state.ps1
```

Copy `.env.example` to `.env` only when testing an optional provider or licensed capability that explicitly requires it.

## Running the Test Suite

The project has a comprehensive test suite covering Rust unit tests, Python integration tests, and full end-to-end (E2E) PDF manipulation tests.

### 1. Rust Unit & Integration Tests
```bash
cargo test
```
*Note: Some tests require AI provider keys in your `.env` file. Tests without keys will gracefully skip or fall back to offline modes.*

### 2. End-to-End Transfer Stress Matrix
This test runs a 42-pair transfer matrix across all 7 supported Australian bank templates, ensuring that transactions can be moved between banks while maintaining mathematical balance and visual fidelity.

```bash
cargo test --test au_transfer_stress -- --ignored --nocapture
```

### 3. Python E2E Pipeline
Simulates the exact GUI flow (click, edit, apply, render, diff) via the CLI.

```bash
python3 scripts/e2e_pipeline.py --build --strict
```

## Architectural Invariants

When contributing to the core engine, you must respect the following invariants (enforced by Gate 07/08):

1.  **No Widen-on-Fail:** Verification thresholds (e.g., SSIM structural floor of `0.85`) are immutable. The engine must never silently widen a mask or lower a threshold to force a pass.
2.  **Explicit Provider Outcomes:** Cloud AI providers (Vision, Document AI, pdfRest) are optional and additive. Their outcomes must be explicitly recorded as `PASS`, `FAIL`, or `UNAVAILABLE`. A provider failure must never be converted into a local pass.
3.  **Mandatory Offline NLU (Qwen 7B):** Per our Dual-Core architecture, all sensitive natural language understanding (NLU) and intent routing MUST be performed entirely offline. You are strictly required to use the local Qwen 2.5 Coder 7B model via the `11434` bridge. Cloud LLMs are explicitly forbidden for intent routing to preserve maximum privacy.
4.  **Exact Decimal Math:** All financial calculations must use exact decimal representations. Floating-point floats (`f32`, `f64`) are forbidden in the balance engine.

## Code Quality

### Linting & Formatting

All code must pass the platform base-state command before merge. For a focused Rust edit, the minimum local checks are:

```bash
cargo fmt --all -- --check
cargo clippy --locked --lib --bins -- -D warnings
```

### Mutation Testing

To ensure high-quality code and robust logic, we highly recommend using mutation testing locally before submitting a PR:

```bash
cargo install cargo-mutants
cargo mutants
```

This verifies that the test suite actually catches bugs in the business logic (especially in `src/engine/balance.rs`, `src/engine/verification.rs`, and `src/engine/offline_parser.rs`).

## Architecture Guidelines

- **Fallback chains:** Every new cloud integration must have an offline fallback. Never leave a pipeline stage with a single point of failure.
- **API availability:** New API keys must be added to `ApiAvailability` in `src/app/config.rs` and checked at boot time.
- **Backend preferences:** New backends should be added to the appropriate enum (`AiProviderMode`, `DocumentParserMode`, `VerificationMode`, `PdfEngineMode`) and surfaced in the Backend Preferences UI in `src/app/modals.rs`.
- **Error handling:** Prefer typed errors with context. No silent failures or unchecked unwraps in production paths.
- **Secrets:** Never log, print, or commit API key values. Use `.env.example` for templates.

## Documentation Parity

Before merging, verify documentation matches code:

- [ ] Version strings in `README.md`, `docs/TECH_STACK.md`, `AGENTS.md`, `CHANGELOG.md`, and `Cargo.toml` are consistent.
- [ ] Default backend/parser mentions match the `#[default]` attributes in `src/app/config.rs`.
- [ ] Dependency version numbers in `docs/TECH_STACK.md` match `Cargo.toml`.
- [ ] Engine descriptions match the actual implementations in `src/pdf/`.
- [ ] OCR / Typst / Feature-gated capabilities documented with correct prerequisites.
- [ ] Comments in `src/pdf/selector.rs` accurately describe engine priority and fallback behavior.

## Bug Reports

If you discover a bug, please run the diagnostic tool and include its output in your report:

```bash
./target/release/dual-core-pdf-pipeline doctor
```
