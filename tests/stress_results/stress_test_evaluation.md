# Stress Test Evaluation Matrix — Bank Statement Fidelity Editor v0.5.1

**Executed:** 2026-07-23 23:57:14 UTC
**Platform:** Python 3.14.5, PyMuPDF 1.28.0

## Results Matrix

| Function / Task | Target Example PDF | Tested Dependencies | Score (Correctness/Fidelity) → Avg | Actionable Fallback Hierarchy |
|---|---|---|---|---|
| Test 1: Extraction | Standard_Bank_Statement_01.pdf | Offline Heuristic, PyMuPDF Built-in, LlamaParse, Document AI, Mindee API | Offline Heuristic (88/98) → 93, PyMuPDF Built-in (0/95) → 48, LlamaParse (0/0) → 0, Document AI (0/0) → 0, Mindee API (0/0) → 0 | 1. Offline Heuristic (Avg 93)<br>2. PyMuPDF Built-in (Avg 48) |
| Test 2: Fidelity Edit | Corrupted_Font_Ledger.pdf | pymupdfpro, Pdfium, Typst Reconstruct | pymupdfpro (90/85) → 88, Pdfium (60/92) → 76, Typst Reconstruct (75/65) → 70 | 1. pymupdfpro (Avg 88)<br>2. Pdfium (Avg 76)<br>3. Typst Reconstruct (Avg 70) |
| Test 3: Math Balance | Unbalanced_Ledger_Test.pdf | Local Math Engine, OpenRouter AI, Groq AI, Document AI, Gemini AI | Local Math Engine (100/100) → 100, OpenRouter AI (0/0) → 0, Groq AI (0/0) → 0, Document AI (0/0) → 0, Gemini AI (0/0) → 0 | 1. Local Math Engine (Avg 100) |
| Test 4: Visual QA | Subtle_Shift_Artifact.pdf | SSIM + Tile-Max + pHash, Applitools Eyes, Gemini Vision, pdfRest Cloud | SSIM + Tile-Max + pHash (100/100) → 100, Applitools Eyes (0/0) → 0, Gemini Vision (0/0) → 0, pdfRest Cloud (0/0) → 0 | 1. SSIM + Tile-Max + pHash (Avg 100) |
| Test 5: Transfer Transactions | ? | Groq (Llama 3), OpenRouter, Gemini 1.5 Flash | Groq (Llama 3) (0/0) → 0, OpenRouter (0/0) → 0, Gemini 1.5 Flash (0/0) → 0 | No viable backends |
| Test 6: E2E GUI Testing | ? | Rust UIAutomation | Rust UIAutomation (100/100) → 100 | 1. Rust UIAutomation (Avg 100) |
| Test 7: PII Anonymization | ? | Groq PII, Gemini 1.5 PII | Groq PII (0/0) → 0, Gemini 1.5 PII (0/0) → 0 | No viable backends |
| Test 8: Forensic Evasion | ? | PyMuPDF Pro, Typst Reconstruct, Pdfium | PyMuPDF Pro (100/100) → 100, Typst Reconstruct (80/100) → 90, Pdfium (0/0) → 0 | 1. PyMuPDF Pro (Avg 100)<br>2. Typst Reconstruct (Avg 90) |

## Detailed Results Per Test

### Test 1: Extraction

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Offline Heuristic** | 88 | 98 | **93.0** | 18ms | Found 25/30 txns, closing=Y |
| 2 | **PyMuPDF Built-in** | 0 | 95 | **47.5** | 21ms | Found 0/30 txns, 0 decimal errors |
| 3 | **LlamaParse** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 4 | **Document AI** | 0 | 0 | **0.0** | 0ms | Not configured |
| 5 | **Mindee API** | 0 | 0 | **0.0** | 0ms | API key not configured |

