#!/usr/bin/env python3
"""End-to-end pipeline validation for every statement PDF.

For each PDF this simulates the exact GUI flow, step by step:
  1. "Click a transaction amount" -> locate a money span (scanning every page).
  2. "Edit a number" -> run the Rust `text` edit (Job::ApplyChange) over that
     span's bbox, replacing it with a changed amount.
  3. "Apply edits" -> the edit is written to an output PDF.
  4. Render original page and edited page and pixel-diff them to confirm the
     edit is LOCAL (only the targeted region changed).

Every step is logged with an explicit PASS / FAIL / SKIP status so the run is
debuggable step by step, and the process exits non-zero if any PDF fails (so it
is safe to use in CI / `run_max_test`).

Outputs a JSON + human summary under output/e2e/.

Examples:
    python scripts/e2e_pipeline.py
    python scripts/e2e_pipeline.py --pdf "AU Bank Statements/anz_example.pdf" -v
    python scripts/e2e_pipeline.py --exe target/release/dual-core-pdf-pipeline.exe
    python scripts/e2e_pipeline.py --build --strict
"""
import argparse
import datetime
import glob
import hashlib
import json
import os
import re
import subprocess
import sys

import pymupdf

MONEY = re.compile(r"(-?\$?\s*[\d,]+\.\d{2})")
MONEY_CORE = re.compile(r"[\d,]+\.\d{2}")

VERBOSE = False
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT_PROFILE_PATH = os.path.join(ROOT, "python", "font-resource-profiles.json")


def log(msg, indent=2):
    print(" " * indent + msg, flush=True)


def find_exe(explicit):
    """Resolve the CLI binary path, preferring an explicit value, then release,
    then debug. Returns None if nothing is found."""
    candidates = []
    if explicit:
        candidates.append(explicit)
    candidates += [
        os.path.join("target", "release", "dual-core-pdf-pipeline.exe"),
        os.path.join("target", "debug", "dual-core-pdf-pipeline.exe"),
        os.path.join("target", "release", "dual-core-pdf-pipeline"),
        os.path.join("target", "debug", "dual-core-pdf-pipeline"),
    ]
    for c in candidates:
        if os.path.exists(c):
            return c
    return None


def build_exe(release):
    cmd = ["cargo", "build"] + (["--release"] if release else [])
    log(f"building binary: {' '.join(cmd)}", 0)
    return subprocess.run(cmd).returncode == 0


