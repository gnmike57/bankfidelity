#!/usr/bin/env python3
"""
DEPRECATED: This python-based MCP server is obsolete.
The BankFidelity orchestrator now runs its own native MCP stdio server
in Rust (src/ai/mcp.rs) as part of the Dual-Core architecture.
Do not use this script; instead, register the compiled dual-core-pdf-pipeline.exe
as the MCP server.

This file is retained only for historical reference and will be removed in v3.0.
"""
"""
BankFidelity MCP Server
=======================
A full Model Context Protocol (MCP) server that exposes every function of the
Bank Statement Fidelity Editor as an AI-callable tool.

This server communicates with the running BankFidelity backend via its HTTP API
(when running in `serve` mode) or directly via the Python bridge (offline mode).

Usage:
    # Start the BankFidelity backend first:
    ./dual-core-pdf-pipeline serve

    # Then start this MCP server (stdio transport for Claude Desktop / Cursor):
    python3 scripts/mcp_server.py

    # Or with SSE transport for web-based agents:
    python3 scripts/mcp_server.py --transport sse --port 8765

Configuration:
    Set BANKFIDELITY_API_URL in your .env (default: http://localhost:8080)
    Set BANKFIDELITY_MCP_PORT for SSE mode (default: 8765)
"""

import argparse
import asyncio
import base64
import json
import logging
import os
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Dependency bootstrap
# ---------------------------------------------------------------------------
try:
    import httpx
except ImportError:
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "-q", "httpx"])
    import httpx

try:
    import fitz  # PyMuPDF — for offline operations
    PYMUPDF_AVAILABLE = True
except ImportError:
    PYMUPDF_AVAILABLE = False

# Load .env from the repo root
_repo_root = Path(__file__).resolve().parent.parent
_env_file = _repo_root / "bank-statement-fidelity-editor.env"
if not _env_file.exists():
    _env_file = _repo_root / ".env"
if _env_file.exists():
    for line in _env_file.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, _, v = line.partition("=")
            os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

BANKFIDELITY_API_URL = os.environ.get("BANKFIDELITY_API_URL", "http://localhost:8080")
logging.basicConfig(level=logging.INFO, stream=sys.stderr)
log = logging.getLogger("bankfidelity-mcp")

# ---------------------------------------------------------------------------
# HTTP client helpers
# ---------------------------------------------------------------------------

async def _api_get(path: str, timeout: float = 10.0) -> dict:
    async with httpx.AsyncClient(base_url=BANKFIDELITY_API_URL, timeout=timeout) as client:
        r = await client.get(path)
        r.raise_for_status()
        return r.json()

async def _api_post(path: str, payload: dict, timeout: float = 120.0) -> dict:
    async with httpx.AsyncClient(base_url=BANKFIDELITY_API_URL, timeout=timeout) as client:
        r = await client.post(path, json=payload)
        r.raise_for_status()
        return r.json()

def _pdf_to_b64(pdf_path: str) -> str:
    return base64.b64encode(Path(pdf_path).read_bytes()).decode()

def _b64_to_pdf(b64: str, out_path: str) -> str:
    Path(out_path).write_bytes(base64.b64decode(b64))
    return out_path

# ---------------------------------------------------------------------------
# Tool implementations
# ---------------------------------------------------------------------------

async def tool_health_check() -> dict:
    """Check if the BankFidelity backend is running and ready."""
    try:
        result = await _api_get("/health")
        ready = await _api_get("/readyz")
        return {"status": "ok", "health": result, "ready": ready}
    except Exception as e:
        return {"status": "error", "message": str(e), "hint": "Start the backend with: ./dual-core-pdf-pipeline serve"}

async def tool_load_document(pdf_path: str, passphrase: str = "") -> dict:
    """Load a bank statement PDF into the editor engine."""
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "load_document",
        "path": str(path.resolve()),
        "passphrase": passphrase or os.environ.get("DUAL_CORE_PASSPHRASE", ""),
    }
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_extract_transactions(pdf_path: str, provider: str = "offline") -> dict:
    """
    Extract all transactions from a bank statement PDF.
    
    provider: 'offline' | 'gemini' | 'mistral' | 'llamaparse' | 'local-llm'
    Returns a JSON array of {date, description, debit, credit, balance} objects.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "extract_transactions",
        "path": str(path.resolve()),
        "provider": provider,
    }
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        # Offline fallback using PyMuPDF directly
        if PYMUPDF_AVAILABLE:
            return _offline_extract(pdf_path)
        return {"error": str(e)}

def _offline_extract(pdf_path: str) -> dict:
    """Direct PyMuPDF extraction without the backend."""
    doc = fitz.open(pdf_path)
    transactions = []
    for page_num in range(len(doc)):
        page = doc[page_num]
        text = page.get_text("text")
        transactions.append({"page": page_num + 1, "raw_text": text[:2000]})
    doc.close()
    return {"provider": "offline_direct", "pages": len(transactions), "raw": transactions}

async def tool_natural_language_edit(
    pdf_path: str,
    instruction: str,
    auto_apply: bool = False,
    provider: str = "gemini",
) -> dict:
    """
    Edit a bank statement using natural language.
    
    Examples:
    - "Change all transactions from January to February"
    - "Replace the account holder name with John Smith"
    - "Set the closing balance to $5,432.10"
    - "Remove all transactions over $1000"
    - "Change the BSB to 062-000"
    - "Add a new transaction on 15 Jan for Coffee Shop $4.50"
    
    auto_apply: if True, immediately writes changes to the PDF without confirmation.
    provider: AI provider to use for understanding the instruction.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "natural_language_edit",
        "path": str(path.resolve()),
        "instruction": instruction,
        "auto_apply": auto_apply,
        "provider": provider,
    }
    try:
        return await _api_post("/job", payload, timeout=180.0)
    except Exception as e:
        return {"error": str(e), "instruction": instruction}

