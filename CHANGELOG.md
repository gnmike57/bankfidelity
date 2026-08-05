# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- **Automated Stress Testing Harness:** Added a comprehensive stress testing benchmark suite (`au_transfer_stress.rs`) and result evaluation framework (`tests/stress_results/stress_test_evaluation.md`) for document processing backends. Evaluates PyMuPDF, Document AI, Mindee, LlamaParse, pdfRest, Applitools, and various LLMs for correctness, fidelity, and latency across 7 axes.

## [v1.0.0] - 2026-07-05

### Added
- **Automated Bug Reporting & Beta Repair Loop:** Integrated interactive fallback UI and webhook telemetry for rapid crash capture and log aggregation during the beta testing phase (`docs/BETA_TESTING.md`).
- **Multi-Backend Pipeline Architecture:** Every pipeline stage now supports configurable primary backends with automatic fallback chains. If a cloud API fails or its key is missing, the pipeline gracefully degrades to the next-best offline option.
- **Boot-Time API Availability Detection:** On startup, the app probes all configured API keys (Gemini, Document AI, Mindee, LlamaParse, pdfRest, Applitools, PyMuPDF Pro) and stores availability status in `ApiAvailability`. Logged to console on boot.
- **Backend Preferences UI:** New settings panel (`Settings → Backend Preferences`) with:
  - Selectable backends for AI Provider, Document Parser, PDF Engine, and Verification Renderer.
  - Unavailable backends marked with ⛔ and explanatory hover text with signup URLs.
  - Unified warning banner when critical keys are missing.
  - Visual diff threshold slider (0.005–0.10, default 0.02).
  - Max visual retry attempts slider (1–10, default 5).
- **Mindee Financial Document Parser:** Added as the default document parser (`DocumentParserMode::MindeeFinDoc`). Requires `MINDEE_API_KEY`.
- **LlamaParse Integration:** Added as an alternative LLM-based document parser. Requires `LLAMAPARSE_API_KEY`.
- **Applitools Eyes Integration:** Visual AI testing layer in the verification pipeline. Auto-runs when `APPLITOOLS_API_KEY` is set; gracefully skips otherwise. Node.js bridge at `src/ai/applitools_bridge.js`.
- **pdfRest Cloud Rendering:** Adobe-tier PDF rendering for verification. Falls back to local Pdfium when `PDFREST_API_KEY` is missing.
- **Typst Reconstruction Engine:** Ultimate fail-safe edit engine that rebuilds PDFs from scratch using Typst and font subsetting. Kicks in automatically when both PyMuPDF and Pdfium edit paths fail.
- **Chaos Fallback Tests:** Integration test (`tests/chaos_fallback.rs`) verifying the engine selector gracefully handles complete engine failures.
- **Transfer Transaction Engine:** Copy transactions between PDFs with font analysis and replication (`Job::TransferTransactions`).
- **Date Period Adjustment:** Shift statement date ranges forward/backward (`Job::AdjustDatePeriods`).
- **Document AI Admin:** Train, deploy, undeploy, and manage Document AI processor versions from within the app.

### Changed
- **Configuration Overhaul:** `AppConfig` now holds `AiProviderMode`, `DocumentParserMode`, `VerificationMode`, and `PdfEngineMode` enums, all persisted via `confy` and hot-reloadable via `Job::ReloadConfig`.
- **Parser Fallback Logic:** All cloud parsers (Mindee, LlamaParse, Document AI) now auto-fallback to `offline_parser::parse_statement_offline()` when unconfigured or on API error.
- **Balance Engine Fallback:** When AI is set to `ManualOnly`, balance analysis uses the local `balance::process_and_reconcile()` engine instead of failing.
- **Verification Pipeline:** Refactored to a multi-layer system: Local SSIM + Tile-max + Perceptual Hash (always) → pdfRest cloud rendering (optional) → Applitools Eyes (optional) → Gemini Vision AI (optional).
- **README:** Complete rewrite with backend preference tables, fallback chains, and architecture diagrams.
- **QUICKSTART:** Updated with all new API keys, backend preferences guide, and expanded CLI examples.
- **.env.example:** Now includes all 11 configurable API keys with signup URLs and descriptions.
- **Dependency Updates:** Full `cargo update` bringing all dependencies to latest compatible versions.

### Fixed
- **Clippy Lints:** Fixed `unnecessary-cast` in `typst_engine.rs`, `items-after-test-module` in `offline_parser.rs`, and `single-component-path-imports` in `tests/chaos_fallback.rs`.
- **OCR Function Ordering:** Moved `extract_text_via_ocr` before test module to comply with Rust 2024 lint rules.
- **Cargo Temp Cleanup:** Cleared orphaned cargo build artifacts and intermediary files.

## [v0.5.0] - 2026-06-07

### Changed
- Bumped version to v0.5.0 and updated documentation.

## [v0.4.0] - 2026-05-26

### Added
- **Smart Balance Engine**: Added the cloud-required Smart Balance Engine pipeline (Document AI -> Hybrid Merging -> Balance Math -> Gemini Proposals).
- **Hybrid Extraction Layer**: Implemented three geometry providers (`Tesseract`, `PyMuPDF Heuristics`, `Bank Templates`) and a `HybridMerger` to map semantic transactions to spatial bounding boxes.
- **PdfEngine Trait Abstraction**: Abstracted all PDF manipulation behind a `PdfEngine` trait with a primary `MuPdfEngine` and fallback `PyMuPdfEngine`.
- **Document AI JWT**: Real RS256 JWT generation and token exchange for Google Cloud Document AI.
- **Structured Telemetry**: Replaced print debugging with process-wide `tracing` macros, daily file rotation, and an optional OpenTelemetry (OTLP) gRPC exporter.
- **CLI Parity**: Full feature parity for the CLI, including new `balance --auto-approve` and `extract` subcommands with typed JSON outputs.
- **Verification Math**: Hardened verification tests to compute mathematical drift correctly against the original unmodified PDF's expected balance.
- **5 Bank Templates**: Shipped default templates for Chase, Bank of America, Wells Fargo, Citibank, and Capital One.

### Changed
- **Unified Engine**: The `SmartDocumentEngine` now serves as the central unified engine for all operations, treating multi-page statements as one connected document.
- **History Simplification**: Consolidated the `ChangeHistory` into a single process-wide source of truth orchestrated by the Tokio runtime. Fixed the auto-undo GUI bug.
- **PyO3 Architecture**: The PyO3 bridge now relies on a non-panicking, dedicated actor thread to isolate Python interpreter state.
- **Gemini Rest Client**: Updated to the stable `v1beta` Google Gemini REST API using exact `camelCase` fields and typed object schemas for `responseSchema`.
- **Refactoring**: Moved from a three-layer architecture to a mature five-layer architecture (`app/`, `engine/`, `pdf/`, `extractors/`, `ai/`, `security/`).

### Removed
- **Legacy Stubs**: Removed placeholder parsing and fake manual heuristics in Python integration and text_editor components.

<!-- link references -->
[v1.0.0]: https://github.com/maryjpww-star/bank-statement-fidelity-editor/compare/v0.5.0...v1.0.0
[v0.5.0]: https://github.com/maryjpww-star/bank-statement-fidelity-editor/compare/v0.4.0...v0.5.0
[v0.4.0]: https://github.com/maryjpww-star/bank-statement-fidelity-editor/releases/tag/v0.4.0
