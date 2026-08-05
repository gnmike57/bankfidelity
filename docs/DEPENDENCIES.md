# Dependency Declaration

The **Bank Statement Fidelity Editor** relies on a specific set of system libraries, Rust crates, and Python packages. This document serves as the canonical list of dependencies required to build and run the application.

## System Dependencies

These must be installed via your operating system's package manager before building the application.

| Dependency | Purpose | Ubuntu/Debian Package | macOS (Homebrew) |
| :--- | :--- | :--- | :--- |
| **C/C++ Compiler** | Required by `cc` crate to build C dependencies (e.g., ring, openssl). | `gcc`, `g++`, `build-essential` | Xcode Command Line Tools |
| **OpenSSL** | Cryptography and secure network requests. | `libssl-dev`, `pkg-config` | `openssl` |
| **MuPDF** | Underlying C library for PyMuPDF rendering. | `libmupdf-dev` | `mupdf` |
| **Tesseract & Leptonica** | Optical Character Recognition (OCR) fallback. | `tesseract-ocr`, `libleptonica-dev` | `tesseract`, `leptonica` |
| **Python 3.10+** | Runtime for the PyO3 bridge. | `python3`, `python3-pip`, `python3-dev` | `python@3.10` |

## Rust Dependencies (`Cargo.toml`)

The core application is written in Rust (edition 2021). Key dependencies include:

*   **`egui` & `eframe`:** Immediate-mode GUI framework.
*   **`tokio`:** Asynchronous runtime for API calls and background jobs.
*   **`pyo3`:** Bindings for the Python interpreter.
*   **`reqwest`:** HTTP client for AI provider APIs.
*   **`serde` & `serde_json`:** Serialization and deserialization of JSON manifests and API payloads.
*   **`ring` & `chacha20poly1305`:** Cryptographic hashing (SHA-256) and software root-of-trust encryption.

*For exact versions, refer to the `Cargo.lock` file.*

## Python Dependencies (`requirements.txt`)

These packages are executed by the embedded Python interpreter via PyO3. They must be installed in the environment where the Rust binary is run.

| Package | Purpose |
| :--- | :--- |
| **`pymupdf`** | Core PDF parsing, rendering, and in-place stream editing. |
| **`pymupdfpro`** | Enhanced font metrics and per-segment editing capabilities. |
| **`fonttools`** | Deep font analysis and replication. |
| **`pillow`** | Image processing for visual diffing and rendering. |
| **`pdf2image`** | Converting PDF pages to images for the visual fidelity gates. |
| **`requests`** | Fallback HTTP client for Python-side API scripts. |
| **`google-generativeai`** | Python SDK for Gemini (used in specific test scripts). |
| **`mistralai`** | Python SDK for Mistral (used in specific test scripts). |

To install all Python dependencies:
```bash
pip install pymupdf pymupdfpro fonttools pillow pdf2image requests google-generativeai mistralai
```
