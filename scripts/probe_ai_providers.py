#!/usr/bin/env python3
"""
AI Provider Fidelity Probe
===========================
Tests all configured AI providers against the 7 AU bank PDFs.
Reports extraction accuracy, latency, and assigns a fidelity score.

Usage:
    python3 scripts/probe_ai_providers.py \
        --pdf-dir "AU Bank Statements" \
        --out audit-evidence/provider-probe \
        [--providers gemini,llamaparse,mistral,offline]
"""

import argparse
import json
import os
import sys
import time
from pathlib import Path
from datetime import datetime

try:
    import fitz  # PyMuPDF
except ImportError:
    print("[ERROR] PyMuPDF not installed. Run: pip install pymupdf")
    sys.exit(1)

# ── Bank definitions ──────────────────────────────────────────────────────────
BANK_PDFS = {
    "ANZ":       "anz_example.pdf",
    "Bankwest":  "bankwest_example.pdf",
    "CommBank":  "commbank_smartaccess_example.pdf",
    "ING":       "ing_orangeeveryday_example.pdf",
    "Macquarie": "macquarie_example.pdf",
    "NAB":       "fallback.pdf",
    "Westpac":   "westpac_choicebasic_example.pdf",
}


def probe_offline(pdf_path: Path) -> dict:
    """Probe the offline PyMuPDF geometry extractor."""
    start = time.time()
    try:
        doc = fitz.open(str(pdf_path))
        all_text = []
        for page in doc:
            blocks = page.get_text("blocks")
            for b in blocks:
                text = b[4].strip()
                if text:
                    all_text.append(text)
        doc.close()
        latency = time.time() - start
        # Count lines that look like transactions (contain a dollar amount)
        import re
        tx_lines = [t for t in all_text if re.search(r'\$?\d[\d,]+\.\d{2}', t)]
        return {
            "status": "PASS",
            "provider": "Offline (PyMuPDF)",
            "latency_s": round(latency, 3),
            "text_blocks": len(all_text),
            "transaction_lines": len(tx_lines),
            "score": min(100, len(tx_lines) * 5),
        }
    except Exception as ex:
        return {"status": "ERROR", "provider": "Offline (PyMuPDF)", "reason": str(ex)}


def probe_gemini(pdf_path: Path, api_key: str) -> dict:
    """Probe Google Gemini Vision for transaction extraction."""
    start = time.time()
    try:
        from google import genai as google_genai
        from PIL import Image
        import io

        client = google_genai.Client(api_key=api_key)

        # Render first page to image
        doc = fitz.open(str(pdf_path))
        page = doc[0]
        mat = fitz.Matrix(150 / 72, 150 / 72)
        pix = page.get_pixmap(matrix=mat, alpha=False)
        img_bytes = pix.tobytes("png")
        doc.close()

        img = Image.open(io.BytesIO(img_bytes))
        import base64
        b64 = base64.b64encode(img_bytes).decode()

        response = client.models.generate_content(
            model="gemini-flash-latest",
            contents=[
                {"role": "user", "parts": [
                    {"text": "Extract all financial transactions from this bank statement. Return a JSON array with fields: date, description, debit, credit, balance."},
                    {"inline_data": {"mime_type": "image/png", "data": b64}}
                ]}
            ]
        )
        latency = time.time() - start

        text = response.text
        import re
        tx_matches = re.findall(r'\{[^{}]+\}', text)
        return {
            "status": "PASS",
            "provider": "Gemini 2.0 Flash",
            "latency_s": round(latency, 3),
            "transactions_found": len(tx_matches),
            "score": min(100, len(tx_matches) * 8),
            "raw_length": len(text),
        }
    except Exception as ex:
        return {"status": "UNAVAILABLE" if "api_key" in str(ex).lower() else "ERROR",
                "provider": "Gemini 2.0 Flash", "reason": str(ex)[:200]}