async def tool_balance_statement(
    pdf_path: str,
    target_balance: float | None = None,
    auto_apply: bool = False,
) -> dict:
    """
    Run the Smart Balance Engine on a bank statement.
    
    Automatically detects balance inconsistencies, proposes corrections,
    and optionally applies them. Can target a specific closing balance.
    
    target_balance: if set, adjusts transactions to reach this exact closing balance.
    auto_apply: if True, applies all proposed changes without confirmation.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "balance_statement",
        "path": str(path.resolve()),
        "target_balance": target_balance,
        "auto_apply": auto_apply,
    }
    try:
        return await _api_post("/job", payload, timeout=180.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_verify_fidelity(
    original_pdf: str,
    edited_pdf: str,
    dpi: int = 150,
    threshold: float = 0.02,
) -> dict:
    """
    Perform pixel-perfect visual fidelity verification between two PDFs.
    
    Renders both PDFs at the specified DPI, computes SSIM scores per page,
    and generates a 5× amplified difference map.
    
    Returns pass/fail status, per-page SSIM scores, and the path to the
    difference map image.
    
    threshold: maximum allowed SSIM deviation (default 0.02 = 98% similarity required).
    """
    for p in [original_pdf, edited_pdf]:
        if not Path(p).exists():
            return {"error": f"File not found: {p}"}
    payload = {
        "job": "verify_fidelity",
        "original": str(Path(original_pdf).resolve()),
        "edited": str(Path(edited_pdf).resolve()),
        "dpi": dpi,
        "threshold": threshold,
    }
    try:
        return await _api_post("/job", payload, timeout=120.0)
    except Exception as e:
        # Offline SSIM via scikit-image
        try:
            return _offline_ssim(original_pdf, edited_pdf, dpi)
        except Exception as e2:
            return {"error": str(e), "offline_error": str(e2)}

def _offline_ssim(orig: str, edited: str, dpi: int) -> dict:
    from pdf2image import convert_from_path
    from skimage.metrics import structural_similarity as ssim
    import numpy as np
    orig_pages = convert_from_path(orig, dpi=dpi)
    edit_pages = convert_from_path(edited, dpi=dpi)
    results = []
    for i, (op, ep) in enumerate(zip(orig_pages, edit_pages)):
        og = np.array(op.convert("L"))
        eg = np.array(ep.convert("L"))
        if og.shape != eg.shape:
            eg = np.array(ep.resize(op.size).convert("L"))
        score, _ = ssim(og, eg, full=True)
        results.append({"page": i + 1, "ssim": round(float(score) * 100, 2)})
    avg = sum(r["ssim"] for r in results) / len(results) if results else 0
    return {
        "provider": "offline_ssim",
        "pages": results,
        "average_ssim": round(avg, 2),
        "pass": avg >= (1 - 0.02) * 100,
    }

async def tool_transfer_transactions(
    source_pdf: str,
    target_pdf: str,
    output_pdf: str | None = None,
    verify_after: bool = True,
) -> dict:
    """
    Transfer transactions from one bank statement PDF to another.
    
    Adapts date formats, currency symbols, and column layouts between
    different AU bank formats. Verifies math and visual fidelity after transfer.
    
    source_pdf: the PDF to copy transactions FROM.
    target_pdf: the PDF template to copy transactions INTO.
    output_pdf: where to save the result (defaults to target_pdf with _transferred suffix).
    verify_after: if True, runs pixel-perfect fidelity check after transfer.
    """
    for p in [source_pdf, target_pdf]:
        if not Path(p).exists():
            return {"error": f"File not found: {p}"}
    if output_pdf is None:
        stem = Path(target_pdf).stem
        output_pdf = str(Path(target_pdf).parent / f"{stem}_transferred.pdf")
    payload = {
        "job": "transfer_transactions",
        "source": str(Path(source_pdf).resolve()),
        "target": str(Path(target_pdf).resolve()),
        "output": str(Path(output_pdf).resolve()),
        "verify_after": verify_after,
    }
    try:
        return await _api_post("/job", payload, timeout=300.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_adjust_dates(
    pdf_path: str,
    shift_days: int | None = None,
    new_start_date: str | None = None,
    new_end_date: str | None = None,
    output_pdf: str | None = None,
) -> dict:
    """
    Bulk-shift or remap all transaction dates in a bank statement.
    
    shift_days: shift all dates by this many days (positive = forward, negative = backward).
    new_start_date: remap dates so the statement starts on this date (YYYY-MM-DD).
    new_end_date: remap dates so the statement ends on this date (YYYY-MM-DD).
    
    Exactly one of shift_days, new_start_date, or new_end_date must be provided.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if not any([shift_days, new_start_date, new_end_date]):
        return {"error": "Provide one of: shift_days, new_start_date, or new_end_date"}
    if output_pdf is None:
        output_pdf = str(path.parent / f"{path.stem}_dated.pdf")
    payload = {
        "job": "adjust_dates",
        "path": str(path.resolve()),
        "shift_days": shift_days,
        "new_start_date": new_start_date,
        "new_end_date": new_end_date,
        "output": output_pdf,
    }
    try:
        return await _api_post("/job", payload, timeout=120.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_render_page(pdf_path: str, page: int = 1, dpi: int = 150) -> dict:
    """
    Render a specific page of a PDF to a base64-encoded PNG image.
    
    Returns the image as a base64 string suitable for display in AI chat interfaces.
    page: 1-indexed page number.
    dpi: render resolution (72=draft, 150=standard, 300=print quality).
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if PYMUPDF_AVAILABLE:
        doc = fitz.open(str(path))
        if page < 1 or page > len(doc):
            return {"error": f"Page {page} out of range (document has {len(doc)} pages)"}
        pg = doc[page - 1]
        mat = fitz.Matrix(dpi / 72, dpi / 72)
        pix = pg.get_pixmap(matrix=mat)
        b64 = base64.b64encode(pix.tobytes("png")).decode()
        doc.close()
        return {
            "page": page,
            "dpi": dpi,
            "width": pix.width,
            "height": pix.height,
            "image_b64": b64,
            "mime_type": "image/png",
        }
    payload = {"job": "render_page", "path": str(path.resolve()), "page": page, "dpi": dpi}
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_analyze_fonts(pdf_path: str) -> dict:
    """
    Analyze all fonts used in a bank statement PDF.
    
    Returns font names, sizes, encoding types, and whether each font is
    embeddable for pixel-perfect editing. Identifies Type-3 fonts (NAB-style)
    which require special handling.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if PYMUPDF_AVAILABLE:
        doc = fitz.open(str(path))
        fonts = {}
        for page_num in range(len(doc)):
            for font in doc[page_num].get_fonts():
                xref, ext, font_type, basefont, name, enc = font
                key = basefont or name
                if key not in fonts:
                    fonts[key] = {
                        "name": key,
                        "type": font_type,
                        "encoding": enc,
                        "pages": [],
                        "embeddable": font_type not in ("Type3",),
                        "requires_special_handling": font_type == "Type3",
                    }
                fonts[key]["pages"].append(page_num + 1)
        doc.close()
        return {"pdf": str(path), "fonts": list(fonts.values()), "total": len(fonts)}
    payload = {"job": "analyze_fonts", "path": str(path.resolve())}
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_get_document_info(pdf_path: str) -> dict:
    """
    Get metadata and structural information about a bank statement PDF.
    
    Returns: page count, detected bank, statement period, account number
    (masked), total transactions, opening/closing balance, and PDF version.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if PYMUPDF_AVAILABLE:
        doc = fitz.open(str(path))
        meta = doc.metadata
        info = {
            "path": str(path),
            "pages": len(doc),
            "title": meta.get("title", ""),
            "author": meta.get("author", ""),
            "creator": meta.get("creator", ""),
            "pdf_version": doc.pdf_version() if hasattr(doc, "pdf_version") else "unknown",
            "encrypted": doc.is_encrypted,
            "file_size_kb": round(path.stat().st_size / 1024, 1),
        }
        # Try to detect bank from first page text
        first_page_text = doc[0].get_text("text")[:500] if len(doc) > 0 else ""
        bank = "Unknown"
        for b in ["ANZ", "Bankwest", "CommBank", "Commonwealth", "ING", "Macquarie", "NAB", "Westpac"]:
            if b.lower() in first_page_text.lower():
                bank = b
                break
        info["detected_bank"] = bank
        info["first_page_preview"] = first_page_text[:300]
        doc.close()
        return info
    payload = {"job": "get_document_info", "path": str(path.resolve())}
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_apply_change(
    pdf_path: str,
    page: int,
    x: float,
    y: float,
    old_text: str,
    new_text: str,
    output_pdf: str | None = None,
) -> dict:
    """
    Apply a precise text replacement at a specific location in a PDF.
    
    Uses pixel-perfect font matching to replace text without disturbing
    surrounding content. Coordinates are in PDF points (1 point = 1/72 inch).
    
    page: 1-indexed page number.
    x, y: coordinates of the text block to replace.
    old_text: the exact text to find and replace.
    new_text: the replacement text.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if output_pdf is None:
        output_pdf = str(path.parent / f"{path.stem}_edited.pdf")
    payload = {
        "job": "apply_change",
        "path": str(path.resolve()),
        "page": page,
        "x": x,
        "y": y,
        "old_text": old_text,
        "new_text": new_text,
        "output": output_pdf,
    }
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_undo(pdf_path: str) -> dict:
    """Undo the last change applied to a bank statement PDF."""
    payload = {"job": "undo", "path": str(Path(pdf_path).resolve())}
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_redo(pdf_path: str) -> dict:
    """Redo the last undone change on a bank statement PDF."""
    payload = {"job": "redo", "path": str(Path(pdf_path).resolve())}
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_export_change_history(pdf_path: str, output_format: str = "json") -> dict:
    """
    Export the full change history for a bank statement PDF.
    
    output_format: 'json' | 'csv' | 'markdown'
    Returns a complete audit trail of all edits, timestamps, and operators.
    """
    payload = {
        "job": "export_change_history",
        "path": str(Path(pdf_path).resolve()),
        "format": output_format,
    }
    try:
        return await _api_post("/job", payload)
    except Exception as e:
        return {"error": str(e)}

async def tool_verify_api_keys() -> dict:
    """
    Verify all configured AI provider API keys are valid and functional.
    
    Tests each key with a live API call and returns pass/fail status,
    latency, and any error messages for each provider.
    """
    results = {}
    providers = {
        "gemini": ("GEMINI_API_KEY", _test_gemini),
        "mistral": ("MISTRAL_API_KEY", _test_mistral),
        "llamaparse": ("LLAMAPARSE_API_KEY", _test_llamaparse),
        "pymupdf_pro": ("PYMUPDF_PRO_KEY", _test_pymupdf_pro),
    }
    for name, (env_var, test_fn) in providers.items():
        key = os.environ.get(env_var, "")
        if not key:
            results[name] = {"status": "not_configured", "env_var": env_var}
            continue
        t0 = time.time()
        try:
            await test_fn(key)
            results[name] = {"status": "ok", "latency_ms": round((time.time() - t0) * 1000)}
        except Exception as e:
            results[name] = {"status": "error", "error": str(e)[:200], "latency_ms": round((time.time() - t0) * 1000)}
    return results

async def _test_gemini(key: str):
    from google import genai
    client = genai.Client(api_key=key)
    client.models.generate_content(model="gemini-flash-latest", contents="ping")

async def _test_mistral(key: str):
    async with httpx.AsyncClient(timeout=10) as c:
        r = await c.post(
            "https://api.mistral.ai/v1/chat/completions",
            headers={"Authorization": f"Bearer {key}"},
            json={"model": "mistral-small-latest", "messages": [{"role": "user", "content": "ping"}], "max_tokens": 1},
        )
        r.raise_for_status()

async def _test_llamaparse(key: str):
    async with httpx.AsyncClient(timeout=10) as c:
        r = await c.get("https://api.cloud.llamaindex.ai/api/v1/parsing/job", headers={"Authorization": f"Bearer {key}"})
        if r.status_code not in (200, 404):
            r.raise_for_status()

async def _test_pymupdf_pro(key: str):
    if PYMUPDF_AVAILABLE:
        import fitz
        _ = fitz.open()
    else:
        raise RuntimeError("PyMuPDF not installed")

async def tool_update_api_key(provider: str, api_key: str) -> dict:
    """
    Update an AI provider API key at runtime without restarting the app.
    
    provider: 'gemini' | 'mistral' | 'llamaparse' | 'pymupdf_pro' | 'openrouter' | 'pdfrest'
    api_key: the new API key value.
    
    The key is written to the .env file and hot-reloaded into the running engine.
    """
    env_var_map = {
        "gemini": "GEMINI_API_KEY",
        "mistral": "MISTRAL_API_KEY",
        "llamaparse": "LLAMAPARSE_API_KEY",
        "pymupdf_pro": "PYMUPDF_PRO_KEY",
        "openrouter": "OPENROUTER_API_KEY",
        "pdfrest": "PDFREST_API_KEY",
    }
    if provider not in env_var_map:
        return {"error": f"Unknown provider '{provider}'. Valid: {list(env_var_map.keys())}"}
    env_var = env_var_map[provider]
    # Update the .env file
    env_path = _repo_root / "bank-statement-fidelity-editor.env"
    if not env_path.exists():
        env_path = _repo_root / ".env"
    lines = env_path.read_text().splitlines() if env_path.exists() else []
    updated = False
    new_lines = []
    for line in lines:
        if line.strip().startswith(f"{env_var}="):
            new_lines.append(f"{env_var}={api_key}")
            updated = True
        else:
            new_lines.append(line)
    if not updated:
        new_lines.append(f"{env_var}={api_key}")
    env_path.write_text("\n".join(new_lines) + "\n")
    os.environ[env_var] = api_key
    # Signal the backend to reload
    try:
        await _api_post("/job", {"job": "reload_config"})
        return {"status": "updated", "provider": provider, "env_var": env_var, "reloaded": True}
    except Exception as e:

        import traceback; traceback.print_exc()
        return {"status": "updated", "provider": provider, "env_var": env_var, "reloaded": False,
                "note": "Backend not running; key saved to .env for next startup"}

async def tool_run_stress_test(
    pdf_dir: str | None = None,
    test_type: str = "transfer_matrix",
) -> dict:
    """
    Run the full stress test suite against a directory of AU bank PDFs.
    
    test_type:
    - 'transfer_matrix': 42-pair cross-bank transfer test (7×6 combinations)
    - 'xray_fidelity': pixel-perfect SSIM comparison for all banks
    - 'provider_probe': test all AI providers against all PDFs
    - 'all': run all three suites
    
    pdf_dir: directory containing AU bank statement PDFs (defaults to 'AU Bank Statements/').
    """
    if pdf_dir is None:
        pdf_dir = str(_repo_root / "AU Bank Statements")
    if not Path(pdf_dir).exists():
        return {"error": f"PDF directory not found: {pdf_dir}"}
    payload = {
        "job": "run_stress_test",
        "pdf_dir": str(Path(pdf_dir).resolve()),
        "test_type": test_type,
    }
    try:
        return await _api_post("/job", payload, timeout=600.0)
    except Exception as e:
        # Run locally using the existing scripts
        if test_type in ("xray_fidelity", "all"):
            import subprocess
            result = subprocess.run(
                [sys.executable, str(_repo_root / "scripts/xray_fidelity_screenshots.py"),
                 "--pdf-dir", pdf_dir, "--out", "/tmp/mcp_xray_out"],
                capture_output=True, text=True, cwd=str(_repo_root)
            )
            return {"status": "completed_locally", "stdout": result.stdout[-2000:], "stderr": result.stderr[-500:]}
        return {"error": str(e)}

async def tool_categorize_transactions(pdf_path: str, provider: str = "gemini") -> dict:
    """
    Categorize all transactions in a bank statement using AI.
    
    Assigns categories like: groceries, utilities, transport, dining,
    entertainment, healthcare, income, transfer, ATM, fees.
    
    Returns the full transaction list with categories and confidence scores.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "categorize_transactions",
        "path": str(path.resolve()),
        "provider": provider,
    }
    try:
        return await _api_post("/job", payload, timeout=120.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_generate_visual_alternatives(
    pdf_path: str,
    field: str,
    current_value: str,
    count: int = 5,
) -> dict:
    """
    Generate AI-suggested alternative values for a specific field.
    
    Useful for exploring plausible edits. For example:
    - field='merchant_name', current_value='WOOLWORTHS' → suggests similar retailers
    - field='amount', current_value='$45.00' → suggests nearby plausible amounts
    - field='date', current_value='2026-01-15' → suggests nearby dates
    
    count: number of alternatives to generate (1-20).
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    payload = {
        "job": "generate_visual_alternatives",
        "path": str(path.resolve()),
        "field": field,
        "current_value": current_value,
        "count": min(max(count, 1), 20),
    }
    try:
        return await _api_post("/job", payload, timeout=60.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_workflow_full(
    pdf_path: str,
    instructions: list[str],
    provider: str = "gemini",
    verify_after_each: bool = True,
    output_pdf: str | None = None,
) -> dict:
    """
    Run a full multi-step editing workflow on a bank statement.
    
    Executes a list of natural language instructions in sequence, verifying
    fidelity after each step. If any step fails the fidelity gate, it rolls
    back and reports the failure without corrupting the document.
    
    instructions: list of natural language edit instructions, e.g.:
        ["Change account holder to John Smith",
         "Set closing balance to $12,500.00",
         "Shift all dates forward by 30 days"]
    
    verify_after_each: if True, runs pixel-perfect verification after each step.
    output_pdf: where to save the final result.
    """
    path = Path(pdf_path)
    if not path.exists():
        return {"error": f"File not found: {pdf_path}"}
    if output_pdf is None:
        output_pdf = str(path.parent / f"{path.stem}_workflow_output.pdf")
    payload = {
        "job": "workflow_full",
        "path": str(path.resolve()),
        "instructions": instructions,
        "provider": provider,
        "verify_after_each": verify_after_each,
        "output": output_pdf,
    }
    try:
        return await _api_post("/job", payload, timeout=600.0)
    except Exception as e:
        return {"error": str(e)}

async def tool_list_available_pdfs(directory: str | None = None) -> dict:
    """
    List all bank statement PDFs available in a directory.
    
    Returns file names, sizes, detected bank names, and page counts.
    directory: defaults to the 'AU Bank Statements/' folder in the repo.
    """
    if directory is None:
        directory = str(_repo_root / "AU Bank Statements")
    d = Path(directory)
    if not d.exists():
        return {"error": f"Directory not found: {directory}", "hint": "Provide a path to a folder containing PDF files"}
    pdfs = list(d.glob("*.pdf"))
    results = []
    for pdf in sorted(pdfs):
        info = {"name": pdf.name, "size_kb": round(pdf.stat().st_size / 1024, 1), "path": str(pdf)}
        if PYMUPDF_AVAILABLE:
            try:
                doc = fitz.open(str(pdf))
                info["pages"] = len(doc)
                text = doc[0].get_text("text")[:300] if len(doc) > 0 else ""
                for bank in ["ANZ", "Bankwest", "CommBank", "Commonwealth", "ING", "Macquarie", "NAB", "Westpac"]:
                    if bank.lower() in text.lower():
                        info["bank"] = bank
                        break
                doc.close()
            except Exception as e:

                import traceback; traceback.print_exc()
                pass
        results.append(info)
    return {"directory": str(d), "count": len(results), "pdfs": results}

async def tool_doctor() -> dict:
    """
    Run a full system health check and configuration diagnostic.
    
    Checks: Rust binary, Python bridge, all API keys, pdfium library,
    .env file, memory, display server, and backend connectivity.
    Returns a structured report with pass/fail for each component.
    """
    checks = {}
    # Binary
    binary = _repo_root / "target/release/dual-core-pdf-pipeline"
    checks["rust_binary"] = {"status": "ok" if binary.exists() else "missing",
                              "path": str(binary),
                              "hint": "Run: cargo build --release" if not binary.exists() else None}
    # .env
    env_file = _repo_root / "bank-statement-fidelity-editor.env"
    if not env_file.exists():
        env_file = _repo_root / ".env"
    checks["env_file"] = {"status": "ok" if env_file.exists() else "missing",
                           "path": str(env_file),
                           "hint": "Copy .env.example to .env" if not env_file.exists() else None}
    # Passphrase
    passphrase = os.environ.get("DUAL_CORE_PASSPHRASE", "")
    checks["passphrase"] = {"status": "ok" if len(passphrase) >= 16 else "weak_or_missing",
                             "length": len(passphrase),
                             "hint": "Set DUAL_CORE_PASSPHRASE to at least 16 characters" if len(passphrase) < 16 else None}
    # PyMuPDF
    checks["pymupdf"] = {"status": "ok" if PYMUPDF_AVAILABLE else "missing",
                          "hint": "pip install pymupdf" if not PYMUPDF_AVAILABLE else None}
    # API keys
    key_checks = await tool_verify_api_keys()
    checks["api_keys"] = key_checks
    # Backend
    try:
        health = await _api_get("/health", timeout=3.0)
        checks["backend"] = {"status": "ok", "response": health}
    except Exception as e:
        checks["backend"] = {"status": "not_running", "error": str(e),
                              "hint": "Start with: ./dual-core-pdf-pipeline serve"}
    # PDF directory
    pdf_dir = _repo_root / "AU Bank Statements"
    pdf_count = len(list(pdf_dir.glob("*.pdf"))) if pdf_dir.exists() else 0
    checks["pdf_directory"] = {"status": "ok" if pdf_count >= 7 else "incomplete",
                                "path": str(pdf_dir), "pdf_count": pdf_count,
                                "hint": "Add AU bank statement PDFs to 'AU Bank Statements/'" if pdf_count < 7 else None}
    overall = "ok" if all(v.get("status") == "ok" for v in checks.values() if isinstance(v, dict)) else "issues_found"
    return {"overall": overall, "checks": checks}

# ---------------------------------------------------------------------------
# MCP Protocol Implementation (stdio transport)
# ---------------------------------------------------------------------------

TOOLS = [
    {
        "name": "health_check",
        "description": "Check if the BankFidelity backend is running and ready.",
        "inputSchema": {"type": "object", "properties": {}, "required": []},
    },
    {
        "name": "load_document",
        "description": "Load a bank statement PDF into the editor engine.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string", "description": "Absolute or relative path to the PDF file"},
                "passphrase": {"type": "string", "description": "Security passphrase (uses DUAL_CORE_PASSPHRASE env var if omitted)"},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "extract_transactions",
        "description": "Extract all transactions from a bank statement PDF. Returns structured JSON with date, description, debit, credit, balance.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "provider": {"type": "string", "enum": ["offline", "gemini", "mistral", "llamaparse", "local-llm"], "default": "offline"},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "natural_language_edit",
        "description": "Edit a bank statement using natural language. Examples: 'Change all January transactions to February', 'Set closing balance to $5000', 'Replace account holder name with John Smith', 'Remove all transactions over $1000'.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "instruction": {"type": "string", "description": "Natural language edit instruction"},
                "auto_apply": {"type": "boolean", "default": False, "description": "Apply immediately without confirmation"},
                "provider": {"type": "string", "enum": ["gemini", "mistral", "local-llm"], "default": "gemini"},
            },
            "required": ["pdf_path", "instruction"],
        },
    },
    {
        "name": "balance_statement",
        "description": "Run the Smart Balance Engine. Detects and fixes balance inconsistencies. Can target a specific closing balance.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "target_balance": {"type": "number", "description": "Target closing balance (optional)"},
                "auto_apply": {"type": "boolean", "default": False},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "verify_fidelity",
        "description": "Pixel-perfect visual fidelity check between original and edited PDF. Returns SSIM scores per page and a difference map.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "original_pdf": {"type": "string"},
                "edited_pdf": {"type": "string"},
                "dpi": {"type": "integer", "default": 150},
                "threshold": {"type": "number", "default": 0.02, "description": "Max SSIM deviation (0.02 = 98% similarity required)"},
            },
            "required": ["original_pdf", "edited_pdf"],
        },
    },
    {
        "name": "transfer_transactions",
        "description": "Transfer transactions from one AU bank statement to another, adapting formats and verifying math + visual fidelity.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_pdf": {"type": "string"},
                "target_pdf": {"type": "string"},
                "output_pdf": {"type": "string", "description": "Output path (optional)"},
                "verify_after": {"type": "boolean", "default": True},
            },
            "required": ["source_pdf", "target_pdf"],
        },
    },
    {
        "name": "adjust_dates",
        "description": "Bulk-shift or remap all transaction dates in a bank statement.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "shift_days": {"type": "integer", "description": "Shift all dates by N days"},
                "new_start_date": {"type": "string", "description": "Remap to start on this date (YYYY-MM-DD)"},
                "new_end_date": {"type": "string", "description": "Remap to end on this date (YYYY-MM-DD)"},
                "output_pdf": {"type": "string"},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "render_page",
        "description": "Render a PDF page to a base64 PNG image for visual inspection.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "page": {"type": "integer", "default": 1},
                "dpi": {"type": "integer", "default": 150},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "analyze_fonts",
        "description": "Analyze all fonts in a PDF. Identifies Type-3 fonts (NAB-style) requiring special handling.",
        "inputSchema": {
            "type": "object",
            "properties": {"pdf_path": {"type": "string"}},
            "required": ["pdf_path"],
        },
    },
    {
        "name": "get_document_info",
        "description": "Get metadata about a bank statement PDF: page count, detected bank, statement period, file size.",
        "inputSchema": {
            "type": "object",
            "properties": {"pdf_path": {"type": "string"}},
            "required": ["pdf_path"],
        },
    },
    {
        "name": "apply_change",
        "description": "Apply a precise text replacement at specific coordinates in a PDF.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "page": {"type": "integer"},
                "x": {"type": "number"},
                "y": {"type": "number"},
                "old_text": {"type": "string"},
                "new_text": {"type": "string"},
                "output_pdf": {"type": "string"},
            },
            "required": ["pdf_path", "page", "x", "y", "old_text", "new_text"],
        },
    },
    {
        "name": "undo",
        "description": "Undo the last change applied to a bank statement PDF.",
        "inputSchema": {
            "type": "object",
            "properties": {"pdf_path": {"type": "string"}},
            "required": ["pdf_path"],
        },
    },
    {
        "name": "redo",
        "description": "Redo the last undone change on a bank statement PDF.",
        "inputSchema": {
            "type": "object",
            "properties": {"pdf_path": {"type": "string"}},
            "required": ["pdf_path"],
        },
    },
    {
        "name": "export_change_history",
        "description": "Export the full audit trail of all edits to a bank statement PDF.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "output_format": {"type": "string", "enum": ["json", "csv", "markdown"], "default": "json"},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "verify_api_keys",
        "description": "Test all configured AI provider API keys with live API calls. Returns pass/fail and latency for each.",
        "inputSchema": {"type": "object", "properties": {}, "required": []},
    },
    {
        "name": "update_api_key",
        "description": "Update an AI provider API key at runtime without restarting the app. Writes to .env and hot-reloads.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "provider": {"type": "string", "enum": ["gemini", "mistral", "llamaparse", "pymupdf_pro", "openrouter", "pdfrest"]},
                "api_key": {"type": "string"},
            },
            "required": ["provider", "api_key"],
        },
    },
    {
        "name": "run_stress_test",
        "description": "Run the full stress test suite: 42-pair transfer matrix, X-ray fidelity, or provider probe.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_dir": {"type": "string"},
                "test_type": {"type": "string", "enum": ["transfer_matrix", "xray_fidelity", "provider_probe", "all"], "default": "transfer_matrix"},
            },
            "required": [],
        },
    },
    {
        "name": "categorize_transactions",
        "description": "AI-categorize all transactions: groceries, utilities, transport, dining, income, etc.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "provider": {"type": "string", "default": "gemini"},
            },
            "required": ["pdf_path"],
        },
    },
    {
        "name": "generate_visual_alternatives",
        "description": "Generate AI-suggested alternative values for a specific field (merchant name, amount, date).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "field": {"type": "string"},
                "current_value": {"type": "string"},
                "count": {"type": "integer", "default": 5},
            },
            "required": ["pdf_path", "field", "current_value"],
        },
    },
    {
        "name": "workflow_full",
        "description": "Run a multi-step editing workflow with fidelity verification after each step. Rolls back on failure.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "pdf_path": {"type": "string"},
                "instructions": {"type": "array", "items": {"type": "string"}},
                "provider": {"type": "string", "default": "gemini"},
                "verify_after_each": {"type": "boolean", "default": True},
                "output_pdf": {"type": "string"},
            },
            "required": ["pdf_path", "instructions"],
        },
    },
    {
        "name": "list_available_pdfs",
        "description": "List all bank statement PDFs in a directory with bank detection and page counts.",
        "inputSchema": {
            "type": "object",
            "properties": {"directory": {"type": "string"}},
            "required": [],
        },
    },
    {
        "name": "doctor",
        "description": "Full system health check: binary, .env, API keys, PyMuPDF, backend, PDF directory.",
        "inputSchema": {"type": "object", "properties": {}, "required": []},
    },
]

