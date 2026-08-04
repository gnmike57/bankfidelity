# Quickstart & Build Guide

This guide covers everything needed to clone, configure, build, and run the **Bank Statement Fidelity Editor** on your first attempt.

## 1. System Dependencies

The application relies on a Rust core, a Python bridge for PDF rendering, and system-level C libraries for linking.

### Windows
1. Install **Visual Studio 2019 or 2022 Build Tools** (ensure the "Desktop development with C++" workload is selected).
2. Install **Python 3.10+** (ensure it is added to your PATH).
3. Install **Rust** via [rustup](https://rustup.rs/).

### macOS
```bash
brew install mupdf tesseract leptonica
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Linux (Ubuntu / Debian)
```bash
sudo apt-get update
sudo apt-get install -y gcc g++ make pkg-config libssl-dev libmupdf-dev tesseract-ocr libleptonica-dev python3 python3-pip
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## 2. Python Environment

The application uses `PyO3` to call Python libraries directly from Rust. You must install the required Python packages globally or in an activated virtual environment before building.

```bash
pip3 install pymupdf pymupdfpro fonttools pillow requests google-generativeai mistralai pdf2image
```

---

## 3. Clone and Configure

Clone the repository:
```bash
git clone https://github.com/gnmike57/bankfidelity.git
cd bankfidelity
```

Create your environment configuration:
```bash
cp .env.example .env
```

### Required Configuration
You **must** set the following in your `.env` file for the application to start:

| Variable | Description |
| :--- | :--- |
| `DUAL_CORE_PASSPHRASE` | A strong passphrase (≥16 chars) to unlock the software root-of-trust. |

### Recommended AI Providers
The application works offline, but cloud parsers drastically improve extraction accuracy. Add these to your `.env` file if available:

| Variable | Purpose |
| :--- | :--- |
| `PYMUPDF_PRO_KEY` | Enables exact font metrics and per-segment editing. |
| `GEMINI_API_KEY` | Enables Smart Balance and AI Completeness checks. |
| `MINDEE_API_KEY` | Enables the default high-accuracy document parser. |
| `LLAMAPARSE_API_KEY` | Enables the LLM-based fallback parser. |
| `PDFREST_API_KEY` | Enables Adobe-tier cloud rendering for verification. |

---

## 4. Build and Run

With dependencies installed and the `.env` file configured, build the project:

```bash
cargo build --release
```

Launch the graphical user interface:

```bash
./target/release/dual-core-pdf-pipeline gui
```

*(On Windows, the binary is `target/release/dual-core-pdf-pipeline.exe`)*

---

## 5. First-Run Walkthrough

1. **Check Backend Status:** Open **Settings → Backend Preferences**. Verify that your configured API keys are recognized (marked with ✅).
2. **Load a Statement:** Enter the path to a PDF in the left panel and click **Load Entire Statement**.
3. **Edit a Value:** Click any text block on the canvas. The right panel will populate. Type a new value and click **🎯 Apply Change**.
4. **Verify Math:** Click **⚖️ Balance Statement** to run the exact-decimal ledger analysis.
5. **Render:** Click **Confirm and Render** to generate the final, pixel-perfect output PDF with its cryptographic evidence manifest.
