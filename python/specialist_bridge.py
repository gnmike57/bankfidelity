"""
BankFidelity specialist IPC bridge (JSON lines over stdin/stdout).

Operations:
  regress_subpixel_bbox        exact vector search on PDFs; optional Florence-2 on images
  detect_layout_and_order      optional Surya reading-order / line polygons
  identify_embedded_fonts      real font names + metrics read from the PDF's font programs
  match_font_contour           raster glyph-crop font ID (not implemented -> "unavailable")
  subpixel_differential_nudge  projection-profile drift between proof and donor renders

Contract: every reply carries ``status`` = "ok" | "unavailable" | "error".
A missing model, missing dependency or bad input is NEVER reported as a
passing result: callers must treat anything other than "ok" as no evidence.
Optional ML backends are opt-in via BF_SPECIALIST_FLORENCE=1 / BF_SPECIALIST_SURYA=1.

Model cache root: $BF_MODELS_DIR (default: <repo>/models).
"""

from __future__ import annotations

import json
import os
import sys
import traceback
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent
MODELS_ROOT = Path(os.environ.get("BF_MODELS_DIR") or REPO_ROOT / "models")
DPI = 300.0
PT_PER_PX = 72.0 / DPI
# Maximum tolerated projection drift, in PDF points.
MAX_DRIFT_PT = 0.05

_FLORENCE: Optional[Tuple[Any, Any]] = None
_FLORENCE_ERROR: Optional[str] = None


def _ok(**fields: Any) -> Dict[str, Any]:
    return {"status": "ok", **fields}


def _unavailable(reason: str, **fields: Any) -> Dict[str, Any]:
    return {"status": "unavailable", "reason": reason, **fields}


def _error(reason: str, **fields: Any) -> Dict[str, Any]:
    return {"status": "error", "reason": reason, **fields}


def _enabled(flag: str) -> bool:
    return os.environ.get(flag, "").strip().lower() in {"1", "true", "yes"}


def _load_florence() -> Tuple[Optional[Tuple[Any, Any]], Optional[str]]:
    global _FLORENCE, _FLORENCE_ERROR
    if _FLORENCE is not None or _FLORENCE_ERROR is not None:
        return _FLORENCE, _FLORENCE_ERROR
    try:
        from transformers import AutoModelForCausalLM, AutoProcessor

        model_id = "microsoft/Florence-2-large"
        cache_dir = str(MODELS_ROOT / "florence2")
        processor = AutoProcessor.from_pretrained(
            model_id, trust_remote_code=True, cache_dir=cache_dir
        )
        model = AutoModelForCausalLM.from_pretrained(
            model_id, trust_remote_code=True, cache_dir=cache_dir
        ).eval()
        _FLORENCE = (model, processor)
    except (ImportError, OSError, ValueError, RuntimeError) as exc:
        _FLORENCE_ERROR = f"{type(exc).__name__}: {exc}"
    return _FLORENCE, _FLORENCE_ERROR


def regress_subpixel_bbox(path_str: str, query: str) -> Dict[str, Any]:
    path = Path(path_str)
    if not path.is_file():
        return _error(f"file not found: {path_str}")
    if not query:
        return _error("empty query")

    if path.suffix.lower() == ".pdf":
        try:
            import pymupdf as fitz
        except ImportError:
            import fitz  # type: ignore[no-redef]
        with fitz.open(path) as doc:
            if doc.page_count == 0:
                return _error("PDF has no pages")
            rects = doc[0].search_for(query)
        if not rects:
            return _unavailable("query not found in the PDF text layer")
        r = rects[0]
        # Exact match against the vector text layer: no estimation involved.
        return _ok(
            bbox=[r.x0, r.y0, r.x1, r.y1],
            engine="pymupdf_vector",
            matches=len(rects),
        )

    if not _enabled("BF_SPECIALIST_FLORENCE"):
        return _unavailable("image grounding disabled (set BF_SPECIALIST_FLORENCE=1)")
    loaded, err = _load_florence()
    if loaded is None:
        return _unavailable(f"Florence-2 could not load: {err}")
    model, processor = loaded
    try:
        import torch
        from PIL import Image

        img = Image.open(path).convert("RGB")
        prompt = f"<CAPTION_TO_PHRASE_GROUNDING> {query}"
        inputs = processor(text=prompt, images=img, return_tensors="pt")
        with torch.no_grad():
            ids = model.generate(
                input_ids=inputs["input_ids"],
                pixel_values=inputs["pixel_values"],
                max_new_tokens=1024,
                num_beams=3,
            )
        text = processor.batch_decode(ids, skip_special_tokens=False)[0]
        parsed = processor.post_process_generation(
            text, task="<CAPTION_TO_PHRASE_GROUNDING>", image_size=(img.width, img.height)
        )
    except (ImportError, OSError, ValueError, RuntimeError) as exc:
        return _error(f"Florence-2 inference failed: {type(exc).__name__}: {exc}")
    boxes = parsed.get("<CAPTION_TO_PHRASE_GROUNDING>", {}).get("bboxes", [])
    if not boxes:
        return _unavailable("Florence-2 found no box for the query")
    # Florence-2 exposes no calibrated confidence; report none rather than invent one.
    return _ok(bbox=boxes[0], engine="florence-2-large", confidence=None)


