# BankFidelity — Remaining Work Tracker

This document tracks remaining steps for packaging and release of the current
crate version (`2.0.0` in `Cargo.toml`). Target packaging tag is TBD.

## 1. GUI `__DISPATCH:` Signal Routing — DONE

**Status:** Implemented in `src/app/gui.rs` (`JobResult::Error` arm).

- Messages starting with `__DISPATCH:` are intercepted and re-dispatched as
  real `Job` values **before** autofix / error toast handling.
- Supported: Undo, Redo, Balance (honors `auto_apply` → `BalanceAndApplyAll`),
  Verify, Extract, Transfer (requires Transfer workflow source + target PDFs;
  otherwise warning toast), AdjustDates, Categorize, Doctor, ReloadConfig,
  StressTest.

## 2. Compile and Verify Rust Binary

**Status:** Routine pre-release gate.

```text
cargo fmt --all -- --check
cargo clippy --lib --bins -- -D warnings
cargo test --lib
cargo test --test runtime_smoke --test server_e2e --test gui_e2e_tests
```

Optional live matrix (API keys + AU PDFs):

```text
cargo test --test workflow_e2e -- --ignored --nocapture
cargo test --test au_transfer_stress -- --ignored --nocapture
```

## 3. Build macOS and Windows Release Installers

**Status:** Pending packaging.

- **macOS:** `scripts/build_mac_app.sh` / release workflow portable bundle.
- **Windows:** portable bundle via release workflow; WiX MSI optional.
  `wix.zip` / `wix_bin/` are gitignored packaging artifacts.

Both installers must bundle or auto-download Pdfium (`pdfium.dll` /
`libpdfium.dylib`).

## 4. Final GitHub Release

**Status:** Pending explicit tag choice.

- Confirm crate version in `Cargo.toml` (currently `2.0.0`).
- Tag `vX.Y.Z` and push; release workflow uploads portable bundles.
- Env readiness: `DUAL_CORE_PASSPHRASE` required; `PYMUPDF_PRO_KEY` required
  for full PyMuPDF Pro edit path (native/offline fallbacks still operate);
  Document AI keys recommended for cloud extract/balance.

## 5. Known residual / env-gated

| Item | Notes |
|------|--------|
| Live AU transfer stress | `#[ignore]`; needs full API matrix (costly) |
| Computer-use / pywinauto | Desktop session required |
| Doctor PYMUPDF_PRO_KEY | Missing key → doctor "Not ready" for Pro path |
