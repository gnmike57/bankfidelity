# Technology Stack Breakdown

This document provides a detailed overview of the specific technologies, frameworks, and libraries used to build the Bank Statement Fidelity Editor (`dual-core-pdf-pipeline` v1.0.0).

## Core Languages

- **Rust (Edition 2021):** The primary language for the application logic, GUI, and high-performance asynchronous orchestration. Rust guarantees memory safety, fast execution, and strict concurrency control.
- **Python (3.10+):** Used selectively for PyMuPDF/PyMuPDF Pro integrations that do not have native Rust equivalents. It runs as a supervised JSON-lines worker process with a strict protocol, bounded lifecycle, and isolated protocol output.

## GUI Framework (Native Desktop)

- **`egui` (0.30):** A highly performant, immediate-mode GUI framework built purely in Rust. Chosen for its simplicity, speed, and seamless integration with Rust data structures, making it perfect for rapid data-driven UI development.
- **`eframe` (0.30):** The official framework wrapper around `egui` to run it as a native desktop application (handling the OS windowing, WebGL/WGPU rendering contexts).
- **`egui_extras`:** Extended widgets including `TableBuilder` for the editable transaction table, and image loading for rendered PDF pages.

## PDF Processing Engines

The application employs a multi-engine strategy with automatic fallback, managed by `PdfEngineSelector`:

- **`PyMuPDF` / `pymupdfpro` (via `pyo3`):** The primary engine. Called from Rust via Python bindings. Capable of high-fidelity, per-segment redaction and text insertion while accurately reusing the exact embedded font dictionaries and glyph metrics. The Pro tier adds enhanced font handling (gated by `PYMUPDF_PRO_KEY`).
- **`pdfium-render` (0.8.x):** Rust bindings to Google's open-source `pdfium` C++ library (the same engine used in Google Chrome). Used as the fallback engine for rendering, text extraction, and editing when PyMuPDF is unavailable.
- **`lopdf` (0.34):** A pure-Rust library for low-level PDF dictionary manipulation. Used for the Split & Merge engine, extracting pages and merging them back together without altering visual fidelity or dropping fonts.
- **Legacy Typst reconstruction:** Persisted configuration values remain readable for compatibility, but lossy reconstruction is not selectable and is rejected in fidelity workflows.

### Engine Mode Hierarchy

```
PdfEngineSelector:
  Auto (default) → PyMuPDF primary, Pdfium fallback
  DualConcurrent  → Both engines in parallel, prefer PyMuPDF
  NativeOnly      → Pdfium only
  PyMuPdfOnly     → PyMuPDF only
  TypstReconstruct → legacy value; explicit unsupported disposition
```

## Document Parsers (Multi-Backend)

Parser routing is explicit: only the selected cloud parser may run, followed by the qualified offline parser. Unrelated providers are never inserted into the fallback chain.

- **Google Cloud Document AI:** Optional selected parser with trained-layout support and explicit provider evidence.
- **LlamaParse (default cloud choice):** Optional selected LLM-based parser via LlamaCloud.
- **Offline Parser (`offline_parser.rs`):** Deterministic local text-layer and geometry parser with stable row identity, exact-decimal continuity checks, and review flags.
- **Local OCR (`ocrs` + `rten`):** Legacy/deferred PDF mode. Configuration values remain readable, but the mode is not selectable in v1.

## AI and Machine Learning Integration

- **Google Gemini / Vertex AI:** Multi-purpose AI engine used for:
  - Smart Balance Engine — generates minimal cascading adjustment plans to resolve math errors.
  - Completeness Validation — checks for missed transaction rows.
  - Vision Validation — compares rendered PDF pages for visual fidelity.
  - Auth modes: API Key (simple) or Vertex AI (enterprise, SA/ADC).
  - Provider pass, rejection, malformed response, and unavailability are represented explicitly; optional AI never overrides mandatory local gates.
- **pdfRest API:** Optional additive cloud rendering. Requests are bounded and contract-tested; authorization is scoped to the API upload, downloaded evidence must be PNG, and provider failure cannot weaken local acceptance.