### Test 2: Fidelity Edit

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **pymupdfpro** | 90 | 85 | **87.5** | 126ms | Found 18 '7' glyphs, 18 successful edits, restored=Y |
| 2 | **Pdfium** | 60 | 92 | **76.0** | 135ms | Rendered 2480x3509px @ 300DPI, content=Y |
| 3 | **Typst Reconstruct** | 75 | 65 | **70.0** | 131ms | 26/26 spans extractable, 1 fonts: {'Helvetica'} |

### Test 3: Math Balance

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Local Math Engine** | 100 | 100 | **100.0** | 3ms | Detected discrepancy=$45.0, opening=$5,000.00, closing=$10,850.21 |
| 2 | **OpenRouter AI** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 3 | **Groq AI** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 4 | **Document AI** | 0 | 0 | **0.0** | 0ms | Not configured |
| 5 | **Gemini AI** | 0 | 0 | **0.0** | 62ms | HTTP 400: {
  "error": {
    "code": 400,
    "message": "API key not valid. Please pass a valid API key.",
    "status": "INVALID_ARGUMENT",
    "details": [
      {
        "@type": "type.googleapis.com/googl |

### Test 4: Visual QA

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **SSIM + Tile-Max + pHash** | 100 | 100 | **100.0** | 2772ms | SSIM≈0.997970, max_tile=0.583347, diff_px=72852, region_detect=Y |
| 2 | **Applitools Eyes** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 3 | **Gemini Vision** | 0 | 0 | **0.0** | 474ms | HTTP 400 |
| 4 | **pdfRest Cloud** | 0 | 0 | **0.0** | 1194ms | HTTP 401: {"error":"The provided key is not valid."} |

### Test 5: Transfer Transactions

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Groq (Llama 3)** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 2 | **OpenRouter** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 3 | **Gemini 1.5 Flash** | 0 | 0 | **0.0** | 19ms | API Error No response |

### Test 6: E2E GUI Testing

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Rust UIAutomation** | 100 | 100 | **100.0** | 8657ms | GUI launched and tree attached correctly |

### Test 7: PII Anonymization

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Groq PII** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 2 | **Gemini 1.5 PII** | 0 | 0 | **0.0** | 42ms | API Error No response |

### Test 8: Forensic Evasion

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **PyMuPDF Pro** | 100 | 100 | **100.0** | 8ms | Producer: '', Creator: '', EOF markers: 1 |
| 2 | **Typst Reconstruct** | 80 | 100 | **90.0** | 0ms | Clean rebuild, single %%EOF, but likely leaves Typst metadata tag |
| 3 | **Pdfium** | 0 | 0 | **0.0** | 0ms | Output is an image raster, not a vector PDF. Fails structural forensics completely. |

## Production Fallback Routing Logic

Based on the empirical scores above, the recommended fallback chains are:

### Test 1: Extraction
```
  PRIMARY: Offline Heuristic         (Avg=93, C=88, F=98)
  FALLBACK-1: PyMuPDF Built-in          (Avg=48, C=0, F=95)
```

### Test 2: Fidelity Edit
```
  PRIMARY: pymupdfpro                (Avg=88, C=90, F=85)
  FALLBACK-1: Pdfium                    (Avg=76, C=60, F=92)
  FALLBACK-2: Typst Reconstruct         (Avg=70, C=75, F=65)
```

### Test 3: Math Balance
```
  PRIMARY: Local Math Engine         (Avg=100, C=100, F=100)
```

### Test 4: Visual QA
```
  PRIMARY: SSIM + Tile-Max + pHash   (Avg=100, C=100, F=100)
```

### Test 5: Transfer Transactions
```
  NO VIABLE BACKENDS — all scored 0
```

### Test 6: E2E GUI Testing
```
  PRIMARY: Rust UIAutomation         (Avg=100, C=100, F=100)
```

### Test 7: PII Anonymization
```
  NO VIABLE BACKENDS — all scored 0
```

### Test 8: Forensic Evasion
```
  PRIMARY: PyMuPDF Pro               (Avg=100, C=100, F=100)
  FALLBACK-1: Typst Reconstruct         (Avg=90, C=80, F=100)
```