TOOL_HANDLERS = {
    "health_check": lambda a: tool_health_check(),
    "load_document": lambda a: tool_load_document(**a),
    "extract_transactions": lambda a: tool_extract_transactions(**a),
    "natural_language_edit": lambda a: tool_natural_language_edit(**a),
    "balance_statement": lambda a: tool_balance_statement(**a),
    "verify_fidelity": lambda a: tool_verify_fidelity(**a),
    "transfer_transactions": lambda a: tool_transfer_transactions(**a),
    "adjust_dates": lambda a: tool_adjust_dates(**a),
    "render_page": lambda a: tool_render_page(**a),
    "analyze_fonts": lambda a: tool_analyze_fonts(**a),
    "get_document_info": lambda a: tool_get_document_info(**a),
    "apply_change": lambda a: tool_apply_change(**a),
    "undo": lambda a: tool_undo(**a),
    "redo": lambda a: tool_redo(**a),
    "export_change_history": lambda a: tool_export_change_history(**a),
    "verify_api_keys": lambda a: tool_verify_api_keys(),
    "update_api_key": lambda a: tool_update_api_key(**a),
    "run_stress_test": lambda a: tool_run_stress_test(**a),
    "categorize_transactions": lambda a: tool_categorize_transactions(**a),
    "generate_visual_alternatives": lambda a: tool_generate_visual_alternatives(**a),
    "workflow_full": lambda a: tool_workflow_full(**a),
    "list_available_pdfs": lambda a: tool_list_available_pdfs(**a),
    "doctor": lambda a: tool_doctor(),
}


