# Architecture Overview

The **Bank Statement Fidelity Editor** is built on a high-performance Rust core that handles cryptographic evidence, ledger mathematics, and exact-geometry layout calculations, paired with a supervised Python bridge for interfacing with specialized PDF and AI libraries.

## High-Level Data Flow

1. **Extraction (Read):** A PDF is loaded. The pipeline attempts to extract geometry and text using cloud AI parsers (Mindee, LlamaParse). If these fail or are unavailable, it falls back to a local, deterministic PyMuPDF geometry extractor.
2. **Analysis (Math):** The exact-decimal ledger engine recalculates every running balance. If an edit introduces an imbalance, the Smart Balance Engine (optionally powered by Gemini) proposes a cascading adjustment plan.
3. **Modification (Write):** Edits are targeted by their exact bounding box and original text identity. The `pdf/` engine modifies the underlying PDF stream bytes in-place, preserving original font dictionaries, kerning, and color.
4. **Verification (Audit):** The output PDF is re-parsed locally. Mandatory gates verify structural integrity, edit membership, and mathematical consistency. A cryptographic JSON manifest is generated and appended to the document.

## Directory Structure

| Directory | Purpose | Key Files / Modules |
| :--- | :--- | :--- |
| **`src/app/`** | Application entry points, configuration, and runtime loop. | `cli.rs`, `gui.rs`, `runtime.rs`, `config.rs` |
| **`src/engine/`** | Core business logic: balance math, verification gates, and layout. | `balance.rs`, `verification.rs`, `font_metrics.rs` |
| **`src/pdf/`** | PDF engine abstractions and the OxidizePdf fallback. | `mod.rs`, `oxidize.rs` |
| **`src/ai/`** | Cloud provider clients and the supervised Python bridge. | `gemini_client.rs`, `python_worker.rs`, `pdfrest.rs` |
| **`src/extractors/`** | Deterministic bank-specific templates and geometry heuristics. | `mod.rs`, `templates.rs` |
| **`python/`** | Python scripts executed by the PyO3 bridge (PyMuPDF, FontTools). | `worker.py`, `font_replicator.py` |
| **`docs/`** | Architecture, remediation plans, and evidence policy documentation. | `EVIDENCE_POLICY.md`, `TECH_STACK.md` |
| **`tests/`** | Integration, ranking, and end-to-end transfer stress tests. | `ranking_test.rs`, `au_transfer_stress.rs` |

## The Python Bridge (`PyO3`)

Because certain PDF manipulation libraries (like PyMuPDF and FontTools) lack mature, feature-complete Rust equivalents, the application uses `PyO3` to execute Python code.

To prevent Python's Global Interpreter Lock (GIL) from blocking the Rust asynchronous runtime or GUI thread, all Python calls are funneled through a single, dedicated actor thread (`src/ai/python_worker.rs`). Panics inside the Python actor are caught, serialized, and surfaced as structured Rust `Result::Err` values, ensuring the application never crashes due to a Python exception.

## Cryptographic Evidence

Every successful edit produces an evidence manifest. This is not a simple log file; it is a cryptographic proof of the edit's locality and intent.

The manifest includes:
*   SHA-256 hashes of the original and edited PDFs.
*   The exact `[x0, y0, x1, y1]` bounding box of the modified text.
*   The policy version and calibration hash used for verification.
*   The explicit pass/fail status of all 8 mandatory structural gates.

This manifest is embedded directly into the output PDF as a new page, ensuring the document carries its own audit trail.