def detect_layout_and_order(path_str: str) -> Dict[str, Any]:
    path = Path(path_str)
    if not path.is_file():
        return _error(f"file not found: {path_str}")
    if not _enabled("BF_SPECIALIST_SURYA"):
        return _unavailable("layout ordering disabled (set BF_SPECIALIST_SURYA=1)")
    try:
        from PIL import Image
        from surya.model.ordering.model import load_model
        from surya.model.ordering.processor import load_processor
        from surya.ordering import batch_ordering
    except ImportError as exc:
        return _unavailable(
            "installed surya does not expose the legacy ordering API this bridge "
            f"targets ({exc}); port to the installed surya release before enabling"
        )
    try:
        img = Image.open(path)
        results = batch_ordering([img], [[]], load_model(), load_processor())
    except (OSError, ValueError, RuntimeError) as exc:
        return _error(f"surya failed: {type(exc).__name__}: {exc}")
    lines = [
        {"index": i, "bbox": item.bbox, "polygon": item.polygon}
        for i, item in enumerate(results[0].bboxes if results else [])
    ]
    return _ok(lines=lines, engine="surya_layout")


def identify_embedded_fonts(pdf_path: str, page_index: int = 0) -> Dict[str, Any]:
    """Read real font names and vertical metrics from the PDF's font programs."""
    path = Path(pdf_path)
    if not path.is_file():
        return _error(f"file not found: {pdf_path}")
    try:
        import pymupdf as fitz
    except ImportError:
        try:
            import fitz  # type: ignore[no-redef]
        except ImportError as exc:
            return _unavailable(f"PyMuPDF not installed: {exc}")
    try:
        from fontTools.ttLib import TTFont, TTLibError
    except ImportError as exc:
        return _unavailable(f"fontTools not installed: {exc}")

    import io

    fonts = []
    with fitz.open(path) as doc:
        if not 0 <= page_index < doc.page_count:
            return _error(f"page {page_index} out of range (0..{doc.page_count - 1})")
        for xref, ext, kind, basefont, *_ in doc[page_index].get_fonts(full=True):
            entry: Dict[str, Any] = {
                "xref": xref,
                "basefont": basefont,
                "type": kind,
                "format": ext,
                "metrics": None,
            }
            if ext in {"ttf", "otf", "ttc"}:
                try:
                    _, _, _, buf = doc.extract_font(xref)
                    tt = TTFont(io.BytesIO(buf), lazy=True)
                    upem = tt["head"].unitsPerEm
                    os2 = tt["OS/2"] if "OS/2" in tt else None
                    hhea = tt["hhea"]
                    entry["metrics"] = {
                        "units_per_em": upem,
                        "ascender": hhea.ascent / upem,
                        "descender": hhea.descent / upem,
                        "x_height": (getattr(os2, "sxHeight", 0) or 0) / upem or None,
                        "cap_height": (getattr(os2, "sCapHeight", 0) or 0) / upem or None,
                    }
                except (TTLibError, KeyError, ValueError, AttributeError) as exc:
                    entry["metrics_error"] = f"{type(exc).__name__}: {exc}"
            fonts.append(entry)
    return _ok(fonts=fonts, engine="pymupdf+fonttools")