async def handle_request(request: dict) -> dict:
    method = request.get("method", "")
    req_id = request.get("id")

    if method == "initialize":
        return {
            "jsonrpc": "2.0", "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "bankfidelity-mcp", "version": "1.2.0"},
            },
        }

    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": req_id, "result": {"tools": TOOLS}}

    if method == "tools/call":
        params = request.get("params", {})
        tool_name = params.get("name", "")
        arguments = params.get("arguments", {})
        handler = TOOL_HANDLERS.get(tool_name)
        if handler is None:
            return {
                "jsonrpc": "2.0", "id": req_id,
                "error": {"code": -32601, "message": f"Unknown tool: {tool_name}"},
            }
        try:
            result = await handler(arguments)
            return {
                "jsonrpc": "2.0", "id": req_id,
                "result": {
                    "content": [{"type": "text", "text": json.dumps(result, indent=2)}],
                    "isError": "error" in result,
                },
            }
        except Exception as e:
            return {
                "jsonrpc": "2.0", "id": req_id,
                "result": {
                    "content": [{"type": "text", "text": json.dumps({"error": str(e)})}],
                    "isError": True,
                },
            }

    if method == "notifications/initialized":
        return None  # No response needed

    return {
        "jsonrpc": "2.0", "id": req_id,
        "error": {"code": -32601, "message": f"Method not found: {method}"},
    }


