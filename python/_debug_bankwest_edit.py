import importlib.util
import tempfile
from pathlib import Path

import pymupdf

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("bridge", MODULE_PATH)
B = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(B)
B._ensure_pro_unlocked = lambda *a, **k: None

fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
print("exists", fixture.exists())
txs = B.get_all_transactions(str(fixture))
first, second = txs[0], txs[1]
print("first raw", first["raw_text"])
print("first date", first["date"], first["field_bboxes"]["date"])
print("second date", second["date"], second["field_bboxes"]["date"])

with pymupdf.open(fixture) as doc:
    page = doc[0]
    rect = pymupdf.Rect(first["field_bboxes"]["date"])
    matches = B._find_exact_target_spans(page, rect, first["date"])
    print("matches", len(matches), [m.get("text") for m in matches])
    if matches:
        m = matches[0]
        print(
            "span bbox",
            m.get("bbox"),
            "origin",
            m.get("origin"),
            "font",
            m.get("font"),
            "size",
            m.get("size"),
        )
    clip = pymupdf.Rect(rect)
    clip.x0 -= 5
    clip.y0 -= 5
    clip.x1 += 40
    clip.y1 += 5
    print("clip text before:", repr(page.get_text("text", clip=clip)))
    # dump spans overlapping date rect expanded
    for block in page.get_text("dict")["blocks"]:
        if "lines" not in block:
            continue
        for line in block["lines"]:
            for span in line.get("spans", []):
                sb = pymupdf.Rect(span["bbox"])
                if sb.intersects(clip):
                    print(
                        "near span",
                        repr(span.get("text")),
                        span.get("bbox"),
                        span.get("font"),
                    )

with tempfile.TemporaryDirectory() as d:
    out = Path(d) / "out.pdf"
    report = B.apply_many_edits(
        str(fixture),
        str(out),
        [
            {
                "page": 0,
                "rect": first["field_bboxes"]["date"],
                "old_text": first["date"],
                "new_text": "31 JUL",
            }
        ],
    )
    print("report keys", {k: report[k] for k in report if k != "edits"})
    print("edit0", report["edits"][0] if report.get("edits") else None)
    # Force inspect intermediate if published
    if out.exists():
        with pymupdf.open(out) as doc:
            page = doc[0]
            clip = pymupdf.Rect(first["field_bboxes"]["date"])
            clip.x0 -= 5
            clip.y0 -= 5
            clip.x1 += 40
            clip.y1 += 5
            print("clip text after:", repr(page.get_text("text", clip=clip)))
    else:
        print("no output published")
        # Try to understand placement: re-run match + placement manually
        with pymupdf.open(fixture) as doc:
            page = doc[0]
            rect = pymupdf.Rect(first["field_bboxes"]["date"])
            matches = B._find_exact_target_spans(page, rect, first["date"])
            span = matches[0]
            emit = B._fallback_standard14(span.get("font") or "helv")
            print("emit font", emit)
            try:
                measure = pymupdf.Font(fontname=emit)
            except Exception as e:
                measure = None
                print("measure fail", e)
            placement = B._placement_for_edit(
                page,
                rect,
                span,
                "31 JUL",
                emit,
                float(span.get("size") or 10),
                supplied_font=measure,
                measured_width=None,
            )
            print("placement", placement)
            print(
                "verify would see",
                repr(
                    page.get_text(
                        "text",
                        clip=pymupdf.Rect(
                            max(0, placement["redact_rect"].x0 - 3),
                            max(0, placement["redact_rect"].y0 - 3),
                            placement["redact_rect"].x1 + 6,
                            placement["redact_rect"].y1 + 6,
                        ),
                    )
                ),
            )
