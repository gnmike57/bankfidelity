# Bank Statement Fidelity Editor

**Version:** 0.5.0  
**Status:** Beta  
**License:** Proprietary (Dual-Core PASSPHRASE required)

A professional Rust/egui desktop application designed for **pixel-perfect, evidence-verified in-place editing of Australian bank statement PDFs**. 

Unlike standard PDF editors that reconstruct or flatten documents (destroying forensic metadata and exact font metrics), this pipeline surgically modifies the underlying PDF stream bytes, leaving the rest of the document mathematically untouched. It features a comprehensive multi-provider AI backend, an exact-decimal balance engine, and a mandatory 8-gate cryptographic evidence system.

---

## 🚀 Key Features

*   **Pixel-Perfect Fidelity:** Surgically edits PDF text streams in-place. Lossy reconstruction, flattening, and automatic font substitution are explicitly disabled.
*   **Multi-Provider AI Extraction:** Integrates with Mindee (default), LlamaParse, Google Document AI, Gemini, pdfRest, and Mistral.
*   **Exact-Decimal Balance Engine:** Automatically recalculates running and closing balances when a transaction is edited, guaranteeing mathematical consistency across the entire ledger.
*   **Cryptographic Evidence Ledger:** Every edit produces an immutable JSON manifest containing policy versions, calibration hashes, exact intent sets, structural gates, and visual fidelity scores.
*   **Offline Fallback:** Works entirely offline with a deterministic geometry extractor if API keys are missing or cloud services fail.
*   **Batch Processing:** Concurrent extraction and auto-balancing across entire directories of PDFs.
*   **100% CLI Parity:** Every GUI feature is fully available via the command line for headless automation.

---

## 🛠️ Architecture

The application is built on a high-performance Rust core with a supervised Python bridge for specific AI/PDF libraries.

| Component | Description |
| :--- | :--- |
| **`app/`** | CLI, egui GUI, runtime loop, audit logging, telemetry, and configuration. |
| **`engine/`** | Exact-decimal balance math, transaction modeling, multi-layer verification, layout, and font analysis. |
| **`pdf/`** | Engine trait + selector (PyMuPDF primary, Pdfium fallback, OxidizePdf). |
| **`extractors/`** | Deterministic geometry providers (per-bank templates) and hybrid mergers. |
| **`ai/`** | Document AI, Gemini, LlamaParse, pdfRest, Vision AI, and the supervised Python bridge. |
| **`security/`** | Software root-of-trust and ChaCha20-Poly1305 encryption. |

---

## 📦 Getting Started

For full installation, dependency requirements, and API key setup, please see the [QUICKSTART.md](QUICKSTART.md) guide.

### Basic Build
```bash
cargo build --release
./target/release/dual-core-pdf-pipeline gui
```

---

## ⚠️ What It Does NOT Do

This tool is designed for **evidence-verified in-place edits** of known bank statement formats. It explicitly does **not**:
*   Generate fake transactions from scratch.
*   Forge cryptographic signatures or bypass digital certificates.
*   Claim universal visual or forensic identity with every commercial PDF producer.
*   Ship with or support local LLM execution (explicitly disabled in v1 for security and determinism).

---

## 📚 Documentation

*   [QUICKSTART & Build Guide](QUICKSTART.md)
*   [Development & Testing Guide](docs/DEVELOPMENT.md)
*   [Architecture Details](docs/TECH_STACK.md)
*   [Evidence & Audit Policy](docs/remediation/EVIDENCE_POLICY.md)