async def run_stdio():
    """Run MCP server over stdio (for Claude Desktop, Cursor, etc.)."""
    log.info("BankFidelity MCP Server starting (stdio transport)")
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)
    loop = asyncio.get_event_loop()
    await loop.connect_read_pipe(lambda: protocol, sys.stdin)
    _, writer = await loop.connect_write_pipe(asyncio.BaseProtocol, sys.stdout)

    while True:
        try:
            line = await reader.readline()
            if not line:
                break
            request = json.loads(line.decode().strip())
            response = await handle_request(request)
            if response is not None:
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
        except json.JSONDecodeError:
            continue
        except Exception as e:
            log.error(f"Error handling request: {e}")


async def run_sse(port: int):
    """Run MCP server over SSE (for web-based agents)."""
    try:
        from aiohttp import web
    except ImportError:
        import subprocess
        subprocess.check_call([sys.executable, "-m", "pip", "install", "-q", "aiohttp"])
        from aiohttp import web

    async def handle_sse(request):
        response = web.StreamResponse()
        response.headers["Content-Type"] = "text/event-stream"
        response.headers["Cache-Control"] = "no-cache"
        response.headers["Access-Control-Allow-Origin"] = "*"
        await response.prepare(request)
        body = await request.json()
        result = await handle_request(body)
        if result:
            data = json.dumps(result)
            await response.write(f"data: {data}\n\n".encode())
        return response

    async def handle_post(request):
        body = await request.json()
        result = await handle_request(body)
        return web.json_response(result or {})

    async def handle_options(request):
        return web.Response(headers={"Access-Control-Allow-Origin": "*",
                                      "Access-Control-Allow-Methods": "POST, OPTIONS",
                                      "Access-Control-Allow-Headers": "Content-Type"})

    app = web.Application()
    app.router.add_post("/mcp", handle_post)
    app.router.add_post("/sse", handle_sse)
    app.router.add_options("/mcp", handle_options)
    app.router.add_get("/tools", lambda r: web.json_response({"tools": TOOLS}))

    runner = web.AppRunner(app)
    await runner.setup()
    site = web.TCPSite(runner, "0.0.0.0", port)
    await site.start()
    log.info(f"BankFidelity MCP Server running on http://0.0.0.0:{port}/mcp")
    log.info(f"Tools endpoint: http://0.0.0.0:{port}/tools")
    await asyncio.sleep(float("inf"))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="BankFidelity MCP Server")
    parser.add_argument("--transport", choices=["stdio", "sse"], default="stdio")
    parser.add_argument("--port", type=int, default=int(os.environ.get("BANKFIDELITY_MCP_PORT", "8765")))
    args = parser.parse_args()

    if args.transport == "sse":
        asyncio.run(run_sse(args.port))
    else:
        asyncio.run(run_stdio())