def probe_mistral(pdf_path: Path, api_key: str, model: str = "mistral-small-latest") -> dict:
    """Probe Mistral for transaction extraction via OCR text."""
    start = time.time()
    try:
        import requests

        # Extract text with PyMuPDF first
        doc = fitz.open(str(pdf_path))
        text = ""
        for page in doc[:2]:
            text += page.get_text()
        doc.close()
        text = text[:3000]  # Limit to avoid token overflow

        headers = {"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"}
        payload = {
            "model": model,
            "messages": [
                {"role": "user", "content": f"Extract all financial transactions from this bank statement text as a JSON array with date, description, debit, credit, balance fields. Text:\n\n{text[:2000]}"}
            ],
            "max_tokens": 2000,
        }
        resp = requests.post("https://api.mistral.ai/v1/chat/completions",
                             headers=headers, json=payload, timeout=60)
        latency = time.time() - start
        resp.raise_for_status()
        result = resp.json()
        content = result["choices"][0]["message"]["content"]

        import re
        tx_matches = re.findall(r'\{[^{}]+\}', content)
        return {
            "status": "PASS",
            "provider": f"Mistral ({model})",
            "latency_s": round(latency, 3),
            "transactions_found": len(tx_matches),
            "score": min(100, len(tx_matches) * 8),
        }
    except Exception as ex:
        return {"status": "UNAVAILABLE" if "401" in str(ex) else "ERROR",
                "provider": f"Mistral ({model})", "reason": str(ex)[:200]}


def probe_llamaparse(pdf_path: Path, api_key: str) -> dict:
    """Probe LlamaParse for structured extraction."""
    start = time.time()
    try:
        import requests

        with open(pdf_path, "rb") as f:
            files = {"file": (pdf_path.name, f, "application/pdf")}
            headers = {"Authorization": f"Bearer {api_key}"}
            resp = requests.post(
                "https://api.cloud.llamaindex.ai/api/parsing/upload",
                headers=headers, files=files, timeout=60
            )
        latency_upload = time.time() - start
        resp.raise_for_status()
        job_id = resp.json().get("id")

        # Poll for result
        for _ in range(20):
            time.sleep(3)
            r = requests.get(
                f"https://api.cloud.llamaindex.ai/api/parsing/job/{job_id}",
                headers=headers, timeout=15
            )
            status = r.json().get("status")
            if status == "SUCCESS":
                break
            elif status in ("ERROR", "CANCELLED"):
                raise Exception(f"LlamaParse job failed: {status}")

        r2 = requests.get(
            f"https://api.cloud.llamaindex.ai/api/parsing/job/{job_id}/result/markdown",
            headers=headers, timeout=15
        )
        content = r2.text
        latency = time.time() - start

        import re
        tx_matches = re.findall(r'\|.*\$?\d[\d,]+\.\d{2}.*\|', content)
        return {
            "status": "PASS",
            "provider": "LlamaParse",
            "latency_s": round(latency, 3),
            "transaction_rows": len(tx_matches),
            "score": min(100, len(tx_matches) * 6),
        }
    except Exception as ex:
        return {"status": "UNAVAILABLE" if "401" in str(ex) or "403" in str(ex) else "ERROR",
                "provider": "LlamaParse", "reason": str(ex)[:200]}


def run_probe(pdf_dir: Path, out_dir: Path, providers: list) -> dict:
    """Run all provider probes across all 7 banks."""
    out_dir.mkdir(parents=True, exist_ok=True)

    # Load env vars
    env_file = Path("bank-statement-fidelity-editor.env")
    if not env_file.exists():
        env_file = Path(os.environ.get("ENV_FILE", ".env"))

    env = {}
    if env_file.exists():
        with open(env_file) as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, _, v = line.partition("=")
                    env[k.strip()] = v.strip().strip('"').strip("'")

    # Merge with os.environ
    for k in ["GEMINI_API_KEY", "LLAMAPARSE_API_KEY", "MISTRAL_API_KEY", "MISTRAL_MODEL",
              "OPENROUTER_API_KEY", "PYMUPDF_PRO_KEY"]:
        if k in os.environ:
            env[k] = os.environ[k]

    all_results = {}

    for bank, filename in BANK_PDFS.items():
        pdf_path = pdf_dir / filename
        if not pdf_path.exists():
            print(f"[SKIP] {bank}: PDF not found")
            all_results[bank] = {"status": "SKIP"}
            continue

        print(f"\n[{bank}] {filename}")
        bank_results = {}

        if "offline" in providers:
            r = probe_offline(pdf_path)
            bank_results["offline"] = r
            print(f"  Offline: {r['status']} | {r.get('transaction_lines', '?')} tx | {r.get('latency_s', '?')}s")

        if "gemini" in providers and env.get("GEMINI_API_KEY"):
            r = probe_gemini(pdf_path, env["GEMINI_API_KEY"])
            bank_results["gemini"] = r
            print(f"  Gemini: {r['status']} | {r.get('transactions_found', '?')} tx | {r.get('latency_s', '?')}s")
        elif "gemini" in providers:
            bank_results["gemini"] = {"status": "UNAVAILABLE", "reason": "GEMINI_API_KEY not set"}
            print(f"  Gemini: UNAVAILABLE (no key)")

        if "mistral" in providers and env.get("MISTRAL_API_KEY"):
            # mistral-ocr is a document API, not a chat model — fall back to mistral-small for chat
            raw_model = env.get("MISTRAL_MODEL", "mistral-small-latest")
            model = raw_model if raw_model != "mistral-ocr" else "mistral-small-latest"
            r = probe_mistral(pdf_path, env["MISTRAL_API_KEY"], model)
            bank_results["mistral"] = r
            print(f"  Mistral: {r['status']} | {r.get('transactions_found', '?')} tx | {r.get('latency_s', '?')}s")
        elif "mistral" in providers:
            bank_results["mistral"] = {"status": "UNAVAILABLE", "reason": "MISTRAL_API_KEY not set"}
            print(f"  Mistral: UNAVAILABLE (no key)")

        if "llamaparse" in providers and env.get("LLAMAPARSE_API_KEY"):
            r = probe_llamaparse(pdf_path, env["LLAMAPARSE_API_KEY"])
            bank_results["llamaparse"] = r
            print(f"  LlamaParse: {r['status']} | {r.get('transaction_rows', '?')} rows | {r.get('latency_s', '?')}s")
        elif "llamaparse" in providers:
            bank_results["llamaparse"] = {"status": "UNAVAILABLE", "reason": "LLAMAPARSE_API_KEY not set"}
            print(f"  LlamaParse: UNAVAILABLE (no key)")

        all_results[bank] = bank_results

    return all_results


