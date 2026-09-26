#!/usr/bin/env python3
"""
Field-level exact-match evaluation of a served VLM on synth_dataset.py output.

Scores held-out pages (default: holdout.jsonl, an unseen bank layout) against the
exact labels. Reports, per field and overall:
  exact      predicted string == label string (row-aligned)
  row_count  fraction of pages where the number of rows matches
Rows are aligned by index; a wrong row count therefore penalises later rows,
which is the behaviour you want for ledger transcription.

  LOCAL_VLM_URL=http://gx10.local:8000/v1 LOCAL_VLM_MODEL=<served-name> \
      python scripts/gx10/eval_synth.py --data data/synth --split holdout --limit 100
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

FIELDS = ("date", "description", "debit", "credit", "balance")


def ask(url: str, model: str, key: str | None, prompt: str, png: bytes, timeout: int) -> str:
    body = json.dumps({
        "model": model, "temperature": 0.0,
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode(png).decode()}},
        ]}],
    }).encode()
    req = urllib.request.Request(
        f"{url.rstrip('/')}/chat/completions", data=body,
        headers={"Content-Type": "application/json", **({"Authorization": f"Bearer {key}"} if key else {})})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.load(resp)["choices"][0]["message"]["content"]


def parse(text: str) -> list:
    t = re.sub(r"^```(?:json)?", "", text.strip()).removesuffix("```").strip()
    rows = json.loads(t)
    if not isinstance(rows, list):
        raise ValueError("not a JSON array")
    return [r for r in rows if isinstance(r, dict)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", type=Path, required=True)
    ap.add_argument("--split", default="holdout", choices=["holdout", "val", "train"])
    ap.add_argument("--limit", type=int, default=100)
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()

    url = os.environ.get("LOCAL_VLM_URL", "").strip()
    model = os.environ.get("LOCAL_VLM_MODEL", "").strip()
    if not url or not model:
        print("set LOCAL_VLM_URL and LOCAL_VLM_MODEL", file=sys.stderr)
        return 2
    key = os.environ.get("LOCAL_VLM_API_KEY") or None

    path = args.data / f"{args.split}.jsonl"
    recs = [json.loads(x) for x in path.read_text(encoding="utf-8").splitlines() if x.strip()][: args.limit]
    if not recs:
        print(f"{path} has no records", file=sys.stderr)
        return 2

    hit = {f: 0 for f in FIELDS}
    total = 0
    count_ok = failed = 0
    for rec in recs:
        truth = json.loads(rec["answer"])
        try:
            pred = parse(ask(url, model, key, rec["prompt"], (args.data / rec["image"]).read_bytes(), args.timeout))
        except (urllib.error.URLError, TimeoutError, KeyError, ValueError, json.JSONDecodeError):
            pred, failed = [], failed + 1
        count_ok += len(pred) == len(truth)
        for i, row in enumerate(truth):
            total += 1
            got = pred[i] if i < len(pred) else {}
            for f in FIELDS:
                hit[f] += (got.get(f) == row[f])
    report = {
        "split": args.split, "pages": len(recs), "failed_pages": failed,
        "row_count_match": round(count_ok / len(recs), 4),
        "field_exact": {f: round(hit[f] / total, 4) for f in FIELDS},
        "all_fields_exact": round(sum(hit.values()) / (total * len(FIELDS)), 4),
    }
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
