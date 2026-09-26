#!/usr/bin/env python3
"""
Extraction benchmark: offline parser vs local VLM, scored against the PDF text layer.

Ground truth = every money amount in the PDF's embedded text layer (digital
statements only). For each contestant we report:
  recall         share of text-layer amounts the contestant reproduced
  hallucination  share of the contestant's amounts that are NOT in the text layer
  failed         contestant produced nothing (counts as recall 0, never skipped)
  seconds        wall time

Contestants
  offline   the project binary's `extract` command (deterministic offline parser)
  vlm       LOCAL_VLM_URL / LOCAL_VLM_MODEL (OpenAI-compatible), page images @ --dpi
Cloud parsers (Reducto etc.) are deliberately NOT called here: they spend API
credits and upload statements. Feed their JSON in with --extra-json name=path.

Usage:
  python scripts/gx10/benchmark_extraction.py --pdf-dir "AU Bank Statements" \
      --binary target/release/dual-core-pdf-pipeline --out bench_out
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Set

try:
    import pymupdf as fitz
except ImportError:  # pragma: no cover
    import fitz  # type: ignore[no-redef]

AMOUNT_RE = re.compile(r"\(?-?\$?\s?\d{1,3}(?:,\d{3})*\.\d{2}\)?|\(?-?\$?\s?\d+\.\d{2}\)?")
MONEY_KEYS = {"debit", "credit", "running_balance", "balance"}

PROMPT = os.environ.get("LOCAL_VLM_PROMPT") or (
    Path(__file__).resolve().parents[2] / "assets" / "vlm_prompt.txt"
).read_text(encoding="utf-8").strip()


def norm_amount(raw: object) -> Optional[str]:
    """Canonical absolute amount string (no currency, commas, sign, CR/DR)."""
    if raw is None:
        return None
    s = re.sub(r"[^\d.]", "", str(raw))
    if not s or s == ".":
        return None
    try:
        d = Decimal(s)
    except InvalidOperation:
        return None
    return f"{d:.2f}"


def amounts_in_text(text: str) -> Set[str]:
    return {a for m in AMOUNT_RE.findall(text) if (a := norm_amount(m))}


def ground_truth(pdf: Path) -> Set[str]:
    with fitz.open(pdf) as doc:
        return amounts_in_text("\n".join(page.get_text() for page in doc))


def walk_money(node: object) -> Iterable[object]:
    if isinstance(node, dict):
        for k, v in node.items():
            if k in MONEY_KEYS and not isinstance(v, (dict, list)):
                yield v
            else:
                yield from walk_money(v)
    elif isinstance(node, list):
        for item in node:
            yield from walk_money(item)


def score(pred: Set[str], truth: Set[str], seconds: float, failed: bool, note: str = "") -> Dict:
    hit = pred & truth
    return {
        "recall": round(len(hit) / len(truth), 4) if truth else None,
        "hallucination": round(len(pred - truth) / len(pred), 4) if pred else None,
        "amounts_found": len(pred),
        "amounts_truth": len(truth),
        "failed": failed,
        "seconds": round(seconds, 2),
        "note": note,
    }


def run_offline(binary: Path, pdf: Path, workdir: Path, timeout: int) -> Dict:
    out = workdir / f"{pdf.stem}.offline.json"
    env = {**os.environ, "REDUCTO_API_KEY": "", "LLAMAPARSE_API_KEY": ""}
    env.setdefault("DUAL_CORE_PASSPHRASE", "benchmark-local-passphrase")
    t0 = time.time()
    try:
        proc = subprocess.run(
            [str(binary), "extract", "-i", str(pdf), "-o", str(out)],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=timeout, env=env,
        )
    except subprocess.TimeoutExpired:
        return {"pred": set(), "seconds": time.time() - t0, "failed": True, "note": "timeout"}
    dt = time.time() - t0
    if proc.returncode != 0 or not out.is_file():
        return {"pred": set(), "seconds": dt, "failed": True, "note": f"exit {proc.returncode}"}
    try:
        data = json.loads(out.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return {"pred": set(), "seconds": dt, "failed": True, "note": f"bad output: {exc}"}
    pred = {a for v in walk_money(data) if (a := norm_amount(v))}
    return {"pred": pred, "seconds": dt, "failed": not pred, "note": "" if pred else "no amounts"}


def parse_rows(text: str) -> List[dict]:
    t = text.strip()
    t = re.sub(r"^```(?:json)?", "", t).removesuffix("```").strip()
    rows = json.loads(t)
    if not isinstance(rows, list):
        raise ValueError("reply is not a JSON array")
    return [r for r in rows if isinstance(r, dict)]


def run_vlm(pdf: Path, url: str, model: str, key: Optional[str], dpi: int, timeout: int) -> Dict:
    pred: Set[str] = set()
    t0 = time.time()
    notes: List[str] = []
    with fitz.open(pdf) as doc:
        for i, page in enumerate(doc):
            png = page.get_pixmap(dpi=dpi).tobytes("png")
            body = json.dumps({
                "model": model, "temperature": 0.0,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": PROMPT},
                    {"type": "image_url", "image_url": {
                        "url": "data:image/png;base64," + base64.b64encode(png).decode()}},
                ]}],
            }).encode()
            req = urllib.request.Request(
                f"{url.rstrip('/')}/chat/completions", data=body,
                headers={"Content-Type": "application/json",
                         **({"Authorization": f"Bearer {key}"} if key else {})},
            )
            try:
                with urllib.request.urlopen(req, timeout=timeout) as resp:
                    reply = json.load(resp)["choices"][0]["message"]["content"]
                try:
                    for row in parse_rows(reply):
                        for k in MONEY_KEYS:
                            if (a := norm_amount(row.get(k))):
                                pred.add(a)
                except (ValueError, json.JSONDecodeError):
                    # OCR-native output (markdown/HTML): score every money amount it emitted.
                    pred |= amounts_in_text(reply)
                    if "raw-output" not in notes:
                        notes.append("raw-output")
            except (urllib.error.URLError, TimeoutError, KeyError, ValueError, json.JSONDecodeError) as exc:
                notes.append(f"p{i + 1}: {type(exc).__name__}")
    return {"pred": pred, "seconds": time.time() - t0, "failed": not pred, "note": "; ".join(notes)}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pdf-dir", type=Path, required=True)
    ap.add_argument("--binary", type=Path, help="dual-core-pdf-pipeline executable (enables 'offline')")
    ap.add_argument("--out", type=Path, default=Path("bench_out"))
    ap.add_argument("--dpi", type=int, default=200)
    ap.add_argument("--timeout", type=int, default=300)
    ap.add_argument("--extra-json", action="append", default=[], metavar="NAME=PATH_OR_DIR",
                    help="score pre-computed parser JSON (dir of <pdf-stem>.json); repeatable")
    args = ap.parse_args()

    vlm_url = os.environ.get("LOCAL_VLM_URL", "").strip()
    vlm_model = os.environ.get("LOCAL_VLM_MODEL", "").strip()
    vlm_key = os.environ.get("LOCAL_VLM_API_KEY") or None
    use_vlm = bool(vlm_url and vlm_model)
    if not args.binary and not use_vlm and not args.extra_json:
        print("nothing to benchmark: pass --binary, set LOCAL_VLM_URL+LOCAL_VLM_MODEL, or --extra-json",
              file=sys.stderr)
        return 2

    args.out.mkdir(parents=True, exist_ok=True)
    extras = {}
    for spec in args.extra_json:
        name, _, path = spec.partition("=")
        if not name or not path:
            print(f"bad --extra-json: {spec}", file=sys.stderr)
            return 2
        extras[name] = Path(path)

    results: Dict[str, Dict[str, Dict]] = {}
    with tempfile.TemporaryDirectory() as tmp:
        for pdf in sorted(args.pdf_dir.glob("*.pdf")):
            truth = ground_truth(pdf)
            if not truth:
                print(f"skip {pdf.name}: no text layer (scan) - needs a labelled set", file=sys.stderr)
                continue
            per: Dict[str, Dict] = {}
            if args.binary:
                r = run_offline(args.binary, pdf, Path(tmp), args.timeout)
                per["offline"] = score(r["pred"], truth, r["seconds"], r["failed"], r["note"])
            if use_vlm:
                r = run_vlm(pdf, vlm_url, vlm_model, vlm_key, args.dpi, args.timeout)
                per["vlm"] = score(r["pred"], truth, r["seconds"], r["failed"], r["note"])
            for name, base in extras.items():
                f = base / f"{pdf.stem}.json" if base.is_dir() else base
                try:
                    pred = {a for v in walk_money(json.loads(f.read_text(encoding="utf-8")))
                            if (a := norm_amount(v))}
                    per[name] = score(pred, truth, 0.0, not pred)
                except (OSError, json.JSONDecodeError) as exc:
                    per[name] = score(set(), truth, 0.0, True, f"{type(exc).__name__}")
            results[pdf.name] = per
            print(f"{pdf.name}: " + ", ".join(
                f"{k} recall={v['recall']} halluc={v['hallucination']}{' FAILED' if v['failed'] else ''}"
                for k, v in per.items()))

    contestants = sorted({k for per in results.values() for k in per})
    summary = {}
    for c in contestants:
        rows = [per[c] for per in results.values() if c in per]
        rec = [r["recall"] or 0.0 for r in rows]  # failures count as 0
        hal = [r["hallucination"] for r in rows if r["hallucination"] is not None]
        summary[c] = {
            "files": len(rows),
            "failed": sum(r["failed"] for r in rows),
            "mean_recall": round(sum(rec) / len(rec), 4) if rec else None,
            "mean_hallucination": round(sum(hal) / len(hal), 4) if hal else None,
            "total_seconds": round(sum(r["seconds"] for r in rows), 1),
        }
    (args.out / "benchmark.json").write_text(
        json.dumps({"summary": summary, "per_file": results}, indent=2), encoding="utf-8")
    md = ["| contestant | files | failed | mean recall | mean hallucination | seconds |", "|---|---|---|---|---|---|"]
    md += [f"| {c} | {s['files']} | {s['failed']} | {s['mean_recall']} | {s['mean_hallucination']} | {s['total_seconds']} |"
           for c, s in summary.items()]
    (args.out / "benchmark.md").write_text("\n".join(md) + "\n", encoding="utf-8")
    print("\n".join(md))
    return 0


if __name__ == "__main__":
    sys.exit(main())