def write_report(results: dict, out_dir: Path):
    """Write JSON and Markdown provider comparison report."""
    json_path = out_dir / "provider_probe_results.json"
    with open(json_path, "w") as f:
        json.dump(results, f, indent=2)

    md_path = out_dir / "PROVIDER_COMPARISON_REPORT.md"
    providers = ["offline", "gemini", "mistral", "llamaparse"]

    with open(md_path, "w") as f:
        f.write("# AI Provider Fidelity Comparison Report\n\n")
        f.write(f"**Generated:** {datetime.utcnow().strftime('%Y-%m-%d %H:%M UTC')}\n\n")
        f.write("## Provider × Bank Matrix\n\n")

        header = "| Bank | " + " | ".join(p.title() for p in providers) + " |\n"
        sep = "| :--- | " + " | ".join(":---:" for _ in providers) + " |\n"
        f.write(header)
        f.write(sep)

        for bank, bank_data in results.items():
            if bank_data.get("status") == "SKIP":
                continue
            row = f"| **{bank}** |"
            for p in providers:
                pd = bank_data.get(p, {})
                status = pd.get("status", "—")
                score = pd.get("score", pd.get("transaction_lines", "—"))
                if status == "PASS":
                    row += f" ✅ {score} |"
                elif status == "UNAVAILABLE":
                    row += " ⛔ N/A |"
                elif status == "ERROR":
                    row += " ❌ ERR |"
                else:
                    row += " — |"
            f.write(row + "\n")

    print(f"\n[REPORT] {md_path}")
    print(f"[JSON]   {json_path}")
    return md_path, json_path


def main():
    parser = argparse.ArgumentParser(description="AI Provider Fidelity Probe")
    parser.add_argument("--pdf-dir", default="AU Bank Statements")
    parser.add_argument("--out", default="audit-evidence/provider-probe")
    parser.add_argument("--providers", default="offline,gemini,mistral,llamaparse")
    parser.add_argument("--env-file", default="bank-statement-fidelity-editor.env")
    args = parser.parse_args()

    os.environ.setdefault("ENV_FILE", args.env_file)
    providers = [p.strip().lower() for p in args.providers.split(",")]

    pdf_dir = Path(args.pdf_dir)
    out_dir = Path(args.out)

    print(f"AI Provider Fidelity Probe")
    print(f"  PDF dir   : {pdf_dir}")
    print(f"  Output    : {out_dir}")
    print(f"  Providers : {', '.join(providers)}")
    print()

    results = run_probe(pdf_dir, out_dir, providers)
    write_report(results, out_dir)

    passed = sum(
        1 for bank_data in results.values()
        if isinstance(bank_data, dict) and any(
            p.get("status") == "PASS" for p in bank_data.values()
            if isinstance(p, dict)
        )
    )
    print(f"\nBanks with at least one PASS: {passed}/{len(results)}")


if __name__ == "__main__":
    main()
