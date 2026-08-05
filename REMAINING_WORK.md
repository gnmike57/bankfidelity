# BankFidelity — Remaining Work Tracker

This document tracks the final remaining steps required to push the v1.4.0 release and complete the BankFidelity feature expansion.

## 1. GUI `__DISPATCH:` Signal Routing Bug (Pending)
**Issue:** The `AiCommand` handler in `runtime.rs` sends command payloads to the GUI by prefixing the error message with `__DISPATCH:`. The GUI currently handles this in the `JobResult::Error` match arm, which means it logs the command as an error, triggers the autofix pipeline, and shows a red toast notification.
**Fix Required:** 
- In `src/app/gui.rs` line ~1847, intercept `JobResult::Error` messages starting with `__DISPATCH:`
- Extract the JSON payload, parse it into the target `Job` variant (e.g., `Job::BalanceStatement`, `Job::ExtractTransactions`)
- Dispatch the parsed job back to the runtime via `self.job_tx.send(job)`
- Do **not** log it as an error or trigger autofix.

## 2. Compile and Verify Rust Binary
**Issue:** The codebase has undergone significant structural changes (NLP router, Financial NLP engine, Server extensions).
**Fix Required:**
- Run `cargo build --release`
- Run the full test suite: `cargo test`
- Run the E2E matrix: `python3 scripts/e2e_pipeline.py`

## 3. Build macOS and Windows Release Installers
**Issue:** The final step requested by the user is compiling the app into polished installers.
**Fix Required:**
- **macOS:** Use `cargo-bundle` to generate a `.app` bundle, then package it into a `.dmg` using `create-dmg` or `hdiutil`.
- **Windows:** Use `cargo-wix` or `Inno Setup` to generate an `.msi` or `.exe` installer.
- Both installers must bundle or auto-download the required dependencies (e.g., `pdfium.dll` / `libpdfium.dylib`).

## 4. Final GitHub Release (v1.4.0)
**Issue:** The new features (Financial NLP, MCP Server, auto-install scripts) need to be published.
**Fix Required:**
- Commit the `__DISPATCH` GUI fix.
- Tag `v1.4.0` and push to GitHub.
- Upload the generated macOS `.dmg` and Windows `.msi` installers as release assets.
