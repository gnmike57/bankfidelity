======================================================================
BENCHMARK HARNESS ΓÇö API AVAILABILITY CHECK
======================================================================
  Mindee               Γ£à AVAILABLE
  LlamaParse           Γ£à AVAILABLE
  Document AI          Γ£à AVAILABLE
  Gemini               Γ£à AVAILABLE
  Groq                 Γ¢ö MISSING
  OpenRouter           Γ¢ö MISSING
  pdfRest              Γ£à AVAILABLE
  Applitools           Γ£à AVAILABLE
  PyMuPDF Pro          Γ£à AVAILABLE
======================================================================

======================================================================
BANK STATEMENT FIDELITY EDITOR v1.0.0 ΓÇö STRESS TEST BENCHMARK
======================================================================

======================================================================
EXECUTING: Test 1: Extraction
======================================================================
  Γ£à Offline Heuristic         | C= 88 F= 98 Avg=93.0 | 94ms | Found 25/30 txns, closing=Y
  ΓÜá∩╕Å PyMuPDF Built-in          | C=  0 F= 95 Avg=47.5 | 127ms | Found 0/30 txns, 0 decimal errors
  Γ¥î Mindee API                | C=  0 F=  0 Avg=0.0 | 3020ms | HTTP 401: {"api_request": {"error": {"code": "Unauthorized", "details": "The token provided is for the v2 API. Please check the documentation here: https://docs.mindee.com/integrations/", "message": "Authorizat
  Γ¥î Document AI               | C=  0 F=  0 Avg=0.0 | 3287ms | HTTP 401: {
  "error": {
    "code": 401,
    "message": "API keys are not supported by this API. Expected OAuth2 access token or other authentication credentials that assert a principal. See https://cloud.goog
  Γ£à LlamaParse                | C=100 F= 96 Avg=98.0 | 9812ms | Found 30/30 txn lines in markdown

======================================================================
EXECUTING: Test 2: Fidelity Edit
======================================================================
  Γ£à Typst Reconstruct         | C= 75 F= 65 Avg=70.0 | 35ms | 26/26 spans extractable, 1 fonts: {'Helvetica'}
  Γ£à pymupdfpro                | C= 90 F= 85 Avg=87.5 | 119ms | Found 18 '7' glyphs, 18 successful edits, restored=Y
  Γ£à Pdfium                    | C= 60 F= 92 Avg=76.0 | 718ms | Rendered 2480x3509px @ 300DPI, content=Y

======================================================================
EXECUTING: Test 3: Math Balance
======================================================================
  Γ¥î Groq AI                   | C=  0 F=  0 Avg=0.0 | 0ms | API key not configured
  Γ¥î OpenRouter AI             | C=  0 F=  0 Avg=0.0 | 0ms | API key not configured
  Γ£à Local Math Engine         | C= 85 F=100 Avg=92.5 | 13ms | Detected discrepancy=$45.0, opening=$5,000.00, closing=$10,850.21
  Γ¥î Gemini AI                 | C=  0 F=  0 Avg=0.0 | 1333ms | HTTP 400: {
  "error": {
    "code": 400,
    "message": "API key not valid. Please pass a valid API key.",
    "status": "INVALID_ARGUMENT",
    "details": [
      {
        "@type": "type.googleapis.com/googl
  Γ¥î Document AI               | C=  0 F=  0 Avg=0.0 | 2059ms | HTTP 401

======================================================================
EXECUTING: Test 4: Visual QA
======================================================================
  Γ£à Applitools Eyes           | C= 40 F=100 Avg=70.0 | 2546ms | Visual AI diff=MISSED, output_len=111
  Γ¥î pdfRest Cloud             | C=  0 F=  0 Avg=0.0 | 3144ms | HTTP 404: { "error":"Invalid endpoint. Please refer to documentation at https://pdfrest.com/documentation/" }
  Γ¥î Gemini Vision             | C=  0 F=  0 Avg=0.0 | 3419ms | HTTP 400
  Γ£à SSIM + Tile-Max + pHash   | C=100 F=100 Avg=100.0 | 15544ms | SSIMΓëê0.997970, max_tile=0.583347, diff_px=72852, region_detect=Y

======================================================================
REPORT: tests/stress_results\stress_test_evaluation.md
RAW DATA: tests/stress_results\stress_test_raw.json
======================================================================
# Stress Test Evaluation Matrix ΓÇö Bank Statement Fidelity Editor v1.0.0

**Executed:** 2026-07-13 10:23:25 UTC
**Platform:** Python 3.14.3, PyMuPDF 1.27.2.3

## Results Matrix

| Function / Task | Target Example PDF | Tested Dependencies | Score (Correctness/Fidelity) ΓåÆ Avg | Actionable Fallback Hierarchy |
|---|---|---|---|---|
| Test 1: Extraction | Standard_Bank_Statement_01.pdf | LlamaParse, Offline Heuristic, PyMuPDF Built-in, Mindee API, Document AI | LlamaParse (100/96) ΓåÆ 98, Offline Heuristic (88/98) ΓåÆ 93, PyMuPDF Built-in (0/95) ΓåÆ 48, Mindee API (0/0) ΓåÆ 0, Document AI (0/0) ΓåÆ 0 | 1. LlamaParse (Avg 98)<br>2. Offline Heuristic (Avg 93)<br>3. PyMuPDF Built-in (Avg 48) |
| Test 2: Fidelity Edit | Corrupted_Font_Ledger.pdf | pymupdfpro, Pdfium, Typst Reconstruct | pymupdfpro (90/85) ΓåÆ 88, Pdfium (60/92) ΓåÆ 76, Typst Reconstruct (75/65) ΓåÆ 70 | 1. pymupdfpro (Avg 88)<br>2. Pdfium (Avg 76)<br>3. Typst Reconstruct (Avg 70) |
| Test 3: Math Balance | Unbalanced_Ledger_Test.pdf | Local Math Engine, Groq AI, OpenRouter AI, Gemini AI, Document AI | Local Math Engine (85/100) ΓåÆ 92, Groq AI (0/0) ΓåÆ 0, OpenRouter AI (0/0) ΓåÆ 0, Gemini AI (0/0) ΓåÆ 0, Document AI (0/0) ΓåÆ 0 | 1. Local Math Engine (Avg 92) |
| Test 4: Visual QA | Subtle_Shift_Artifact.pdf | SSIM + Tile-Max + pHash, Applitools Eyes, pdfRest Cloud, Gemini Vision | SSIM + Tile-Max + pHash (100/100) ΓåÆ 100, Applitools Eyes (40/100) ΓåÆ 70, pdfRest Cloud (0/0) ΓåÆ 0, Gemini Vision (0/0) ΓåÆ 0 | 1. SSIM + Tile-Max + pHash (Avg 100)<br>2. Applitools Eyes (Avg 70) |

## Detailed Results Per Test

### Test 1: Extraction

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **LlamaParse** | 100 | 96 | **98.0** | 9812ms | Found 30/30 txn lines in markdown |
| 2 | **Offline Heuristic** | 88 | 98 | **93.0** | 94ms | Found 25/30 txns, closing=Y |
| 3 | **PyMuPDF Built-in** | 0 | 95 | **47.5** | 127ms | Found 0/30 txns, 0 decimal errors |
| 4 | **Mindee API** | 0 | 0 | **0.0** | 3020ms | HTTP 401: {"api_request": {"error": {"code": "Unauthorized", "details": "The token provided is for the v2 API. Please check the documentation here: https://docs.mindee.com/integrations/", "message": "Authorizat |
| 5 | **Document AI** | 0 | 0 | **0.0** | 3287ms | HTTP 401: {
  "error": {
    "code": 401,
    "message": "API keys are not supported by this API. Expected OAuth2 access token or other authentication credentials that assert a principal. See https://cloud.goog |

### Test 2: Fidelity Edit

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **pymupdfpro** | 90 | 85 | **87.5** | 119ms | Found 18 '7' glyphs, 18 successful edits, restored=Y |
| 2 | **Pdfium** | 60 | 92 | **76.0** | 718ms | Rendered 2480x3509px @ 300DPI, content=Y |
| 3 | **Typst Reconstruct** | 75 | 65 | **70.0** | 35ms | 26/26 spans extractable, 1 fonts: {'Helvetica'} |

### Test 3: Math Balance

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **Local Math Engine** | 85 | 100 | **92.5** | 13ms | Detected discrepancy=$45.0, opening=$5,000.00, closing=$10,850.21 |
| 2 | **Groq AI** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 3 | **OpenRouter AI** | 0 | 0 | **0.0** | 0ms | API key not configured |
| 4 | **Gemini AI** | 0 | 0 | **0.0** | 1333ms | HTTP 400: {
  "error": {
    "code": 400,
    "message": "API key not valid. Please pass a valid API key.",
    "status": "INVALID_ARGUMENT",
    "details": [
      {
        "@type": "type.googleapis.com/googl |
| 5 | **Document AI** | 0 | 0 | **0.0** | 2059ms | HTTP 401 |

### Test 4: Visual QA

| Rank | Tool | Correctness | Fidelity | Avg | Latency | Details |
|---|---|---|---|---|---|---|
| 1 | **SSIM + Tile-Max + pHash** | 100 | 100 | **100.0** | 15544ms | SSIMΓëê0.997970, max_tile=0.583347, diff_px=72852, region_detect=Y |
| 2 | **Applitools Eyes** | 40 | 100 | **70.0** | 2546ms | Visual AI diff=MISSED, output_len=111 |
| 3 | **pdfRest Cloud** | 0 | 0 | **0.0** | 3144ms | HTTP 404: { "error":"Invalid endpoint. Please refer to documentation at https://pdfrest.com/documentation/" } |
| 4 | **Gemini Vision** | 0 | 0 | **0.0** | 3419ms | HTTP 400 |

## Production Fallback Routing Logic

Based on the empirical scores above, the recommended fallback chains are:

### Test 1: Extraction
```
  PRIMARY: LlamaParse                (Avg=98, C=100, F=96)
  FALLBACK-1: Offline Heuristic         (Avg=93, C=88, F=98)
  FALLBACK-2: PyMuPDF Built-in          (Avg=48, C=0, F=95)
```

### Test 2: Fidelity Edit
```
  PRIMARY: pymupdfpro                (Avg=88, C=90, F=85)
  FALLBACK-1: Pdfium                    (Avg=76, C=60, F=92)
  FALLBACK-2: Typst Reconstruct         (Avg=70, C=75, F=65)
```

### Test 3: Math Balance
```
  PRIMARY: Local Math Engine         (Avg=92, C=85, F=100)
```

### Test 4: Visual QA
```
  PRIMARY: SSIM + Tile-Max + pHash   (Avg=100, C=100, F=100)
  FALLBACK-1: Applitools Eyes           (Avg=70, C=40, F=100)
```

