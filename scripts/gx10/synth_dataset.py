#!/usr/bin/env python3
"""
Synthetic statement-page dataset for fine-tuning a local VLM.

Pages are drawn with PyMuPDF from the repo's bank templates (column_x_ranges,
date_format, header_signatures) using random but arithmetically consistent
ledgers, so every label is exact by construction. Output (JSONL, one page per
line):

    {"image": "images/<id>.png", "prompt": "<assets/vlm_prompt.txt>",
     "answer": "<JSON array of rows>", "template": "<id>", "split": "train|val|holdout"}

Splits: one template can be held out entirely (--holdout-template) to measure
generalisation to an unseen bank layout; the rest is split 90/10 by page.

CAVEAT: synthetic pages teach layout/transcription, not real-world scan noise
or bank-specific typography. Mix in real, human-verified pages before trusting
a fine-tune, and never train on customer statements without explicit approval.

Usage:
  python scripts/gx10/synth_dataset.py --out data/synth --count 2000 --holdout-template westpac_choice_basic_au
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from datetime import date, timedelta
from decimal import Decimal
from pathlib import Path

import yaml

try:
    import pymupdf as fitz
except ImportError:  # pragma: no cover
    import fitz  # type: ignore[no-redef]

REPO = Path(__file__).resolve().parents[2]
PROMPT = (REPO / "assets" / "vlm_prompt.txt").read_text(encoding="utf-8").strip()
PAGE_W, PAGE_H = 595.0, 842.0

MERCHANTS = [
    "Woolworths", "Coles", "Bunnings Warehouse", "Uber Trip", "Netflix.com", "BP Service Station",
    "Telstra Bill", "AGL Energy", "Transport for NSW", "Chemist Warehouse", "JB Hi-Fi", "Kmart",
    "Salary ACME PTY LTD", "Transfer to Savings", "ATM Withdrawal", "Officeworks", "Optus",
    "Direct Debit Insurance", "Interest Credit", "Medicare Refund", "Rent Payment", "Spotify",
]
FONTS = ["helv", "tiro", "cour"]


ORDER = ("date", "description", "debit", "credit", "balance")


def layout_is_sound(cols: dict, max_overlap: float = 8.0) -> bool:
    """Columns present, left-to-right in ledger order, on the page, barely overlapping.

    Several auto-derived templates fail this (credit left of description, x past
    the page edge); drawing from them would produce garbled, mislabelled pages.
    """
    if not all(k in cols for k in ORDER):
        return False
    try:
        spans = [(float(cols[k][0]), float(cols[k][1])) for k in ORDER]
    except (TypeError, ValueError, IndexError):
        return False
    if any(x0 < 0 or x1 > PAGE_W - 8 or x1 - x0 < 40 for x0, x1 in spans):
        return False
    return all(
        spans[i][0] < spans[i + 1][0] and spans[i][1] - spans[i + 1][0] <= max_overlap
        for i in range(len(spans) - 1)
    )


def random_layout(rng: random.Random) -> dict:
    """A plausible AU-style ledger layout with randomised column widths."""
    x = rng.uniform(30, 60)
    cols = {}
    for name, (lo, hi) in (("date", (55, 80)), ("description", (150, 230))):
        w = rng.uniform(lo, hi)
        cols[name] = [x, x + w]
        x += w + rng.uniform(0, 8)
    w = min((PAGE_W - 12 - x) / 3 - 4, rng.uniform(80, 100))
    for name in ("debit", "credit", "balance"):
        cols[name] = [x, x + w]
        x += w + rng.uniform(0, 4)
    return {
        "id": "random",
        "date_format": rng.choice(["%d/%m/%Y", "%d %b %Y", "%d %b", "%d-%m-%y"]),
        "header_signatures": [rng.choice(["Everyday", "Savings", "Complete", "Access"]), "Account"],
        "column_x_ranges": cols,
    }


def load_templates(dirpath: Path) -> dict:
    out = {}
    for f in sorted(dirpath.glob("*.yaml")):
        try:
            t = yaml.safe_load(f.read_text(encoding="utf-8"))
        except yaml.YAMLError:
            continue
        cols = (t or {}).get("column_x_ranges") or {}
        if t and t.get("date_format") and layout_is_sound(cols):
            # Prefer the refined variant when both exist.
            tid = t.get("id") or f.stem.replace(".refined", "")
            if tid not in out or f.name.endswith(".refined.yaml"):
                out[tid] = t
    return out


def money(d: Decimal, cr_suffix: bool) -> str:
    s = f"{d:,.2f}"
    return f"{s} CR" if cr_suffix else s


def make_ledger(rng: random.Random, n: int, fmt: str, cr_suffix: bool):
    bal = Decimal(rng.randint(50_000, 5_000_000)) / 100
    day = date(2025, 1, 1) + timedelta(days=rng.randint(0, 300))
    rows = []
    for _ in range(n):
        day += timedelta(days=rng.randint(0, 3))
        amt = Decimal(rng.randint(150, 250_000)) / 100
        is_credit = rng.random() < 0.3
        bal = bal + amt if is_credit else max(Decimal("0.00"), bal - amt)
        rows.append({
            "date": day.strftime(fmt),
            "description": rng.choice(MERCHANTS) + (f" {rng.randint(100, 9999)}" if rng.random() < 0.5 else ""),
            "debit": None if is_credit else money(amt, False),
            "credit": money(amt, False) if is_credit else None,
            "balance": money(bal, cr_suffix),
        })
    return rows


def render_page(rng: random.Random, tpl: dict, rows: list, out_png: Path, dpi: int) -> None:
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    font = rng.choice(FONTS)
    size = rng.choice([8.0, 8.5, 9.0, 9.5])
    cols = tpl["column_x_ranges"]
    title = " ".join(str(s) for s in tpl.get("header_signatures", [])[:4]) or "Statement"
    page.insert_text((36, 50), title, fontname="hebo", fontsize=14)
    page.insert_text((36, 68), "Statement of account", fontname=font, fontsize=size)
    y = 110.0
    for name, (x0, x1) in cols.items():
        if name in ("debit", "credit", "balance"):
            w = fitz.get_text_length(name.capitalize(), fontname="hebo", fontsize=size)
            page.insert_text((x1 - 4 - w, y), name.capitalize(), fontname="hebo", fontsize=size)
        else:
            page.insert_text((x0, y), name.capitalize(), fontname="hebo", fontsize=size)
    y += size * 2.2
    step = size * rng.choice([1.6, 1.8, 2.0])
    for row in rows:
        for name, (x0, x1) in cols.items():
            val = row[name]
            if val is None:
                continue
            if name in ("debit", "credit", "balance"):
                w = fitz.get_text_length(val, fontname=font, fontsize=size)
                if w > x1 - x0 - 4:
                    raise RuntimeError(f"{name} value too wide for its column: {val!r}")
                page.insert_text((x1 - 4 - w, y), val, fontname=font, fontsize=size)
            else:
                # Clip description to its column so neighbouring cells never overlap.
                maxw = x1 - x0 - 6
                text = val
                while text and fitz.get_text_length(text, fontname=font, fontsize=size) > maxw:
                    text = text[:-1]
                page.insert_text((x0, y), text, fontname=font, fontsize=size)
                row[name] = text.rstrip()  # label must equal what was actually drawn
        y += step
    # Every label string must be present in the page's text layer.
    drawn = " ".join(page.get_text().split())
    for row in rows:
        for name, val in row.items():
            if val is not None and " ".join(val.split()) not in drawn:
                raise RuntimeError(f"label/page mismatch on {name!r}: {val!r}")
    page.get_pixmap(dpi=dpi).save(out_png)
    doc.close()


def rows_that_fit(rng: random.Random, tpl: dict) -> int:
    return rng.randint(12, 34)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--templates", type=Path, default=REPO / "bank_templates")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--count", type=int, default=500)
    ap.add_argument("--dpi", type=int, default=200)
    ap.add_argument("--seed", type=int, default=1234)
    ap.add_argument("--holdout-template", default=None)
    args = ap.parse_args()

    templates = load_templates(args.templates)
    print(f"sound templates: {sorted(templates) or 'none (random layouts only)'}")
    if args.holdout_template and args.holdout_template not in templates:
        print(f"unknown holdout template {args.holdout_template!r}; have: {sorted(templates)}", file=sys.stderr)
        return 2

    rng = random.Random(args.seed)
    img_dir = args.out / "images"
    img_dir.mkdir(parents=True, exist_ok=True)
    files = {s: (args.out / f"{s}.jsonl").open("w", encoding="utf-8") for s in ("train", "val", "holdout")}
    ids = sorted(templates)
    try:
        for i in range(args.count):
            png = img_dir / f"{i:06d}.png"
            for _attempt in range(8):
                if ids and rng.random() < 0.5:
                    tid = rng.choice(ids)
                    tpl = templates[tid]
                else:
                    tpl = random_layout(rng)
                    tid = "random"
                rows = make_ledger(
                    rng, rows_that_fit(rng, tpl), tpl["date_format"], cr_suffix=rng.random() < 0.3
                )
                try:
                    render_page(rng, tpl, rows, png, args.dpi)
                    break
                except RuntimeError:
                    continue
            else:
                print(f"page {i}: no consistent page after 8 attempts", file=sys.stderr)
                return 1
            label = [{k: r[k] for k in ("date", "description", "debit", "credit", "balance")} for r in rows]
            if tid == args.holdout_template:
                split = "holdout"
            else:
                split = "val" if rng.random() < 0.1 else "train"
            files[split].write(json.dumps({
                "image": f"images/{png.name}", "prompt": PROMPT,
                "answer": json.dumps(label, ensure_ascii=False),
                "template": tid, "split": split,
            }, ensure_ascii=False) + "\n")
    finally:
        for f in files.values():
            f.close()
    counts = {s: sum(1 for _ in (args.out / f"{s}.jsonl").open(encoding="utf-8")) for s in files}
    print(f"wrote {args.count} pages to {args.out}: {counts}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