## Verification Pipeline (Multi-Layer)

The verification system separates mandatory local gates from optional provider evidence:

1. **Structural gates:** Page count/order, MediaBox, CropBox, rotation, content presence, font resources, and metadata policy.
2. **Exact content/editability gates:** Old text must exist exactly once at the source target, new live text exactly once at the edited target, and stale or duplicate membership fails.
3. **Visual gates:** Every page is rendered at 300 DPI; tile-max, outside-region SSIM, and intended-region residuals use immutable thresholds.
4. **Financial gates:** The independently reparsed ledger must preserve row count, sequence, dates, descriptions, signs, values, every running balance, and closing balance.
5. **Evidence gate:** JSON report, replay configuration, input hashes, rendered-artifact hashes, and typed dispositions are atomically persisted and read back.
6. **Optional providers:** pdfRest, Vision AI, and Document AI outcomes remain additive `passed`, `failed`, or `unavailable` evidence.

The immutable policy is versioned in `assets/verification-calibration-v2.json`; deterministic verification never widens masks or retries unchanged output under looser criteria.

## Geometry Extraction (Hybrid)

- **Bank Templates (YAML):** Per-bank column layouts for deterministic text extraction. Ships with templates for AU and US banks.
- **PyMuPDF Heuristic:** Statistical column boundary detection from embedded text runs.
- **Hybrid Merger:** Merges results from multiple geometry providers with deterministic tiebreak rules.

## Concurrency and Asynchronous Runtime

- **`tokio` (1.x):** The industry-standard async runtime for Rust. Handles all network requests, file I/O, job dispatching, and timeout management.
- **MPSC Channels:** Multi-producer, single-consumer channels bridge the synchronous, immediate-mode GUI (`egui`) with the asynchronous background tasks (`tokio`).
- **API Semaphore:** Limits concurrent cloud API calls (default 3) to prevent rate limiting.
- **Cancellation Registry:** Per-job cancellation tokens for responsive UI cancellation.

## Python Interoperability (FFI)

- **Supervised Python worker:** Rust launches a pinned Python runtime through a strict JSON-lines protocol. Native/third-party stdout is quarantined from the protocol stream, request/response schemas reject unknown fields, lifecycle results are exactly-once, and worker termination is bounded.

## Observability and Logging

- **`tracing` / `tracing-subscriber`:** Structured, event-driven logging framework for Rust with daily file rotation.
- **`opentelemetry` (0.27):** OpenTelemetry SDK for distributed tracing, allowing the application to export metrics and traces to an OTLP-compatible endpoint for deep debugging.
- **Capability Summary:** Local configuration is reported without unrelated startup network calls; explicit credential checks can be run on demand.

## Serialization and State Management

- **`serde` (1.0) / `serde_json`:** Used ubiquitously for serializing and deserializing API requests, JSON outputs, and internal message passing.
- **`confy` (0.6):** Configuration management for user preferences and backend choices. Verification thresholds shown in the UI are read-only policy values, not user-tunable acceptance criteria.
- **`dotenvy`:** Loads `.env` file on startup. Hot-reloadable via `Job::ReloadConfig`.

## Security and Fault Tolerance

- **`chacha20poly1305`:** Strong encryption of the local Document AI cache and other sensitive artifacts at rest.
- **Enterprise Fault Tolerance:** Exponential backoffs, automatic retry middleware via `reqwest-retry`, strict cryptographic software root-of-trust (via SHA-256 and `.pipeline_key`).
- **Explicit provider availability:** Missing, malformed, rejected, and timed-out provider outcomes remain typed and visible; provider availability never changes mandatory local verification criteria.

## Font Engineering

- **`ttf-parser`:** Fast, zero-allocation TrueType/OpenType font parsing for font analysis and metric extraction.
- **Font coverage analysis:** Embedded and supplied fonts are inspected for required glyph coverage. Unsupported automatic synthesis, donor substitution, and metric adaptation are quarantined and fail before publication.
