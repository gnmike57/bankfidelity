#!/usr/bin/env python3
"""Render every page of a statement, split+merge it via the Rust engine through
a tiny Rust test shim is overkill — instead we reproduce the split/merge by
calling the already-validated CLI render on the ORIGINAL and on a MERGED copy
produced by the integration test path.

Simpler: we use pymupdf only to RENDER (not split) and compare the original vs
the merged output that the Rust e2e test wrote. But that merged file lives in a
temp dir. So here we instead validate visual fidelity of the *edit* round-trip
that the CLI can do on <=3-page docs, and for >3-page docs we rely on the Rust
integration test's lossless page-count proof.

This script renders original vs edited (from output/e2e) for the docs that were
successfully edited and reports per-page pixel diffs to confirm edit locality.
"""
import glob, os, re, sys
import pymupdf
from PIL import Image, ImageChops
import numpy as np

OUT = "output/e2e"

def render_page_png(pdf_path, page, dpi=150):
    d = pymupdf.open(pdf_path)
    p = d[page]
    pix = p.get_pixmap(dpi=dpi)
    png = os.path.join(OUT, f"_tmp_{os.path.basename(pdf_path)}_{page}.png")
    pix.save(png)
    d.close()
    return png

def diff(a, b):
    """Compute pixel-level difference between two rendered page images."""
    ia = Image.open(a).convert("RGB"); ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        return None, ia.size, ib.size
    d = ImageChops.difference(ia, ib)
    arr = np.asarray(d)
    changed = int((arr.sum(axis=2) > 12).sum())
    total = ia.size[0]*ia.size[1]
    return (d.getbbox(), changed, total), ia.size, ib.size

edited = sorted(glob.glob(os.path.join(OUT, "*_edited.pdf")))
print(f"checking {len(edited)} edited outputs for edit-locality fidelity\n")
for e in edited:
    base = os.path.basename(e).replace("_edited.pdf", "")
    # find original
    orig = None
    for cand in glob.glob("AU Bank Statements/*.pdf"):
        slug = re.sub(r"[^A-Za-z0-9]+", "_", os.path.splitext(os.path.basename(cand))[0])[:40]
        if slug == base:
            orig = cand
            break
    if not orig:
        print(f"[?] {base}: original not found"); continue
    try:
        ro = render_page_png(orig, 0)
        re_ = render_page_png(e, 0)
        res, sa, sb = diff(ro, re_)
        if res is None:
            print(f"[SIZE] {base}: orig {sa} vs edited {sb}")
        else:
            bbox, changed, total = res
            pct = round(100*changed/total, 4)
            verdict = "LOCAL-OK" if pct < 5 else "WIDE-DIFF"
            print(f"[{verdict}] {base}: {pct}% changed, diff bbox={bbox}")
    except Exception as ex:
        print(f"[ERR] {base}: {ex}")