def match_font_contour(_glyph_crop_path: str) -> Dict[str, Any]:
    return _unavailable(
        "raster glyph-crop font identification is not implemented; "
        "use identify_embedded_fonts for PDFs with embedded font programs"
    )


def _profile_shift(a: Any, b: Any) -> float:
    """Sub-pixel lag of ``a`` relative to ``b`` via correlation + parabolic peak fit."""
    import numpy as np

    a = a - a.mean()
    b = b - b.mean()
    corr = np.correlate(a, b, mode="full")
    k = int(np.argmax(corr))
    lag = k - (len(b) - 1)
    if 0 < k < len(corr) - 1:
        y0, y1, y2 = corr[k - 1], corr[k], corr[k + 1]
        denom = y0 - 2 * y1 + y2
        if denom != 0:
            return float(lag + 0.5 * (y0 - y2) / denom)
    return float(lag)


def subpixel_differential_nudge(proof_png: str, donor_png: str) -> Dict[str, Any]:
    try:
        import cv2
        import numpy as np
    except ImportError as exc:
        return _unavailable(f"opencv/numpy not installed: {exc}")
    proof = cv2.imread(proof_png, cv2.IMREAD_GRAYSCALE)
    donor = cv2.imread(donor_png, cv2.IMREAD_GRAYSCALE)
    if proof is None or donor is None:
        return _error("could not read proof/donor image", approved=False)
    if proof.shape != donor.shape:
        return _error(
            f"image size mismatch: proof {proof.shape} vs donor {donor.shape}",
            approved=False,
        )
    ink_p = 255.0 - proof.astype("float64")
    ink_d = 255.0 - donor.astype("float64")
    if ink_p.sum() == 0 or ink_d.sum() == 0:
        return _error("blank image: no ink to correlate", approved=False)
    dy_px = _profile_shift(ink_p.sum(axis=1), ink_d.sum(axis=1))
    dx_px = _profile_shift(ink_p.sum(axis=0), ink_d.sum(axis=0))
    dy_pt, dx_pt = dy_px * PT_PER_PX, dx_px * PT_PER_PX
    drift = max(abs(dy_pt), abs(dx_pt))
    return _ok(
        approved=bool(drift <= MAX_DRIFT_PT),
        delta_x=dx_pt,
        delta_y=dy_pt,
        max_drift_pt=drift,
        assumed_dpi=DPI,
        engine="projection_profile_correlator",
    )


def dispatch_request(req: Dict[str, Any]) -> Dict[str, Any]:
    op = req.get("op", "")
    if op == "regress_subpixel_bbox":
        return regress_subpixel_bbox(req.get("image_path", ""), req.get("query", ""))
    if op == "detect_layout_and_order":
        return detect_layout_and_order(req.get("image_path", ""))
    if op == "identify_embedded_fonts":
        return identify_embedded_fonts(req.get("pdf_path", ""), int(req.get("page", 0)))
    if op == "match_font_contour":
        return match_font_contour(req.get("glyph_crop_path", ""))
    if op == "subpixel_differential_nudge":
        return subpixel_differential_nudge(req.get("proof_png", ""), req.get("donor_png", ""))
    return _error(f"unknown operation: {op}")


def main() -> int:
    if len(sys.argv) > 1 and sys.argv[1] == "--interactive":
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                res = dispatch_request(json.loads(line))
            except (json.JSONDecodeError, TypeError, ValueError) as exc:
                res = _error(f"bad request: {type(exc).__name__}: {exc}")
            except Exception as exc:  # noqa: BLE001 - IPC boundary: report, never die
                res = _error(
                    f"unhandled {type(exc).__name__}: {exc}",
                    traceback=traceback.format_exc(),
                )
            sys.stdout.write(json.dumps(res) + "\n")
            sys.stdout.flush()
        return 0
    print("[SPECIALIST BRIDGE] Ready. Run with --interactive for IPC mode.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