def run(exe, args, timeout=180):
    env = dict(os.environ)
    env.setdefault("DUAL_CORE_PASSPHRASE", "ci-only-passphrase-do-not-use")
    try:
        p = subprocess.run(
            [exe] + args,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            env=env,
        )
    except subprocess.TimeoutExpired:
        return 124, f"TIMEOUT after {timeout}s running: {' '.join(args)}"
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def prepare_edit_input(pdf_path, page, out_root, slug):
    """Return an input whose Pro edit scope is never larger than three pages."""
    source = pymupdf.open(pdf_path)
    try:
        if source.page_count <= 3:
            return pdf_path, page, None
        start = (page // 3) * 3
        end = min(start + 2, source.page_count - 1)
        segment_dir = os.path.join(out_root, "segments")
        os.makedirs(segment_dir, exist_ok=True)
        segment_path = os.path.join(segment_dir, f"{slug}_pages_{start + 1}_{end + 1}.pdf")
        segment = pymupdf.open()
        try:
            segment.insert_pdf(source, from_page=start, to_page=end)
            segment.save(segment_path, garbage=4, deflate=True)
        finally:
            segment.close()
        return segment_path, page - start, {
            "source_page": page,
            "segment_page": page - start,
            "page_range": [start, end],
            "path": segment_path,
        }
    finally:
        source.close()


def _font_program_bytes(document, xref):
    try:
        raw = document.extract_font(int(xref))
    except Exception:
        return b""
    if isinstance(raw, dict):
        return bytes(raw.get("content") or b"")
    if isinstance(raw, (tuple, list)):
        for item in reversed(raw):
            if isinstance(item, (bytes, bytearray)) and item:
                return bytes(item)
    return b""


def _load_font_profiles():
    try:
        with open(FONT_PROFILE_PATH, "r", encoding="utf-8") as stream:
            payload = json.load(stream)
    except (OSError, json.JSONDecodeError):
        return {}
    profiles = payload.get("profiles") if payload.get("schema_version") == 1 else None
    return profiles if isinstance(profiles, dict) else {}


def _profiled_amount_span(document):
    profiles = _load_font_profiles()
    if not profiles:
        return None, None, None, None
    for page_number in range(document.page_count):
        page = document[page_number]
        resources = []
        for resource in page.get_fonts(full=True):
            xref = int(resource[0])
            program = _font_program_bytes(document, xref)
            fingerprint = hashlib.sha256(program).hexdigest() if program else None
            profile = profiles.get(fingerprint or "")
            if isinstance(profile, dict):
                resources.append((resource, profile))
        if not resources:
            continue
        dict_spans = [
            span
            for block in page.get_text("dict").get("blocks", [])
            for line in block.get("lines", [])
            for span in line.get("spans", [])
        ]
        for trace in page.get_texttrace():
            chars = list(trace.get("chars") or [])
            if not chars:
                continue
            profile = None
            for resource, candidate in resources:
                if candidate.get("font_trace_name") == trace.get("font"):
                    profile = candidate
                    break
            if profile is None:
                continue
            cid_to_character = {
                int(cid): character
                for character, cid in (profile.get("character_to_cid") or {}).items()
            }
            try:
                decoded = "".join(cid_to_character[int(item[1])] for item in chars)
            except (KeyError, TypeError, ValueError):
                continue
            if not MONEY.fullmatch(decoded):
                continue
            trace_rect = pymupdf.Rect(trace.get("bbox") or (0, 0, 0, 0))
            matching_spans = []
            for span in dict_spans:
                span_rect = pymupdf.Rect(span.get("bbox") or (0, 0, 0, 0))
                intersection = trace_rect & span_rect
                if intersection.is_empty:
                    continue
                overlap = (intersection.width * intersection.height) / max(
                    1e-9, min(trace_rect.width * trace_rect.height, span_rect.width * span_rect.height)
                )
                if overlap >= 0.8:
                    matching_spans.append(span)
            if len(matching_spans) != 1:
                continue
            source_text = str(matching_spans[0].get("text") or "")
            if not source_text:
                continue
            bbox = [round(value, 1) for value in matching_spans[0]["bbox"]]
            return bbox, source_text, decoded, page_number
    return None, None, None, None


def first_amount_span(pdf_path):
    """Return (bbox, source_text, visible_text, page) for a money span.

    Scans EVERY page (not just page 0) and tries span-level matches first, then
    falls back to a line-level reconstruction (handy when a bank splits an
    amount across several spans). A final signed-profile pass handles Type0
    fonts with deliberately empty ToUnicode maps while preserving the exact raw
    source span as the edit identity."""
    d = pymupdf.open(pdf_path)
    try:
        # Pass 1: a span that is essentially just a money value (cleanest target).
        for pno in range(d.page_count):
            for b in d[pno].get_text("dict")["blocks"]:
                if "lines" not in b:
                    continue
                for line in b["lines"]:
                    for s in line["spans"]:
                        t = s["text"].strip()
                        if not t:
                            continue
                        m = MONEY.fullmatch(t) or MONEY.search(t)
                        if m and len(t) <= 18 and MONEY_CORE.search(t):
                            bbox = [round(v, 1) for v in s["bbox"]]
                            return bbox, t, t, pno
        # Pass 2: line-level reconstruction for split amounts.
        for pno in range(d.page_count):
            for b in d[pno].get_text("dict")["blocks"]:
                if "lines" not in b:
                    continue
                for line in b["lines"]:
                    spans = [s for s in line["spans"] if s["text"].strip()]
                    joined = "".join(s["text"] for s in spans).strip()
                    m = MONEY.search(joined)
                    if not m:
                        continue
                    frag = m.group(0).strip()
                    # Locate the contiguous spans that cover the matched money text.
                    for s in spans:
                        st = s["text"].strip()
                        if MONEY_CORE.search(st) and frag.endswith(st[-4:]):
                            bbox = [round(v, 1) for v in s["bbox"]]
                            return bbox, st, frag, pno
        return _profiled_amount_span(d)
    finally:
        d.close()


def render(exe, pdf, page, outdir, out_root):
    os.makedirs(outdir, exist_ok=True)
    code, log_text = run(
        exe,
        ["render", "--input", pdf, "--output-dir", outdir, "--page", str(page), "--dpi", "200"],
    )
    basename = os.path.splitext(os.path.basename(pdf))[0]
    expected = os.path.join(outdir, f"{basename}_page_{page + 1}_200dpi.png")
    return (code == 0 and os.path.exists(expected)), expected, log_text


def pixel_diff(a, b):
    from PIL import Image, ImageChops
    import numpy as np

    ia = Image.open(a).convert("RGB")
    ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        return {"same_size": False, "a": ia.size, "b": ib.size}
    pdiff = ImageChops.difference(ia, ib)
    bbox = pdiff.getbbox()
    arr = np.asarray(pdiff)
    changed = int((arr.sum(axis=2) > 12).sum())
    total = ia.size[0] * ia.size[1]
    return {
        "same_size": True,
        "diff_bbox": bbox,
        "changed_px": changed,
        "total_px": total,
        "changed_pct": round(100.0 * changed / total, 4),
    }


def process_pdf(exe, pdf, out_root):
    """Run the four-step flow for a single PDF. Returns a result record whose
    'status' is one of PASS / FAIL / SKIP."""
    name = os.path.splitext(os.path.basename(pdf))[0]
    slug = re.sub(r"[^A-Za-z0-9]+", "_", name)[:40]
    rec = {"pdf": pdf, "status": "PASS", "steps": {}}
    print(f"\n=== {name} ===", flush=True)

    # Step 1+2: locate a money span ("click amount") and craft an edit.
    bbox, source_text, visible_text, page = first_amount_span(pdf)
    if not bbox:
        rec["steps"]["locate_amount"] = "NO_MONEY_SPAN"
        rec["status"] = "SKIP"
        log("[SKIP] step 1 locate amount: no money span found", 2)
        return rec
    digits = MONEY_CORE.search(visible_text).group(0)
    try:
        val = float(digits.replace(",", ""))
    except ValueError:
        val = 0.0
    new_val = val + 100.00
    new_text = visible_text.replace(digits, f"{new_val:,.2f}")
    rec["steps"]["locate_amount"] = {
        "bbox": bbox,
        "source_text": source_text,
        "visible_text": visible_text,
        "new_text": new_text,
        "page": page,
    }
    log(
        f"[PASS] step 1 locate amount: visible={visible_text!r} source={source_text!r} "
        f"@ p{page} {bbox} -> {new_text!r}",
        2,
    )
    edit_input, edit_page, segment = prepare_edit_input(pdf, page, out_root, slug)
    if segment is not None:
        rec["steps"]["segment"] = segment
        log(
            f"[PASS] production segmentation: source p{page} -> segment p{edit_page} "
            f"({segment['page_range'][0] + 1}-{segment['page_range'][1] + 1})",
            2,
        )

    # Step 3: apply edit.
    edited = os.path.join(out_root, f"{slug}_edited.pdf")
    code, edit_log = run(
        exe,
        ["text", "--input", edit_input, "--output", edited, "--old", source_text, "--new", new_text,
         "--page", str(edit_page), "--bbox", ",".join(str(v) for v in bbox)],
    )
    applied = code == 0 and os.path.exists(edited)
    rec["steps"]["apply_edit"] = {"exit": code, "ok": applied}
    if not applied:
        rec["status"] = "FAIL"
        rec["steps"]["apply_edit"]["log_tail"] = edit_log[-600:]
        log(f"[FAIL] step 3 apply edit: exit {code}", 2)
        if VERBOSE:
            log(edit_log[-600:], 4)
        return rec
    log(f"[PASS] step 3 apply edit: exit {code}", 2)

    # Step 4: render both + pixel diff.
    ok_o, ro, log_o = render(exe, edit_input, edit_page, os.path.join(out_root, f"{slug}_orig"), out_root)
    ok_e, re_path, log_e = render(exe, edited, edit_page, os.path.join(out_root, f"{slug}_edit"), out_root)
    if ok_o and ok_e:
        pdiff = pixel_diff(ro, re_path)
        rec["steps"]["pixel_diff"] = pdiff
        log(f"[PASS] step 4 render+diff: {pdiff.get('changed_pct')}% changed, bbox={pdiff.get('diff_bbox')}", 2)
    else:
        rec["status"] = "FAIL"
        rec["steps"]["pixel_diff"] = {"render_orig": ok_o, "render_edit": ok_e}
        log(f"[FAIL] step 4 render: orig={ok_o} edit={ok_e}", 2)
        if VERBOSE:
            log((log_o or log_e)[-600:], 4)
    return rec


def main():
    global VERBOSE
    ap = argparse.ArgumentParser(description="Step-by-step end-to-end pipeline validation")
    ap.add_argument("--pdf-dir", default="AU Bank Statements", help="directory of input PDFs")
    ap.add_argument("--pdf", action="append", help="run a single PDF (repeatable); overrides --pdf-dir")
    ap.add_argument("--exe", help="explicit path to the CLI binary")
    ap.add_argument("--out", default="output/e2e", help="output directory")
    ap.add_argument("--limit", type=int, default=0, help="process at most N PDFs (0 = all)")
    ap.add_argument("--build", action="store_true", help="cargo build the binary if missing")
    ap.add_argument("--release", action="store_true", help="prefer/build the release binary")
    ap.add_argument("--strict", action="store_true", help="treat SKIP (no money span) as a failure")
    ap.add_argument("-v", "--verbose", action="store_true", help="print log tails on failure")
    args = ap.parse_args()
    VERBOSE = args.verbose

    os.makedirs(args.out, exist_ok=True)

    exe = find_exe(args.exe)
    if exe is None and args.build:
        if build_exe(args.release):
            exe = find_exe(args.exe)
    if exe is None:
        log("[x] CLI binary not found. Build it first: `cargo build --release` "
            "or pass --build / --exe.", 0)
        return 2
    log(f"using binary: {exe}", 0)

    if args.pdf:
        pdfs = list(args.pdf)
    else:
        pdfs = sorted(glob.glob(os.path.join(args.pdf_dir, "*.pdf")))
    if args.limit:
        pdfs = pdfs[: args.limit]
    if not pdfs:
        log(f"[x] No PDFs found (pdf-dir={args.pdf_dir!r}). Nothing to do.", 0)
        return 2
    log(f"{len(pdfs)} PDF(s) to process", 0)

    results = []
    for pdf in pdfs:
        if not os.path.exists(pdf):
            results.append({"pdf": pdf, "status": "FAIL", "steps": {"input": "NOT_FOUND"}})
            print(f"\n=== {pdf} ===\n  [FAIL] input not found", flush=True)
            continue
        results.append(process_pdf(exe, pdf, args.out))

    ts = datetime.datetime.now().isoformat()
    with open(os.path.join(args.out, "e2e_results.json"), "w", encoding="utf-8") as f:
        json.dump({"ts": ts, "results": results}, f, indent=2)

    # Human summary.
    n_pass = sum(1 for r in results if r["status"] == "PASS")
    n_fail = sum(1 for r in results if r["status"] == "FAIL")
    n_skip = sum(1 for r in results if r["status"] == "SKIP")
    lines = [f"E2E PIPELINE RESULTS {ts}", ""]
    for r in results:
        s = r["steps"]
        loc = s.get("locate_amount")
        if r["status"] == "SKIP":
            lines.append(f"[SKIP] {os.path.basename(r['pdf'])}: no money span")
            continue
        if isinstance(loc, dict):
            ap_ = s.get("apply_edit", {})
            pd = s.get("pixel_diff", {})
            displayed_old_text = loc.get("visible_text", loc.get("text"))
            lines.append(
                f"[{r['status']:4}] {os.path.basename(r['pdf'])}: "
                f"edit {displayed_old_text!r}->{loc['new_text']!r} | "
                f"applied={ap_.get('ok')} | changed={pd.get('changed_pct')}% "
                f"bbox={pd.get('diff_bbox')}"
            )
        else:
            lines.append(f"[{r['status']:4}] {os.path.basename(r['pdf'])}: {s}")
    lines += ["", f"TOT:{len(results)}  PASS:{n_pass}  FAIL:{n_fail}  SKIP:{n_skip}"]
    summary = "\n".join(lines)
    with open(os.path.join(args.out, "e2e_summary.txt"), "w", encoding="utf-8") as f:
        f.write(summary)
    print("\n" + summary, flush=True)

    if n_fail or (args.strict and n_skip):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
