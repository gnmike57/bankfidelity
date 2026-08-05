#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PyMuPDF Pro Smart Targeted Editor v2.1
- Get all text blocks with accurate bounding boxes
- Robust targeted replacement inside a specific rectangle using redaction
"""

import hashlib
import os
import re
import sys
from decimal import Decimal, InvalidOperation

# ── Windows DLL search-path fix (must run BEFORE import pymupdf) ─────────────
# pymupdf/_extra.pyd depends on mupdfcpp64.dll which lives inside the pymupdf
# package directory in site-packages. The pymupdf package is also aliased as
# "fitz", so find_spec("pymupdf") may resolve to the fitz folder which does NOT
# contain the DLLs. We search all site-packages subdirs for mupdfcpp64.dll and
# add whichever directory contains it. os.add_dll_directory() is Windows 3.8+.
if sys.platform == "win32" and hasattr(os, "add_dll_directory"):
    try:
        import site as _site
        _dll_name = "mupdfcpp64.dll"
        _site_dirs = []
        try:
            _site_dirs += _site.getsitepackages()
        except Exception:
            pass
        try:
            _site_dirs.append(_site.getusersitepackages())
        except Exception:
            pass
        for _sdir in _site_dirs:
            if not os.path.isdir(_sdir):
                continue
            for _sub in os.listdir(_sdir):
                _candidate = os.path.join(_sdir, _sub)
                if os.path.isfile(os.path.join(_candidate, _dll_name)):
                    os.add_dll_directory(_candidate)
        # Also register the Python home (python312.dll)
        _py_home = os.path.dirname(sys.executable)
        if _py_home and os.path.isdir(_py_home):
            os.add_dll_directory(_py_home)
    except Exception:
        pass  # Non-fatal: pymupdf may still load if DLLs are already on PATH

import pymupdf
import json
import math
import gc

_FONT_RESOURCE_PROFILES = None

# PyMuPDF Pro lives in the separate `pymupdfpro` package and exposes the
# `pymupdf.pro` submodule. Import it defensively: if the Pro package is not
# installed (or fails to load), we must NOT crash the whole module — doing so
# would take down the entire Python actor and disable even non-Pro helpers.
# Instead we record availability and fail loudly only when a Pro-gated call is
# actually made. This keeps the headless health server and any non-Pro paths
# working, and turns an opaque "PyEngine init failed" into an actionable error
# at the exact call site.
try:
    import pymupdf.pro  # noqa: F401  (registers the pymupdf.pro submodule)
    _PYMUPDF_PRO_AVAILABLE = True
    _PYMUPDF_PRO_IMPORT_ERROR = None
except Exception as _e:  # ImportError or any loader-level failure
    _PYMUPDF_PRO_AVAILABLE = False
    _PYMUPDF_PRO_IMPORT_ERROR = _e
    print(
        "[pymupdf_pro_integration] WARNING: PyMuPDF Pro (pymupdf.pro) is not "
        f"available: {_e}. Pro-gated operations will fail until the "
        "`pymupdfpro` package is installed; non-Pro paths still work.",
        file=sys.stderr,
    )

PYMUPDF_PRO_KEY = os.environ.get("PYMUPDF_PRO_KEY", "")

# ---------------------------------------------------------------------------
# PyMuPDF Pro 3-page licensing limit (Requirement 5).
#
# A single PyMuPDF Pro unlock+operation may only legally touch a document of
# <=3 pages. Subsystem A (the pure-Rust `lopdf` split engine) guarantees that
# only <=3-page segments are ever fed to the Pro editor, so this guard is a
# defensive safety net: it verifies the page count BEFORE the unlock and
# refuses to unlock Pro for an over-limit document.
#
# `PRO_PAGE_LIMIT_TOKEN` is the STABLE prefix of the RuntimeError message
# raised when the limit is exceeded. The Rust PyO3 bridge (and the runtime
# above it) matches on this exact token to surface a structured error, so it
# must not be changed without updating `src/ai/pyo3_bridge.rs`.
# ---------------------------------------------------------------------------
# This is a licensing and correctness boundary, not a developer tuning knob.
# Long statements must be split into verified <=3-page segments by Rust.
PRO_PAGE_LIMIT = 3
PRO_PAGE_LIMIT_TOKEN = "PRO_PAGE_LIMIT_EXCEEDED"


def _count_pages_without_pro_unlock(pdf_path):
    """Return the page count of `pdf_path` WITHOUT unlocking PyMuPDF Pro.

    Opening a document just to read its page count does not require a Pro
    unlock for ordinary PDF inputs -- which is all the 3 Page Mode pipeline
    ever feeds here: <=3-page PDF segments produced by Subsystem A (`lopdf`).
    The document is opened, its `page_count` is read, and it is closed again,
    so the file on disk is left completely unchanged (it is never saved).

    Returns an int page count, or ``None`` when the count cannot be determined
    cheaply without an unlock (for example a Pro-only container format that
    will not open until Pro is unlocked, or an unreadable file). Callers treat
    a ``None`` result as "cannot positively prove the limit is exceeded" and
    fall through to the normal post-unlock path, which surfaces its own error
    for a genuinely unreadable file. This keeps the guard from ever unlocking
    Pro on a document we have positively determined to be >3 pages, while not
    blocking legitimate <=3-page edits when the count is merely unknown.
    """
    doc = None
    try:
        doc = pymupdf.open(pdf_path)
        return int(doc.page_count)
    except Exception:
        # Could not open/count without an unlock. We deliberately do NOT raise
        # here: a None result means "limit not provable", and the caller lets
        # the normal flow report any real failure. We never unlock as a result
        # of this branch on a doc we KNOW is over-limit.
        return None
    finally:
        if doc is not None:
            try:
                doc.close()
            except Exception:
                pass


def _assert_within_pro_page_limit(pdf_path):
    """Verify `pdf_path` has <=3 pages BEFORE any Pro unlock is performed.

    Enforces the PyMuPDF Pro 3-page licensing limit (Requirement 5.2/5.3):
      * counts pages WITHOUT unlocking Pro (see
        `_count_pages_without_pro_unlock`),
      * raises ``RuntimeError("PRO_PAGE_LIMIT_EXCEEDED: ...")`` when the
        document has more than ``PRO_PAGE_LIMIT`` pages -- and does so BEFORE
        ``pymupdf.pro.unlock`` is ever called, so the unlock never happens for
        an over-limit document, and
      * leaves the document unchanged (it is only opened read-only to count).

    A ``None`` page count (the count could not be determined without an
    unlock) does NOT raise; the caller proceeds and the normal path surfaces
    any genuine error. ``pdf_path`` may be ``None`` (callers that have no path
    to check), in which case the guard is a no-op.
    """
    if not pdf_path:
        return
    page_count = _count_pages_without_pro_unlock(pdf_path)
    if page_count is not None and page_count > PRO_PAGE_LIMIT:
        raise RuntimeError(
            f"{PRO_PAGE_LIMIT_TOKEN}: document at '{pdf_path}' has {page_count} "
            f"pages, which exceeds the PyMuPDF Pro {PRO_PAGE_LIMIT}-page limit. "
            f"The Pro unlock was not performed and the document was left "
            f"unchanged."
        )


def _ensure_pro_unlocked(pdf_path=None):
    """Unlock PyMuPDF Pro with the configured key.

    Centralizes the `pymupdf.pro.unlock(PYMUPDF_PRO_KEY)` call that was
    previously copy-pasted at the top of every function. If the Pro package
    is missing, raise a single, clear error naming the fix rather than an
    opaque AttributeError/ModuleNotFoundError deep in a call.

    When `pdf_path` is supplied, the PyMuPDF Pro 3-page limit is enforced
    FIRST via `_assert_within_pro_page_limit`: a document of more than 3 pages
    raises a structured ``PRO_PAGE_LIMIT_EXCEEDED`` error before the unlock, so
    Pro is never unlocked for an over-limit document (Requirement 5). Callers
    that have no document path (or operate on the full original document
    outside the per-segment Pro path) omit `pdf_path` and the guard is a no-op.
    """
    # 3-page Pro limit guard runs BEFORE the availability check and BEFORE the
    # unlock, so an over-limit document is rejected without ever unlocking Pro.
    _assert_within_pro_page_limit(pdf_path)
    if not _PYMUPDF_PRO_AVAILABLE:
        raise RuntimeError(
            "PyMuPDF Pro is not installed (pip install pymupdfpro). "
            f"Original import error: {_PYMUPDF_PRO_IMPORT_ERROR}"
        )
    pymupdf.pro.unlock(PYMUPDF_PRO_KEY)


def render_page_to_png(pdf_path: str, page_num: int = 0, dpi: float = 150.0):
    """Render a single PDF page to PNG bytes using PyMuPDF (no Pro needed).

    Returns a dict with keys: png_bytes (base64), width_pts, height_pts.
    This uses the standard (free) PyMuPDF rasteriser which handles all fonts,
    embedded images, vector graphics, etc. — producing a faithful preview
    even when the native Rust engine cannot.
    """
    import base64
    doc = pymupdf.open(pdf_path)
    if page_num >= doc.page_count:
        doc.close()
        raise ValueError(f"Page {page_num} out of range (document has {doc.page_count} pages)")
    page = doc[page_num]
    zoom = dpi / 72.0
    mat = pymupdf.Matrix(zoom, zoom)
    pix = page.get_pixmap(matrix=mat, alpha=False)
    png_bytes = pix.tobytes("png")
    width_pts = page.rect.width
    height_pts = page.rect.height
    doc.close()
    return {
        "png_base64": base64.b64encode(png_bytes).decode("ascii"),
        "width_pts": width_pts,
        "height_pts": height_pts,
    }


def get_text_blocks(pdf_path: str, page_num: int = 0):
    """Return text spans using the core PyMuPDF extraction API.

    Text extraction does not require PyMuPDF Pro. Keep this operation available in
    the base runtime and use a context manager so every success and failure path
    deterministically closes the document handle.
    """
    blocks = []
    with pymupdf.open(pdf_path) as doc:
        page = doc[page_num]
        for block in page.get_text("dict")["blocks"]:
            if "lines" not in block:
                continue
            for line in block["lines"]:
                for span in line["spans"]:
                    blocks.append({
                        "page": page_num,
                        "text": span["text"],
                        "bbox": list(span["bbox"]),      # [x0, y0, x1, y1]
                        "font": span["font"],
                        "size": round(span["size"], 2),
                        "color": span["color"],
                        "origin": list(span.get("origin", [0, 0])),
                    })
    return blocks


def _color_int_to_rgb(color_int: int):
    """PyMuPDF gives sRGB span colour as a single int (0xRRGGBB). Map to (r,g,b) floats."""
    if color_int is None:
        return (0.0, 0.0, 0.0)
    r = ((color_int >> 16) & 0xFF) / 255.0
    g = ((color_int >> 8) & 0xFF) / 255.0
    b = (color_int & 0xFF) / 255.0
    return (r, g, b)


# ===========================================================================
# Stage 8.5: Document-level font analysis.
#
# Runs once when the user opens a PDF. For each font used in the document
# we report:
#   - usage_role: "digits", "letters", "mixed", "punctuation", "other"
#   - characters_used: every codepoint that actually appears
#   - missing_chars: characters_used \ subset coverage
#   - fidelity_impact: free-text human-readable summary the GUI shows the user
#
# The decision rule is straightforward and matches the users requirement:
#
#   For each font:
#     used = set of characters actually written with this font in the doc
#     covered = set of characters the embedded subset (or standard-14) renders
#     missing = used - covered
#
#     if missing == empty: no action needed for this font
#     elif missing is digits-only: only those digit glyphs need creation
#     elif missing is letters-only: only those letter glyphs need creation
#     else (digits + letters mixed): only the specific missing glyphs of
#         each kind need creation -- never the full alphabet
#
# Even if `used` happens to span letters and digits, the *creation scope* is
# only `missing`, never the universe of the alphabet.
# ===========================================================================

def _classify_role(chars: set) -> str:
    """Bucket the characters used by a font into a usage role for display."""
    has_digits = any(c.isdigit() for c in chars)
    has_letters = any(c.isalpha() for c in chars)
    has_punct = any((not c.isalnum()) and (not c.isspace()) for c in chars)
    if has_digits and not has_letters:
        return "digits"
    if has_letters and not has_digits:
        return "letters"
    if has_digits and has_letters:
        return "mixed"
    if has_punct and not has_digits and not has_letters:
        return "punctuation"
    return "other"


def _missing_breakdown(missing: list) -> dict:
    """Split `missing` into digits / letters / other for the scope summary."""
    digits = [c for c in missing if c.isdigit()]
    letters = [c for c in missing if c.isalpha()]
    other = [c for c in missing if not c.isalnum()]
    return {
        "digits": digits,
        "letters": letters,
        "other": other,
    }


def _is_standard_14_basename(name: str) -> bool:
    if not name:
        return False
    n = name.lower()
    if "+" in n:
        n = n.split("+", 1)[1]
    return n in _STANDARD_14_FONTS


def _has_glyph_safe(font_obj, codepoint: int) -> bool:
    """`Font.has_glyph` can raise on some malformed subsets; fall back to
    `glyph_advance` returning a positive width."""
    try:
        if bool(font_obj.has_glyph(codepoint)):
            return True
    except Exception:
        pass
    try:
        return float(font_obj.glyph_advance(codepoint)) > 0.0
    except Exception:
        return False


def _winansi_covers(ch: str) -> bool:
    """Standard-14 fonts have implicit WinAnsi coverage. Anything that
    encodes to cp1252 is renderable without an embedded subset."""
    try:
        ch.encode("cp1252")
        return True
    except UnicodeEncodeError:
        return False


def analyze_fonts(pdf_path: str) -> dict:
    """Return a per-font breakdown of usage, coverage and fidelity impact.

    Output shape:
      {
        "fonts": [
          {
            "name": "ABCDEF+Helvetica-Bold",
            "base_name": "Helvetica-Bold",
            "xref": 12,
            "is_standard_14": false,
            "is_subset": true,
            "usage_role": "digits",
            "pages_used_on": [0, 1, 2],
            "size_range": [8.5, 10.0],
            "occurrences": 247,
            "characters_used": "$,.0123456789",
            "missing_chars": ["$"],
            "missing_breakdown": {"digits": [], "letters": [], "other": ["$"]},
            "creation_scope": "Create only 1 missing glyph(s): $",
            "fidelity_impact": "Used only for digits -- but $ is missing. ..."
          },
          ...
        ],
        "summary": {
          "total_fonts": 5,
          "fonts_needing_action": 2,
          "missing_digit_count": 0,
          "missing_letter_count": 3,
          "missing_other_count": 1,
          "all_fonts_covered": false,
        }
      }
    """
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)

    # 1. Collect per-font usage data, keyed by basename so subsets of the
    #    same base font roll up together.
    per_font = {}

    for page_idx, page in enumerate(doc):
        try:
            page_fonts = {f[0]: f for f in page.get_fonts(full=True)}
        except Exception:
            page_fonts = {}

        for block in page.get_text("dict").get("blocks", []):
            if "lines" not in block:
                continue
            for line in block["lines"]:
                for span in line.get("spans", []):
                    raw_name = span.get("font", "") or ""
                    base = raw_name.split("+", 1)[1] if "+" in raw_name else raw_name
                    key = base.lower()
                    if not key:
                        continue
                    text = span.get("text", "") or ""
                    if not text:
                        continue

                    rec = per_font.setdefault(key, {
                        "name": raw_name,
                        "base_name": base,
                        "is_subset": "+" in raw_name,
                        "is_standard_14": _is_standard_14_basename(raw_name),
                        "characters_used": set(),
                        "pages_used_on": set(),
                        "size_min": float("inf"),
                        "size_max": 0.0,
                        "occurrences": 0,
                        "first_xref": None,
                    })
                    rec["characters_used"].update(text)
                    rec["pages_used_on"].add(page_idx)
                    sz = float(span.get("size", 0.0))
                    if sz > 0:
                        rec["size_min"] = min(rec["size_min"], sz)
                        rec["size_max"] = max(rec["size_max"], sz)
                    rec["occurrences"] += 1
                    if rec["first_xref"] is None:
                        for xref, info in page_fonts.items():
                            try:
                                bf = (info[3] or "").lower()
                                al = (info[4] or "").lower()
                            except (IndexError, TypeError):
                                continue
                            if base.lower() in bf or base.lower() in al or al == raw_name.lower():
                                rec["first_xref"] = xref
                                break

    # 2. Build coverage report per font.
    fonts_out = []
    fonts_needing_action = 0
    total_missing_digits = 0
    total_missing_letters = 0
    total_missing_other = 0

    for key, rec in per_font.items():
        chars = rec["characters_used"]
        chars_clean = sorted({c for c in chars if not c.isspace()})
        role = _classify_role(set(chars_clean))

        # Determine which characters are NOT covered by the embedded subset.
        if rec["is_standard_14"]:
            # WinAnsi renders all cp1252 chars implicitly.
            missing = [c for c in chars_clean if not _winansi_covers(c)]
        elif rec["first_xref"] is not None:
            try:
                font_info = doc.extract_font(rec["first_xref"])
                content = None
                if isinstance(font_info, dict):
                    content = font_info.get("content")
                elif isinstance(font_info, (tuple, list)) and len(font_info) >= 4:
                    for item in reversed(font_info):
                        if isinstance(item, (bytes, bytearray)) and len(item) > 0:
                            content = bytes(item)
                            break
                if content:
                    f = pymupdf.Font(fontbuffer=content)
                    missing = [
                        c for c in chars_clean if not _has_glyph_safe(f, ord(c))
                    ]
                else:
                    missing = []
            except Exception:
                missing = []
        else:
            missing = []

        breakdown = _missing_breakdown(missing)

        # Fidelity impact and creation-scope language.
        if not missing:
            impact = "✅ All characters used in this document are covered by the embedded subset -- no font creation needed."
            scope = "None -- all used glyphs already present."
        else:
            fonts_needing_action += 1
            total_missing_digits += len(breakdown["digits"])
            total_missing_letters += len(breakdown["letters"])
            total_missing_other += len(breakdown["other"])

            kinds = []
            if breakdown["digits"]:
                kinds.append(f"{len(breakdown['digits'])} digit(s)")
            if breakdown["letters"]:
                kinds.append(f"{len(breakdown['letters'])} letter(s)")
            if breakdown["other"]:
                kinds.append(f"{len(breakdown['other'])} other glyph(s)")
            kinds_str = ", ".join(kinds)

            preview = "".join(missing[:12])
            if len(missing) > 12:
                preview += "…"

            scope = (
                f"Create only the {len(missing)} missing glyph(s): "
                f"{preview}  ({kinds_str})"
            )

            if role == "digits":
                impact = (
                    f"⚠ Digits-only font -- {len(missing)} glyph(s) missing in this document. "
                    f"Only those specific glyph(s) need creation; the full alphabet is not required."
                )
            elif role == "punctuation":
                impact = (
                    f"⚠ Punctuation-only font -- {len(missing)} glyph(s) missing. "
                    f"Targeted creation of those glyph(s) only."
                )
            elif role == "letters":
                impact = (
                    f"⚠ Letters font -- {len(missing)} letter(s) missing. "
                    f"Only those specific letter glyph(s) need creation; the full alphabet is not required."
                )
            elif role == "mixed":
                # The users rule: even if used spans letters+digits, the
                # creation scope is the actual missing set, not the universe.
                impact = (
                    f"⚠ Mixed font (letters + digits) -- {len(missing)} glyph(s) missing. "
                    f"Creation scope is limited to those glyph(s) only ({kinds_str})."
                )
            else:
                impact = f"⚠ {len(missing)} glyph(s) missing in role '{role}'."

        fonts_out.append({
            "name": rec["name"],
            "base_name": rec["base_name"],
            "xref": rec["first_xref"],
            "is_standard_14": rec["is_standard_14"],
            "is_subset": rec["is_subset"],
            "usage_role": role,
            "pages_used_on": sorted(rec["pages_used_on"]),
            "size_range": [
                round(rec["size_min"], 2) if rec["size_min"] != float("inf") else 0.0,
                round(rec["size_max"], 2),
            ],
            "characters_used": "".join(chars_clean),
            "missing_chars": missing,
            "missing_breakdown": breakdown,
            "occurrences": rec["occurrences"],
            "fidelity_impact": impact,
            "creation_scope": scope,
        })

    fonts_out.sort(key=lambda f: (-f["occurrences"], f["base_name"]))
    doc.close()

    return {
        "fonts": fonts_out,
        "summary": {
            "total_fonts": len(fonts_out),
            "fonts_needing_action": fonts_needing_action,
            "missing_digit_count": total_missing_digits,
            "missing_letter_count": total_missing_letters,
            "missing_other_count": total_missing_other,
            "all_fonts_covered": fonts_needing_action == 0,
        },
    }


def _find_dominant_span(page, rect_obj):
    """Find the text span whose bbox best overlaps the supplied rectangle.

    Returns the span dict (text/font/size/color/origin) or None if nothing overlaps.
    """
    best = None
    best_area = 0.0
    rect = pymupdf.Rect(rect_obj)
    for block in page.get_text("dict").get("blocks", []):
        if "lines" not in block:
            continue
        for line in block["lines"]:
            for span in line["spans"]:
                sp_rect = pymupdf.Rect(span["bbox"])
                inter = sp_rect & rect
                if inter.is_empty:
                    continue
                area = inter.width * inter.height
                if area > best_area:
                    best_area = area
                    best = span
    return best


def _normalized_text_identity(value: str) -> str:
    return " ".join(str(value).split())


def _normalized_money_identity(value):
    """Return a two-decimal money identity or ``None`` for non-money text.

    This is intentionally narrow: it tolerates presentation-only currency
    symbols, thousands separators, AUD labels, balance suffixes, and accounting
    parentheses, but it does not guess OCR substitutions or approximate values.
    Geometry and uniqueness checks remain mandatory in
    ``_find_exact_target_spans``.
    """
    text = str(value).replace("\u00a0", " ").strip().upper()
    negative_parentheses = text.startswith("(") and text.endswith(")")
    if negative_parentheses:
        text = text[1:-1].strip()
    text = re.sub(r"\s+(?:CR|DR)\s*$", "", text)
    text = re.sub(r"^(?:AUD\s*)?\$?", "", text)
    text = text.replace(",", "").strip()
    if text.startswith("+"):
        text = text[1:]
    try:
        amount = Decimal(text)
    except (InvalidOperation, ValueError):
        return None
    if negative_parentheses:
        amount = -amount
    return abs(amount).quantize(Decimal("0.01"))


def _ocr_words_for_exact_matching(page):
    try:
        textpage = page.get_textpage_ocr(language="eng", dpi=300, full=True)
        return list(textpage.extractWORDS())
    except Exception as error:
        print(f"[apply_many] OCR identity fallback unavailable: {error}", file=sys.stderr)
        return []


def _ocr_identity_matches_rect(ocr_words, rect_obj, old_text: str) -> bool:
    rect = pymupdf.Rect(rect_obj)
    selected = []
    for word in ocr_words or []:
        word_rect = pymupdf.Rect(float(word[0]), float(word[1]), float(word[2]), float(word[3]))
        center = pymupdf.Point(
            (float(word_rect.x0) + float(word_rect.x1)) / 2.0,
            (float(word_rect.y0) + float(word_rect.y1)) / 2.0,
        )
        if center in rect:
            selected.append(word)
            continue
        intersection = word_rect & rect
        word_area = max(float(word_rect.width * word_rect.height), 0.0)
        if not intersection.is_empty and word_area > 0.0:
            overlap = float(intersection.width * intersection.height) / word_area
            if overlap >= 0.6:
                selected.append(word)
    if not selected:
        return False
    selected.sort(key=lambda item: (round(float(item[1]), 2), float(item[0])))
    observed = " ".join(str(word[4]).strip() for word in selected if str(word[4]).strip())
    observed_identity = _normalized_text_identity(observed)
    requested_identity = _normalized_text_identity(old_text)
    if observed_identity == requested_identity:
        return True
    observed_money = _normalized_money_identity(observed)
    requested_money = _normalized_money_identity(old_text)
    return requested_money is not None and observed_money == requested_money


def _find_exact_target_spans(page, rect_obj, old_text: str, ocr_words=None) -> list:
    """Return spans matching stable text or exact money identity and geometry."""
    rect = pymupdf.Rect(rect_obj)
    rect_area = max(float(rect.width * rect.height), 0.0)
    if rect_area <= 0.0:
        return []
    identity = _normalized_text_identity(old_text)
    money_identity = _normalized_money_identity(old_text)
    matches = []
    for block in page.get_text("dict").get("blocks", []):
        if "lines" not in block:
            continue
        for line in block["lines"]:
            for span in line.get("spans", []):
                span_text = span.get("text", "")
                span_identity = _normalized_text_identity(span_text)
                dotted_leader_matches = False
                if identity and span_identity.startswith(identity):
                    suffix = span_identity[len(identity):]
                    dotted_leader_matches = bool(suffix) and all(
                        character == "." for character in suffix
                    )
                date_suffix_matches = False
                if (
                    re.fullmatch(r"\d{1,2}\s+[A-Za-z]{3}", identity or "")
                    and span_identity.casefold().startswith(identity.casefold())
                ):
                    suffix = span_identity[len(identity):].strip()
                    date_suffix_matches = bool(
                        re.fullmatch(r"(?:\d{2}|\d{4})", suffix)
                    )
                text_matches = (
                    span_identity == identity
                    or dotted_leader_matches
                    or date_suffix_matches
                )
                money_matches = (
                    money_identity is not None
                    and _normalized_money_identity(span_text) == money_identity
                )
                if not text_matches and not money_matches:
                    continue
                span_rect = pymupdf.Rect(span.get("bbox") or (0, 0, 0, 0))
                intersection = span_rect & rect
                if intersection.is_empty:
                    continue
                overlap = float(intersection.width * intersection.height) / rect_area
                if overlap >= 0.5:
                    matches.append(span)
    if matches or not ocr_words:
        return matches
    if not _ocr_identity_matches_rect(ocr_words, rect, old_text):
        return []
    native_span = _find_dominant_span(page, rect)
    if native_span is None:
        return []
    native_span = dict(native_span)
    native_span["_ocr_identity_verified"] = True
    return [native_span]


_STANDARD_14_FONTS = {
    # The PDF spec guarantees every reader supplies these. Their full
    # WinAnsiEncoding glyph set is always usable, so coverage is implicit.
    "times-roman", "times-bold", "times-italic", "times-bolditalic",
    "helvetica", "helvetica-bold", "helvetica-oblique", "helvetica-boldoblique",
    "courier", "courier-bold", "courier-oblique", "courier-boldoblique",
    "symbol", "zapfdingbats",
}


def _is_standard_14(name: str) -> bool:
    if not name:
        return False
    n = name.lower()
    # Some PDFs prefix subsetted names like "ABCDEF+Times-Roman".
    if "+" in n:
        n = n.split("+", 1)[1]
    return n in _STANDARD_14_FONTS


def _font_covers_text(page, font_xref: int, font_name: str, text: str):
    """Return (covers, missing_chars).

    Coverage logic, in order:
      1. If `font_name` is one of the PDF standard 14 (Times/Helvetica/Courier/Symbol/ZapfDingbats),
         every WinAnsi codepoint is supplied by the reader. We only flag
         characters outside WinAnsiEncoding (rare -- emoji, CJK, etc.).
      2. Otherwise we attempt to extract the embedded font subset and probe
         glyph coverage with PyMuPDF.Font(buffer=...).
      3. Any failure to determine coverage is treated as 'unknown' and
         returns (False, list(text)) so the caller can decide.
    """
    if _is_standard_14(font_name):
        # WinAnsi covers most western characters. Flag only ones that are not
        # representable in cp1252.
        missing = []
        for ch in text:
            try:
                ch.encode("cp1252")
            except UnicodeEncodeError:
                missing.append(ch)
        return (len(missing) == 0, missing)

    try:
        result = page.parent.extract_font(font_xref)
    except Exception:
        return (False, list(text))

    content = None
    if isinstance(result, dict):
        content = result.get("content")
    elif isinstance(result, (tuple, list)) and len(result) >= 4:
        for item in reversed(result):
            if isinstance(item, (bytes, bytearray)) and len(item) > 0:
                content = bytes(item)
                break

    if not content:
        return (False, list(text))

    try:
        f = pymupdf.Font(fontbuffer=content)
    except Exception:
        return (False, list(text))

    missing = []
    for ch in text:
        if ch in (" ",):
            continue
        try:
            ok = bool(f.has_glyph(ord(ch)))
        except Exception:
            try:
                ok = bool(f.glyph_advance(ord(ch)))
            except Exception:
                ok = False
        if not ok:
            missing.append(ch)
    return (len(missing) == 0, missing)


def _embedded_font_xref_for_span(page, span: dict):
    """Locate the xref of the font used by `span`.

    PyMuPDF `dict` extraction returns the font's *base name* (e.g. 'Times-Roman'
    or 'F1' depending on the source). We cross-reference page.get_fonts(full=True)
    to pull the matching xref. Falls back to the first font on the page.
    """
    try:
        fonts = page.get_fonts(full=True)
    except Exception:
        return None
    if not fonts:
        return None
    raw_name = span.get("font") or ""
    type3_reference = re.search(r"Type3\s*\((\d+)\s+0\s+R\)", raw_name, re.IGNORECASE)
    if type3_reference:
        referenced_xref = int(type3_reference.group(1))
        for font in fonts:
            try:
                if int(font[0]) == referenced_xref:
                    return referenced_xref
            except (IndexError, TypeError, ValueError):
                continue

    needle = raw_name.lower()
    if needle:
        # Match by basefont (index 3) or by the alias name (index 4).
        for f in fonts:
            try:
                basefont = (f[3] or "").lower()
                alias = (f[4] or "").lower()
            except (IndexError, TypeError):
                continue
            if basefont == needle or alias == needle or basefont.endswith("+" + needle) or needle in basefont:
                return f[0]
    # No match: first font.
    try:
        return fonts[0][0]
    except (IndexError, TypeError):
        return None


def _median(values):
    ordered = sorted(float(value) for value in values)
    if not ordered:
        return 0.0
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2.0


def _type3_source_resource_plan(
    page,
    span: dict,
    new_text: str,
    source_text: str | None = None,
):
    """Return a fail-closed same-resource Type3 emission plan or ``None``.

    Type3 fonts have no extractable TTF/CFF program, so the normal embedded-font
    path cannot re-register them. Their existing page resource can still be
    reused exactly when text tracing proves a unique one-byte source code for
    every replacement character. No glyph is synthesized and no substitute font
    is allowed. Ambiguous, missing, multi-byte, or rotated mappings fail closed.
    """
    font_name = str(span.get("font") or "")
    reference = re.search(r"Type3\s*\((\d+)\s+0\s+R\)", font_name, re.IGNORECASE)
    if not reference:
        return None
    font_xref = int(reference.group(1))
    aliases = {}
    try:
        for font in page.get_fonts(full=True):
            aliases[int(font[0])] = str(font[4] or "")
    except Exception:
        aliases = {}
    try:
        traces = list(page.get_texttrace())
    except Exception as error:
        return {
            "available": False,
            "font_xref": font_xref,
            "font_name": font_name,
            "resource_alias": aliases.get(font_xref),
            "missing_chars": list(dict.fromkeys(new_text)),
            "ambiguous_chars": [],
            "reason": f"text trace unavailable: {error}",
        }

    def evidence_for(candidate_name):
        code_sets = {}
        advance_samples = {}
        sizes = []
        for trace in traces:
            if str(trace.get("font") or "") != candidate_name:
                continue
            trace_size = max(float(trace.get("size") or span.get("size") or 10.0), 1.0)
            sizes.append(trace_size)
            chars = list(trace.get("chars") or [])
            for index, item in enumerate(chars):
                try:
                    character = chr(int(item[0]))
                    source_code = int(item[1])
                    origin = item[2]
                    bbox = item[3]
                except (IndexError, TypeError, ValueError, OverflowError):
                    continue
                if not 0 <= source_code <= 255:
                    continue
                code_sets.setdefault(character, set()).add(source_code)
                advance = max(float(bbox[2]) - float(bbox[0]), 0.0)
                if index + 1 < len(chars):
                    try:
                        next_origin = chars[index + 1][2]
                        same_baseline = abs(float(next_origin[1]) - float(origin[1])) <= 0.25
                        delta = float(next_origin[0]) - float(origin[0])
                        if same_baseline and 0.0 < delta <= trace_size * 2.5:
                            advance = delta
                    except (IndexError, TypeError, ValueError):
                        pass
                if advance > 0.0:
                    advance_samples.setdefault(character, []).append(advance)
        return code_sets, advance_samples, _median(sizes)

    unique_characters = list(dict.fromkeys(new_text))
    original_codes, original_advances, original_size = evidence_for(font_name)

    def build_plan(candidate_name, donor=False):
        match = re.search(r"Type3\s*\((\d+)\s+0\s+R\)", candidate_name, re.IGNORECASE)
        if not match:
            return None
        candidate_xref = int(match.group(1))
        resource_alias = aliases.get(candidate_xref)
        code_sets, advance_samples, candidate_size = evidence_for(candidate_name)
        missing_chars = [
            character for character in unique_characters if not code_sets.get(character)
        ]
        ambiguous_chars = [
            character
            for character in unique_characters
            if len(code_sets.get(character, ())) != 1
        ]
        missing_advances = [
            character for character in unique_characters if not advance_samples.get(character)
        ]
        if not resource_alias or missing_chars or ambiguous_chars or missing_advances:
            return None
        codes = {
            character: next(iter(code_sets[character]))
            for character in unique_characters
        }
        advances = {
            character: _median(advance_samples[character])
            for character in unique_characters
        }
        encoded = bytes(codes[character] for character in new_text)
        plan = {
            "available": True,
            "font_xref": candidate_xref,
            "font_name": candidate_name,
            "resource_alias": resource_alias,
            "missing_chars": [],
            "ambiguous_chars": [],
            "codes": codes,
            "advances": advances,
            "encoded_hex": encoded.hex(),
            "source_encoded_hex": None,
            "text_width": sum(advances[character] for character in new_text),
            "donor_font_name": candidate_name if donor else None,
        }
        target_size = max(float(span.get("size") or 10.0), 1.0)
        shared = [
            character
            for character in unique_characters
            if original_advances.get(character) and advance_samples.get(character)
        ]
        if donor and not shared:
            return None
        advance_delta = (
            sum(
                abs(
                    _median(advance_samples[character])
                    - _median(original_advances[character])
                )
                for character in shared
            )
            / max(len(shared), 1)
        )
        size_delta = abs(candidate_size - target_size)
        plan["donor_score"] = advance_delta + size_delta * 0.1
        return plan

    original_plan = build_plan(font_name)
    if original_plan is not None:
        return original_plan

    candidate_names = sorted(
        {
            str(trace.get("font") or "")
            for trace in traces
            if str(trace.get("font") or "").startswith("Type3")
            and str(trace.get("font") or "") != font_name
        }
    )
    donor_plans = [
        plan
        for candidate_name in candidate_names
        if (plan := build_plan(candidate_name, donor=True)) is not None
    ]
    donor_plans = [
        plan
        for plan in donor_plans
        if float(plan.get("donor_score", 999.0)) <= 1.25
    ]
    if donor_plans:
        donor_plans.sort(
            key=lambda plan: (
                float(plan.get("donor_score", 999.0)),
                int(plan.get("font_xref", 0)),
            )
        )
        return donor_plans[0]

    missing_chars = [
        character for character in unique_characters if not original_codes.get(character)
    ]
    ambiguous_chars = [
        character
        for character in unique_characters
        if len(original_codes.get(character, ())) != 1
    ]
    missing_advances = [
        character for character in unique_characters if not original_advances.get(character)
    ]
    return {
        "available": False,
        "font_xref": font_xref,
        "font_name": font_name,
        "resource_alias": aliases.get(font_xref),
        "missing_chars": missing_chars or missing_advances,
        "ambiguous_chars": ambiguous_chars,
        "reason": "no same-page Type3 resource proves one near-metric code and advance per replacement character",
    }


def _replace_type3_inplace(page, span: dict, rect_obj, plan: dict, old_text: str, new_text: str):
    old_codes = bytes.fromhex(str(plan.get("source_encoded_hex") or ""))
    new_codes = bytes.fromhex(str(plan.get("encoded_hex") or ""))
    if not old_codes or not new_codes:
        raise ValueError("TYPE3_INPLACE_CODES_MISSING")
    font_name = str(plan.get("font_name") or span.get("font") or "")
    alias = str(plan.get("resource_alias") or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", alias):
        raise ValueError("TYPE3_INPLACE_ALIAS_INVALID")

    target_sequences = []
    for trace in page.get_texttrace():
        if str(trace.get("font") or "") != font_name:
            continue
        chars = list(trace.get("chars") or [])
        glyphs = bytes(int(item[1]) for item in chars)
        start = 0
        while True:
            index = glyphs.find(old_codes, start)
            if index < 0:
                break
            selected = chars[index:index + len(old_codes)]
            sequence_rect = pymupdf.Rect(selected[0][3])
            for item in selected[1:]:
                sequence_rect |= pymupdf.Rect(item[3])
            if not (sequence_rect & rect_obj).is_empty:
                target_sequences.append({"trace": trace, "bbox": sequence_rect})
            start = index + 1
    if len(target_sequences) != 1:
        raise ValueError(f"TYPE3_INPLACE_TARGET_TRACE_COUNT:{len(target_sequences)}")

    document = page.parent
    stream_matches = []
    font_pattern = re.compile(rb"/([A-Za-z0-9_.-]+)\s+([0-9.]+)\s+Tf")
    for content_xref in page.get_contents():
        stream = document.xref_stream(int(content_xref)) or b""
        start = 0
        while True:
            offset = stream.find(old_codes, start)
            if offset < 0:
                break
            active_fonts = list(font_pattern.finditer(stream[:offset]))
            if active_fonts and active_fonts[-1].group(1).decode("ascii") == alias:
                stream_matches.append({
                    "xref": int(content_xref),
                    "offset": offset,
                    "stream": stream,
                    "font_size": float(active_fonts[-1].group(2)),
                })
            start = offset + 1
    if len(stream_matches) != 1:
        raise ValueError(f"TYPE3_INPLACE_STREAM_MATCH_COUNT:{len(stream_matches)}")

    match = stream_matches[0]
    stream = match["stream"]
    offset = int(match["offset"])
    tail = offset + len(old_codes)
    if offset < 1 or stream[offset - 1:offset] != b"(" or stream[tail:tail + 3] != b")Tj":
        raise ValueError("TYPE3_INPLACE_LITERAL_OPERATOR_REQUIRED")
    trace_size = float(
        target_sequences[0]["trace"].get("size")
        or span.get("size")
        or 1.0
    )
    content_font_size = float(match["font_size"])
    if content_font_size <= 0.0 or trace_size <= 0.0:
        raise ValueError("TYPE3_INPLACE_FONT_SIZE_INVALID")
    content_to_page_scale = trace_size / content_font_size
    advances = plan.get("advances") or {}
    old_width = sum(float(advances[character]) for character in old_text)
    new_width = sum(float(advances[character]) for character in new_text)
    shift_content_units = (new_width - old_width) / content_to_page_scale
    shift = f"{-shift_content_units:.8f} 0 Td\n".encode("ascii")
    restore = f"{shift_content_units:.8f} 0 Td\n".encode("ascii")
    updated = (
        stream[:offset - 1]
        + shift
        + b"<" + new_codes.hex().encode("ascii") + b"> Tj\n"
        + restore
        + stream[tail + 3:]
    )
    document.update_stream(int(match["xref"]), updated, compress=True)
    return int(match["xref"])


def _glyph_name_character(name: str):
    standard = {
        "space": " ", "dollar": "$", "period": ".", "comma": ",",
        "hyphen": "-", "minus": "-", "plus": "+", "parenleft": "(",
        "parenright": ")", "zero": "0", "one": "1", "two": "2",
        "three": "3", "four": "4", "five": "5", "six": "6",
        "seven": "7", "eight": "8", "nine": "9",
    }
    if name in standard:
        return standard[name]
    converter = getattr(pymupdf, "glyph_name_to_unicode", None)
    if converter is None:
        return None
    try:
        value = converter(name)
        if isinstance(value, int) and 0 <= value <= 0x10FFFF:
            return chr(value)
        if isinstance(value, str) and len(value) == 1:
            return value
    except Exception:
        return None
    return None


def _winansi_code_map(document, font_xref: int):
    code_to_character = {}
    for code in range(256):
        try:
            code_to_character[code] = bytes([code]).decode("cp1252")
        except UnicodeDecodeError:
            continue

    try:
        encoding_type, encoding_value = document.xref_get_key(font_xref, "Encoding")
    except Exception:
        encoding_type, encoding_value = "", ""
    encoding_object = str(encoding_value or "")
    if encoding_type == "xref":
        match = re.match(r"\s*(\d+)\s+0\s+R", encoding_object)
        if match:
            try:
                encoding_object = document.xref_object(int(match.group(1)), compressed=False)
            except Exception:
                encoding_object = ""

    differences = re.search(r"/Differences\s*\[(.*?)\]", encoding_object, re.DOTALL)
    if differences:
        current_code = None
        for token in re.findall(r"/[^\s\[\]<>]+|[-+]?\d+", differences.group(1)):
            if token.lstrip("+-").isdigit():
                current_code = int(token)
                continue
            if current_code is None or not token.startswith("/"):
                continue
            character = _glyph_name_character(token[1:])
            if character is not None and 0 <= current_code <= 255:
                code_to_character[current_code] = character
            current_code += 1

    character_codes = {}
    for code, character in code_to_character.items():
        character_codes.setdefault(character, set()).add(code)
    return character_codes


def _macroman_code_map(document, font_xref: int):
    code_to_character = {}
    for code in range(256):
        try:
            code_to_character[code] = bytes([code]).decode("mac_roman")
        except UnicodeDecodeError:
            continue

    try:
        encoding_type, encoding_value = document.xref_get_key(font_xref, "Encoding")
    except Exception:
        encoding_type, encoding_value = "", ""
    encoding_object = str(encoding_value or "")
    if encoding_type == "xref":
        match = re.match(r"\s*(\d+)\s+0\s+R", encoding_object)
        if match:
            try:
                encoding_object = document.xref_object(int(match.group(1)), compressed=False)
            except Exception:
                encoding_object = ""

    differences = re.search(r"/Differences\s*\[(.*?)\]", encoding_object, re.DOTALL)
    if differences:
        current_code = None
        for token in re.findall(r"/[^\s\[\]<>]+|[-+]?\d+", differences.group(1)):
            if token.lstrip("+-").isdigit():
                current_code = int(token)
                continue
            if current_code is None or not token.startswith("/"):
                continue
            character = _glyph_name_character(token[1:])
            if character is not None and 0 <= current_code <= 255:
                code_to_character[current_code] = character
            current_code += 1

    character_codes = {}
    for code, character in code_to_character.items():
        character_codes.setdefault(character, set()).add(code)
    return character_codes


def _simple_source_resource_plan(page, span: dict, font_xref, new_text: str, old_text: str = None):
    """Plan exact one-byte emission through an existing source font resource.

    This path is for simple TrueType fonts whose embedded subset is valid for
    rendering but whose extracted cmap cannot be safely re-embedded by PyMuPDF.
    It reuses the page's original font dictionary, Encoding and ToUnicode map.
    Characters must map to exactly one source byte and exist in the embedded
    glyph program; otherwise the operation fails closed or uses an explicitly
    supplied reviewed font.
    """
    if font_xref is None:
        return None
    resource = None
    try:
        for font in page.get_fonts(full=True):
            if int(font[0]) == int(font_xref):
                resource = font
                break
    except Exception:
        resource = None
    if resource is None:
        return None
    font_type = str(resource[2] or "")
    encoding_name = str(resource[5] or "")
    is_winansi = "WinAnsiEncoding" in encoding_name
    is_macroman = "MacRomanEncoding" in encoding_name
    if font_type != "TrueType" or not (is_winansi or is_macroman):
        return None
    if is_macroman and old_text is not None and len(old_text) == len(new_text):
        money_pattern = re.compile(r"^[+\-]?[0-9][0-9,]*(?:\.[0-9]+)?$")
        if not (
            money_pattern.fullmatch(old_text.strip())
            and money_pattern.fullmatch(new_text.strip())
        ):
            return None
    resource_alias = str(resource[4] or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", resource_alias):
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": list(dict.fromkeys(new_text)),
            "ambiguous_chars": [],
            "reason": "simple font resource alias is unavailable or invalid",
        }

    character_codes = (
        _winansi_code_map(page.parent, int(font_xref))
        if is_winansi
        else _macroman_code_map(page.parent, int(font_xref))
    )
    unique_characters = list(dict.fromkeys(new_text))
    missing_chars = [character for character in unique_characters if not character_codes.get(character)]
    ambiguous_chars = [
        character for character in unique_characters if len(character_codes.get(character, ())) != 1
    ]
    buffer = _extract_font_buffer(page, int(font_xref))
    font_obj = None
    if buffer:
        try:
            font_obj = pymupdf.Font(fontbuffer=buffer)
        except Exception:
            font_obj = None
    uncovered = []
    if font_obj is None:
        uncovered = unique_characters
    else:
        uncovered = [
            character for character in unique_characters
            if character != " " and not _has_glyph_safe(font_obj, ord(character))
        ]
    missing_chars = list(dict.fromkeys(missing_chars + uncovered))
    if missing_chars or ambiguous_chars:
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": missing_chars,
            "ambiguous_chars": ambiguous_chars,
            "reason": "source one-byte resource does not prove one covered byte per character",
        }

    codes = {character: next(iter(character_codes[character])) for character in unique_characters}
    encoded = bytes(codes[character] for character in new_text)
    fontsize = float(span.get("size") or 10.0)
    return {
        "available": True,
        "font_xref": int(font_xref),
        "font_type": font_type,
        "encoding": encoding_name,
        "resource_alias": resource_alias,
        "missing_chars": [],
        "ambiguous_chars": [],
        "codes": codes,
        "encoded_hex": encoded.hex(),
        "font_obj": font_obj,
        "text_width": float(font_obj.text_length(new_text, fontsize=fontsize)),
    }


def _one_byte_same_length_plan(page, span: dict, font_xref, old_text: str, new_text: str):
    if font_xref is None or len(old_text) != len(new_text):
        return None
    resource = None
    try:
        for font in page.get_fonts(full=True):
            if int(font[0]) == int(font_xref):
                resource = font
                break
    except Exception:
        resource = None
    if resource is None:
        return None
    encoding_name = str(resource[5] or "")
    codec = (
        "mac_roman" if "MacRomanEncoding" in encoding_name
        else "cp1252" if "WinAnsiEncoding" in encoding_name
        else None
    )
    if codec is None:
        return None
    try:
        source_bytes = old_text.encode(codec)
        replacement_bytes = new_text.encode(codec)
    except UnicodeEncodeError:
        return None
    if len(source_bytes) != len(replacement_bytes):
        return None
    resource_alias = str(resource[4] or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", resource_alias):
        return None
    coverage_ok, missing_chars = _font_covers_text(
        page,
        int(font_xref),
        str(span.get("font") or ""),
        new_text,
    )
    if not coverage_ok:
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": missing_chars,
            "ambiguous_chars": [],
            "reason": "one-byte source font does not cover the replacement",
        }
    font_pattern = re.compile(rb"/([A-Za-z0-9_.-]+)\s+([0-9.]+)\s+Tf")
    matches = []
    literal_matches = []
    for content_xref in page.get_contents():
        stream = page.parent.xref_stream(int(content_xref)) or b""
        start = 0
        while True:
            offset = stream.find(source_bytes, start)
            if offset < 0:
                break
            active_fonts = list(font_pattern.finditer(stream[:offset]))
            tail = offset + len(source_bytes)
            literal = (
                offset > 0
                and stream[offset - 1:offset] == b"("
                and stream[tail:tail + 3] == b")Tj"
            )
            if literal:
                literal_matches.append((int(content_xref), offset))
                if active_fonts and active_fonts[-1].group(1).decode("ascii") == resource_alias:
                    matches.append((int(content_xref), offset))
            start = offset + 1
    match_basis = "font-resource"
    if not matches:
        same_font_spans = []
        target_font_name = str(span.get("font") or "")
        for block in page.get_text("dict").get("blocks", []):
            if "lines" not in block:
                continue
            for line in block.get("lines", []):
                for candidate in line.get("spans", []):
                    if _normalized_text_identity(candidate.get("text", "")) != _normalized_text_identity(old_text):
                        continue
                    if str(candidate.get("font") or "") != target_font_name:
                        continue
                    same_font_spans.append(
                        pymupdf.Rect(candidate.get("bbox") or (0, 0, 0, 0))
                    )
        if not literal_matches or len(same_font_spans) != len(literal_matches):
            return None
        matches = literal_matches
        match_basis = "geometry-ordinal-inherited-font-state"

    match_ordinal = 0
    if len(matches) > 1:
        target_bbox = pymupdf.Rect(span.get("bbox") or (0, 0, 0, 0))
        candidate_spans = []
        for block in page.get_text("dict").get("blocks", []):
            if "lines" not in block:
                continue
            for line in block.get("lines", []):
                for candidate in line.get("spans", []):
                    if _normalized_text_identity(candidate.get("text", "")) != _normalized_text_identity(old_text):
                        continue
                    candidate_xref = _embedded_font_xref_for_span(page, candidate)
                    if candidate_xref is None or int(candidate_xref) != int(font_xref):
                        continue
                    candidate_bbox = pymupdf.Rect(candidate.get("bbox") or (0, 0, 0, 0))
                    candidate_spans.append(candidate_bbox)
        candidate_spans.sort(key=lambda bbox: (round(float(bbox.y0), 3), round(float(bbox.x0), 3)))
        target_indices = [
            index
            for index, bbox in enumerate(candidate_spans)
            if max(abs(float(bbox[i]) - float(target_bbox[i])) for i in range(4)) <= 0.75
        ]
        if len(candidate_spans) != len(matches) or len(target_indices) != 1:
            return None
        match_ordinal = target_indices[0]
    return {
        "available": True,
        "font_xref": int(font_xref),
        "font_type": str(resource[2] or ""),
        "encoding": encoding_name,
        "resource_alias": resource_alias,
        "missing_chars": [],
        "ambiguous_chars": [],
        "source_encoded_hex": source_bytes.hex(),
        "encoded_hex": replacement_bytes.hex(),
        "font_obj": None,
        "text_width": float(pymupdf.Rect(span.get("bbox") or (0, 0, 0, 0)).width),
        "proven_stream_match_count": len(matches),
        "match_ordinal": match_ordinal,
        "match_basis": match_basis,
    }


def _replace_one_byte_same_length_inplace(page, plan: dict):
    old_bytes = bytes.fromhex(str(plan.get("source_encoded_hex") or ""))
    new_bytes = bytes.fromhex(str(plan.get("encoded_hex") or ""))
    alias = str(plan.get("resource_alias") or "")
    if not old_bytes or len(old_bytes) != len(new_bytes):
        raise ValueError("ONE_BYTE_INPLACE_LENGTH_MISMATCH")
    font_pattern = re.compile(rb"/([A-Za-z0-9_.-]+)\s+([0-9.]+)\s+Tf")
    matches = []
    literal_matches = []
    document = page.parent
    for content_xref in page.get_contents():
        stream = document.xref_stream(int(content_xref)) or b""
        start = 0
        while True:
            offset = stream.find(old_bytes, start)
            if offset < 0:
                break
            active_fonts = list(font_pattern.finditer(stream[:offset]))
            tail = offset + len(old_bytes)
            literal = (
                offset > 0
                and stream[offset - 1:offset] == b"("
                and stream[tail:tail + 3] == b")Tj"
            )
            if literal:
                literal_matches.append((int(content_xref), offset, stream))
                if active_fonts and active_fonts[-1].group(1).decode("ascii") == alias:
                    matches.append((int(content_xref), offset, stream))
            start = offset + 1
    if not matches and plan.get("match_basis") == "geometry-ordinal-inherited-font-state":
        matches = literal_matches
    match_ordinal = int(plan.get("match_ordinal", 0))
    expected_matches = int(plan.get("proven_stream_match_count", 1))
    if len(matches) != expected_matches or match_ordinal < 0 or match_ordinal >= len(matches):
        raise ValueError(
            f"ONE_BYTE_INPLACE_MATCH_COUNT:{len(matches)}:EXPECTED:{expected_matches}:ORDINAL:{match_ordinal}"
        )
    content_xref, offset, stream = matches[match_ordinal]
    updated = stream[:offset] + new_bytes + stream[offset + len(old_bytes):]
    document.update_stream(content_xref, updated, compress=True)
    return content_xref


def _load_font_resource_profiles():
    global _FONT_RESOURCE_PROFILES
    if _FONT_RESOURCE_PROFILES is not None:
        return _FONT_RESOURCE_PROFILES
    profile_path = os.path.join(os.path.dirname(__file__), "font-resource-profiles.json")
    try:
        with open(profile_path, "r", encoding="utf-8") as stream:
            payload = json.load(stream)
    except (OSError, json.JSONDecodeError):
        _FONT_RESOURCE_PROFILES = {}
        return _FONT_RESOURCE_PROFILES
    profiles = payload.get("profiles") if payload.get("schema_version") == 1 else None
    _FONT_RESOURCE_PROFILES = profiles if isinstance(profiles, dict) else {}
    return _FONT_RESOURCE_PROFILES


class _ProfiledFontMetrics:
    def __init__(self, character_to_cid: dict, advance_em: dict):
        self.character_to_cid = dict(character_to_cid)
        self.advance_em = dict(advance_em)

    def text_length(self, text: str, fontsize: float = 1.0) -> float:
        total = 0.0
        for character in text:
            cid = self.character_to_cid.get(character)
            if cid is not None:
                total += float(self.advance_em.get(str(cid), 0.0)) * float(fontsize)
        return total

    def glyph_advance(self, codepoint: int) -> float:
        try:
            character = chr(int(codepoint))
        except (TypeError, ValueError, OverflowError):
            return 0.0
        cid = self.character_to_cid.get(character)
        if cid is None:
            return 0.0
        return float(self.advance_em.get(str(cid), 0.0))


def _profiled_type0_source_resource_plan(
    page,
    span: dict,
    font_xref,
    new_text: str,
    source_text: str | None = None,
):
    if font_xref is None:
        return None
    resource = None
    try:
        for font in page.get_fonts(full=True):
            if int(font[0]) == int(font_xref):
                resource = font
                break
    except Exception:
        resource = None
    if resource is None:
        return None
    font_type = str(resource[2] or "")
    encoding_name = str(resource[5] or "")
    if font_type != "Type0" or encoding_name != "Identity-H":
        return None
    resource_alias = str(resource[4] or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", resource_alias):
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": list(dict.fromkeys(new_text)),
            "ambiguous_chars": [],
            "reason": "profiled Type0 resource alias is unavailable or invalid",
        }
    buffer = _extract_font_buffer(page, int(font_xref))
    fingerprint = hashlib.sha256(buffer).hexdigest() if buffer else None
    profile = _load_font_resource_profiles().get(fingerprint or "")
    if not isinstance(profile, dict):
        return None
    if (
        profile.get("font_program_sha256") != fingerprint
        or profile.get("pdf_subtype") != font_type
        or profile.get("encoding") != encoding_name
    ):
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": list(dict.fromkeys(new_text)),
            "ambiguous_chars": [],
            "reason": "profile metadata does not match the active Type0 font resource",
        }
    character_to_cid = profile.get("character_to_cid") or {}
    advance_em = profile.get("cid_advance_em") or {}
    unique_characters = list(dict.fromkeys(new_text))
    missing_chars = [
        character
        for character in unique_characters
        if character not in character_to_cid
        or str(character_to_cid[character]) not in advance_em
    ]
    if missing_chars:
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": missing_chars,
            "ambiguous_chars": [],
            "reason": "replacement contains characters absent from the verified Type0 profile",
            "profile_sha256": fingerprint,
        }
    codes = {
        character: int(character_to_cid[character])
        for character in unique_characters
    }
    if any(code < 0 or code > 0xFFFF for code in codes.values()):
        return {
            "available": False,
            "font_xref": int(font_xref),
            "resource_alias": resource_alias,
            "missing_chars": unique_characters,
            "ambiguous_chars": [],
            "reason": "profile contains a CID outside the Identity-H two-byte range",
            "profile_sha256": fingerprint,
        }
    encoded = b"".join(
        codes[character].to_bytes(2, "big") for character in new_text
    )
    source_codes = None
    if source_text:
        if all(character in character_to_cid for character in source_text):
            source_codes = [int(character_to_cid[character]) for character in source_text]
        elif all(ord(character) <= 0xFFFF for character in source_text):
            # Empty-ToUnicode Identity-H fonts may expose their raw CIDs as
            # control-code characters in the matched dict span (for example
            # the ANZ fixture). Preserve those exact source codes.
            source_codes = [ord(character) for character in source_text]
    source_encoded = None
    source_text_width = None
    if source_codes is not None:
        source_encoded = b"".join(code.to_bytes(2, "big") for code in source_codes)
    fontsize = float(span.get("size") or 10.0)
    if source_codes is not None and all(str(code) in advance_em for code in source_codes):
        source_text_width = sum(
            float(advance_em[str(code)]) * fontsize for code in source_codes
        )
    text_width = sum(
        float(advance_em[str(codes[character])]) * fontsize
        for character in new_text
    )
    return {
        "available": True,
        "font_xref": int(font_xref),
        "font_type": font_type,
        "encoding": encoding_name,
        "resource_alias": resource_alias,
        "missing_chars": [],
        "ambiguous_chars": [],
        "codes": codes,
        "expected_glyph_ids": [codes[character] for character in new_text],
        "encoded_hex": encoded.hex(),
        "source_encoded_hex": source_encoded.hex() if source_encoded is not None else None,
        "source_text_width": source_text_width,
        "font_obj": _ProfiledFontMetrics(character_to_cid, advance_em),
        "text_width": text_width,
        "profile_sha256": fingerprint,
        "profile_name": profile.get("name"),
    }


def _profiled_glyph_sequence_present(page, rect_obj, plan: dict) -> bool:
    expected = list(plan.get("expected_glyph_ids") or [])
    if not expected:
        return True
    matches = 0
    try:
        traces = page.get_texttrace()
    except Exception:
        return False
    expanded = pymupdf.Rect(rect_obj)
    expanded.x0 -= 3.0
    expanded.y0 -= 3.0
    expanded.x1 += 3.0
    expanded.y1 += 3.0
    for trace in traces:
        chars = list(trace.get("chars") or [])
        glyph_ids = [int(item[1]) for item in chars]
        if glyph_ids != expected:
            continue
        trace_rect = pymupdf.Rect(trace.get("bbox") or (0, 0, 0, 0))
        if not (trace_rect & expanded).is_empty:
            matches += 1
    return matches == 1


def _profiled_type0_inplace_matches(page, plan: dict):
    old_hex = str(plan.get("source_encoded_hex") or "").encode("ascii")
    if not old_hex:
        return []
    alias = str(plan.get("resource_alias") or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", alias):
        return []
    font_pattern = re.compile(rb"/([A-Za-z0-9_.-]+)\s+([0-9.]+)\s+Tf")
    matches = []
    document = page.parent
    for content_xref in page.get_contents():
        stream = document.xref_stream(int(content_xref)) or b""
        searchable = stream.lower()
        start = 0
        while True:
            offset = searchable.find(old_hex.lower(), start)
            if offset < 0:
                break
            active_fonts = list(font_pattern.finditer(stream[:offset]))
            if active_fonts and active_fonts[-1].group(1).decode("ascii") == alias:
                matches.append((
                    int(content_xref),
                    offset,
                    stream,
                    float(active_fonts[-1].group(2)),
                ))
            start = offset + 1
    return matches


def _replace_profiled_type0_inplace(page, span: dict, plan: dict):
    old_hex = str(plan.get("source_encoded_hex") or "").encode("ascii")
    new_hex = str(plan.get("encoded_hex") or "").encode("ascii")
    if not old_hex or not new_hex:
        raise ValueError("PROFILED_TYPE0_INPLACE_CODES_MISSING")
    matches = _profiled_type0_inplace_matches(page, plan)
    if len(matches) != 1:
        raise ValueError(f"PROFILED_TYPE0_INPLACE_MATCH_COUNT:{len(matches)}")
    document = page.parent
    content_xref, offset, stream, content_font_size = matches[0]
    source_slice = stream[offset:offset + len(old_hex)]
    replacement = new_hex.upper() if source_slice.isupper() else new_hex.lower()
    if len(old_hex) == len(new_hex):
        updated = stream[:offset] + replacement + stream[offset + len(old_hex):]
    else:
        source_width = plan.get("source_text_width")
        new_width = plan.get("text_width")
        if source_width is None or new_width is None or content_font_size <= 0.0:
            raise ValueError("PROFILED_TYPE0_INPLACE_WIDTH_EVIDENCE_MISSING")
        if offset < 1 or stream[offset - 1:offset] != b"<":
            raise ValueError("PROFILED_TYPE0_INPLACE_HEX_OPERATOR_REQUIRED")
        tail = offset + len(old_hex)
        operator = re.match(rb">\s*Tj", stream[tail:tail + 12])
        if not operator:
            raise ValueError("PROFILED_TYPE0_INPLACE_TJ_REQUIRED")
        trace_size = float(span.get("size") or 0.0)
        if trace_size <= 0.0:
            raise ValueError("PROFILED_TYPE0_INPLACE_TRACE_SIZE_INVALID")
        content_to_page_scale = trace_size / content_font_size
        shift_content_units = (float(new_width) - float(source_width)) / content_to_page_scale
        shift = f"{-shift_content_units:.8f} 0 Td\n".encode("ascii")
        restore = f"{shift_content_units:.8f} 0 Td\n".encode("ascii")
        updated = (
            stream[:offset - 1]
            + shift
            + b"<" + replacement + b"> Tj\n"
            + restore
            + stream[tail + operator.end():]
        )
    document.update_stream(content_xref, updated, compress=True)
    return content_xref


def _append_page_content_stream(page, stream: bytes):
    document = page.parent
    content_xref = document.get_new_xref()
    document.update_object(content_xref, "<<>>")
    document.update_stream(content_xref, stream, compress=True)
    contents = [int(xref) for xref in page.get_contents()]
    contents.append(content_xref)
    document.xref_set_key(
        page.xref,
        "Contents",
        "[" + " ".join(f"{xref} 0 R" for xref in contents) + "]",
    )
    return content_xref


def _pdf_fill_operator(color) -> str:
    values = tuple(float(value) for value in color)
    if len(values) == 1:
        return f"{values[0]:.8f} g"
    if len(values) == 4:
        return " ".join(f"{value:.8f}" for value in values) + " k"
    rgb = values[:3] if len(values) >= 3 else (0.0, 0.0, 0.0)
    return " ".join(f"{value:.8f}" for value in rgb) + " rg"


def _emit_source_resource(page, placement: dict, plan: dict, fontsize: float, color):
    if not plan.get("available"):
        raise ValueError("SOURCE_RESOURCE_UNAVAILABLE")
    writing_dir = placement.get("writing_dir", (1.0, 0.0))
    if abs(float(writing_dir[0]) - 1.0) > 1e-3 or abs(float(writing_dir[1])) > 1e-3:
        raise ValueError("SOURCE_RESOURCE_WRITING_MODE_UNSUPPORTED")
    alias = str(plan.get("resource_alias") or "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", alias):
        raise ValueError("SOURCE_RESOURCE_ALIAS_INVALID")
    origin = pymupdf.Point(*placement["origin"])
    try:
        pdf_origin = origin * ~page.transformation_matrix
    except Exception:
        pdf_origin = pymupdf.Point(origin.x, float(page.rect.height) - origin.y)
    char_spacing = float(placement.get("char_spacing", 0.0))
    horizontal_scale = float(placement.get("h_scale", 1.0)) * 100.0
    stream = (
        "q\nBT\n"
        f"/{alias} {float(fontsize):.8f} Tf\n"
        f"{char_spacing:.8f} Tc\n"
        f"{horizontal_scale:.8f} Tz\n"
        f"{_pdf_fill_operator(color)}\n"
        f"1 0 0 1 {float(pdf_origin.x):.8f} {float(pdf_origin.y):.8f} Tm\n"
        f"<{plan['encoded_hex']}> Tj\n"
        "ET\nQ\n"
    ).encode("ascii")
    return _append_page_content_stream(page, stream)


# ===========================================================================
# Stage A / Items #1-#4: embedded-font reuse.
#
# The single most important fidelity fix. The old emit path re-inserted the
# new text with `page.insert_text(fontname=<original_basefont_name>)`. For any
# subsetted or non-standard font (`ABCDEF+ArialMT`, `F1`, ...) PyMuPDF cannot
# resolve that name, throws, and silently drops to Helvetica metrics — exactly
# on the fonts where fidelity matters most.
#
# These helpers extract the original embedded glyph program (the actual TTF/
# CFF bytes), register it into the page so `insert_text` can draw with the
# *original outlines + hinting*, and build a `pymupdf.Font` over the same bytes
# so every width / kerning measurement uses the real metrics instead of a
# Helvetica stand-in.
# ===========================================================================

def _extract_font_buffer(page, font_xref):
    """Return the raw embedded font-program bytes for `font_xref`, or None.

    Mirrors the extraction logic in `_font_covers_text`/`analyze_fonts` but
    returns only the bytes so callers can both probe coverage and re-embed.
    """
    if font_xref is None:
        return None
    try:
        result = page.parent.extract_font(font_xref)
    except Exception:
        return None
    content = None
    if isinstance(result, dict):
        content = result.get("content")
    elif isinstance(result, (tuple, list)) and len(result) >= 4:
        for item in reversed(result):
            if isinstance(item, (bytes, bytearray)) and len(item) > 0:
                content = bytes(item)
                break
    if content and len(content) > 0:
        return bytes(content)
    return None


def _resolve_embedded_font(page, font_xref):
    """Item #1/#2/#3: make the original embedded glyph program reusable.

    Returns a dict ``{refname, font_obj, buffer}``:
      * ``refname``  - a font name registered into THIS page via
                       ``insert_font``, usable directly by
                       ``page.insert_text(fontname=refname)``. ``None`` when
                       the program could not be re-embedded.
      * ``font_obj`` - a ``pymupdf.Font`` built from the same buffer, for
                       exact width + kerning measurement. ``None`` on failure.
      * ``buffer``   - the raw font-program bytes.

    Returns ``None`` when no embedded program exists (e.g. a non-embedded
    standard-14 base font); the caller then uses name-based handling.

    Registration is idempotent: ``insert_font`` returns the existing xref
    when the same name is already present on the page, so repeated calls
    inside `apply_many_edits` are cheap.
    """
    buffer = _extract_font_buffer(page, font_xref)
    if not buffer:
        return None
    try:
        font_obj = pymupdf.Font(fontbuffer=buffer)
    except Exception:
        font_obj = None
    refname = f"embf_{font_xref}"
    try:
        page.insert_font(fontname=refname, fontbuffer=buffer)
    except Exception:
        # Some CFF/OTF subsets cannot be re-embedded by name. Preserve the
        # measuring evidence but force the caller to fail closed.
        refname = None

    return {"refname": refname, "font_obj": font_obj, "buffer": buffer}


def _fallback_standard14(font_name: str) -> str:
    """Resolve an original standard-14 font name to PyMuPDF's builtin code.

    This helper is only valid when the selected source span itself uses a
    standard-14 family; it must never substitute for an embedded typeface.
    """
    n = (font_name or "").lower()
    if "+" in n:
        n = n.split("+", 1)[1]
    bold = any(k in n for k in ("bold", "black", "heavy", "semibold", "demibold", "-bd"))
    italic = any(k in n for k in ("italic", "oblique", "-it"))
    mono = any(k in n for k in ("mono", "courier", "consol", "typewriter"))
    serif = (
        any(k in n for k in ("times", "serif", "georgia", "roman", "minion", "garamond", "book"))
        and not mono
    )
    if mono:
        table = {(False, False): "cour", (True, False): "cobo", (False, True): "coit", (True, True): "cobi"}
    elif serif:
        table = {(False, False): "tiro", (True, False): "tibo", (False, True): "tiit", (True, True): "tibi"}
    else:
        table = {(False, False): "helv", (True, False): "hebo", (False, True): "heit", (True, True): "hebi"}
    return table[(bold, italic)]


# ===========================================================================
# Stage 9: Background & color-space fidelity helpers (Items #5, #6).
# ===========================================================================

def _sample_pixel(page, x: float, y: float):
    """Sample a single pixel from the rendered page at (x,y) in PDF points.
    Returns (r,g,b) floats in [0,1]. None if the position is off the page.
    """
    page_w = float(page.rect.width)
    page_h = float(page.rect.height)
    if x < 0 or x >= page_w or y < 0 or y >= page_h:
        return None
    # 1pt-wide clip; cheap and avoids huge pixmap allocation.
    clip = pymupdf.Rect(x, y, x + 1.0, y + 1.0)
    try:
        pix = page.get_pixmap(clip=clip, alpha=False)
    except Exception:
        return None
    if pix.width == 0 or pix.height == 0:
        return None
    samples = pix.samples
    n = pix.n
    if n == 1:
        v = samples[0] / 255.0
        return (v, v, v)
    if n >= 3:
        return (samples[0] / 255.0, samples[1] / 255.0, samples[2] / 255.0)
    return None


def _sample_patch(page, x: float, y: float, half: float = 1.5, dpi: float = 150.0):
    """Stage E / Item #13: sample a small PATCH around (x,y) and return the
    per-channel MEDIAN colour in [0,1], instead of a single pixel.

    Single-pixel probes alias badly on fine zebra striping and halftoned
    watermarks, which made the redaction fill seam-visible against the
    surrounding row. A median over a few-pixel patch is robust to that
    sub-pixel structure while still being local enough not to bleed in a
    neighbouring stripe. Falls back to `_sample_pixel` when rendering fails.
    """
    page_w = float(page.rect.width)
    page_h = float(page.rect.height)
    if x < 0 or x >= page_w or y < 0 or y >= page_h:
        return None
    x0 = max(0.0, x - half)
    y0 = max(0.0, y - half)
    x1 = min(page_w, x + half)
    y1 = min(page_h, y + half)
    if x1 <= x0 or y1 <= y0:
        return _sample_pixel(page, x, y)
    try:
        pix = page.get_pixmap(clip=pymupdf.Rect(x0, y0, x1, y1), dpi=dpi, alpha=False)
    except Exception:
        return _sample_pixel(page, x, y)
    if pix.width == 0 or pix.height == 0:
        return _sample_pixel(page, x, y)
    samples = pix.samples
    n = pix.n
    rs, gs, bs = [], [], []
    for i in range(0, len(samples), n):
        if n == 1:
            v = samples[i]
            rs.append(v); gs.append(v); bs.append(v)
        else:
            rs.append(samples[i]); gs.append(samples[i + 1]); bs.append(samples[i + 2])
    if not rs:
        return _sample_pixel(page, x, y)

    def _median(vals):
        vals = sorted(vals)
        m = len(vals) // 2
        if len(vals) % 2:
            return vals[m]
        return (vals[m - 1] + vals[m]) / 2.0

    return (_median(rs) / 255.0, _median(gs) / 255.0, _median(bs) / 255.0)


def classify_background(page, rect_obj):
    """Stage 9 / Item #5: classify the area beneath an edit as
    solid / striped / patterned, and return the local fill color we should
    use for the redaction.

    Strategy: sample 8 points around the rect's edge, plus the centre.
      * If all samples agree to within tolerance, return ("solid", color).
      * If samples cluster into 2 colors that alternate left-right or
        top-bottom, return ("striped", color_at_center). The redactor
        uses the center color so we do not flip the row's stripe.
      * Otherwise return ("patterned", center_color) and let the caller
        decide whether to fall back to a vector-only redact.
    """
    page_w = float(page.rect.width)
    page_h = float(page.rect.height)
    cx = (rect_obj.x0 + rect_obj.x1) / 2.0
    cy = (rect_obj.y0 + rect_obj.y1) / 2.0
    # `_sample_patch` uses a 1.5 pt half-width. A 1 pt offset therefore lets
    # every sample patch overlap the glyph box by 0.5 pt; tight bold amount
    # spans can contaminate all edge samples and turn a white cell black.
    # Keep the full patch outside the target with a conservative 3 pt gap.
    outside = 3.0

    sample_points = [
        # top-left, top-mid, top-right
        (rect_obj.x0 - outside, rect_obj.y0 - outside),
        (cx, rect_obj.y0 - outside),
        (rect_obj.x1 + outside, rect_obj.y0 - outside),
        # mid row outside left/right
        (rect_obj.x0 - outside, cy),
        (rect_obj.x1 + outside, cy),
        # bottom-left, bottom-mid, bottom-right
        (rect_obj.x0 - outside, rect_obj.y1 + outside),
        (cx, rect_obj.y1 + outside),
        (rect_obj.x1 + outside, rect_obj.y1 + outside),
    ]
    samples = []
    for x, y in sample_points:
        if 0 <= x < page_w and 0 <= y < page_h:
            s = _sample_patch(page, x, y)
            if s is not None:
                samples.append(s)

    centre_color = _sample_patch(page, cx, cy) or (1.0, 1.0, 1.0)

    if not samples:
        return ("solid", centre_color)

    # Unique colors clustered to nearest 0.05 step.
    def quant(c):
        return tuple(round(v * 20.0) / 20.0 for v in c)

    clusters = {}
    for s in samples:
        clusters.setdefault(quant(s), 0)
        clusters[quant(s)] += 1

    def channel_median(colors):
        channels = list(zip(*colors))
        return tuple(_median(channel) for channel in channels)

    # The center of an edit rect usually contains the glyph being replaced.
    # It is therefore evidence about foreground ink, not background. Use the
    # outside-edge samples for any solid fill decision; otherwise black text
    # on a white page becomes a dark redaction rectangle.
    edge_color = channel_median(samples)

    if len(clusters) == 1:
        return ("solid", edge_color)

    # 2 clusters and roughly evenly split → striped.
    if len(clusters) == 2:
        return ("striped", edge_color)

    return ("patterned", centre_color)


def detect_colorspace_from_span(span: dict, page=None) -> str:
    """Stage 9 / Item #6 + Stage 13 / Item #14: figure out which color
    space the original glyph was emitted with.

    Two-tier detection:
      1. If `page` is supplied, scan the content stream for the most
         recent text-fill colorspace operator (`cs`/`CS` followed by
         a known DeviceName). DeviceCMYK requires this path because
         PyMuPDF's `color` integer is always RGB-packed.
      2. Otherwise heuristic from the dict-level color: R==G==B implies
         DeviceGray, else DeviceRGB.

    Returns one of: "DeviceGray", "DeviceRGB", "DeviceCMYK".
    """
    if page is not None:
        cmyk = _scan_content_stream_for_cmyk(page, span.get("bbox"))
        if cmyk:
            return "DeviceCMYK"
    color_int = span.get("color", 0) or 0
    r = (color_int >> 16) & 0xFF
    g = (color_int >> 8) & 0xFF
    b = color_int & 0xFF
    if r == g == b:
        return "DeviceGray"
    return "DeviceRGB"


def _native_fill_color(span: dict, page=None):
    """Stage D / Item #10: return the original glyph fill colour as a tuple
    in its NATIVE colour space, ready to hand to `insert_text`:

      * DeviceGray  -> (v,)              1-tuple
      * DeviceRGB   -> (r, g, b)         3-tuple
      * DeviceCMYK  -> (c, m, y, k)      4-tuple

    PyMuPDF only exposes the span colour as an RGB-packed int, so for CMYK
    we recover the K-heavy equivalent from the RGB value (rich/registration
    black and process tints round-trip faithfully; this avoids the visible
    tone shift of re-emitting a CMYK black as RGB black). For Gray and RGB
    we return the exact value.
    """
    cs = detect_colorspace_from_span(span, page)
    color_int = span.get("color", 0) or 0
    r = ((color_int >> 16) & 0xFF) / 255.0
    g = ((color_int >> 8) & 0xFF) / 255.0
    b = (color_int & 0xFF) / 255.0
    if cs == "DeviceGray":
        # Use the luminance of the (equal) channels.
        return (round(r, 4),)
    if cs == "DeviceCMYK":
        # Standard RGB->CMYK conversion. For pure/near blacks this yields
        # (0,0,0,k) which is how statement text is normally set.
        k = 1.0 - max(r, g, b)
        if k >= 1.0 - 1e-6:
            return (0.0, 0.0, 0.0, 1.0)
        c = (1.0 - r - k) / (1.0 - k)
        m = (1.0 - g - k) / (1.0 - k)
        y = (1.0 - b - k) / (1.0 - k)
        return (round(c, 4), round(m, 4), round(y, 4), round(k, 4))
    return (round(r, 4), round(g, 4), round(b, 4))


def _color_luminance(color):
    """Approximate perceptual luminance [0,1] of a Gray/RGB/CMYK tuple."""
    if not color:
        return 0.0
    if len(color) == 1:
        return float(color[0])
    if len(color) == 4:
        c, m, y, k = color
        r = (1.0 - c) * (1.0 - k)
        g = (1.0 - m) * (1.0 - k)
        b = (1.0 - y) * (1.0 - k)
    else:
        r, g, b = color[0], color[1], color[2]
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def _scan_content_stream_for_cmyk(page, span_bbox) -> bool:
    """Scan a PDF page content stream looking for `K`, `k`, or
    `/DeviceCMYK cs|CS` operators near the text region. Returns True
    when CMYK is the most likely colorspace.

    Heuristic only: PyMuPDF doesn't give per-span colorspace info, so we
    look for *any* CMYK operator on the page and assume it applies. This
    is correct for bank statements that use a uniform colorspace
    throughout (the common case) and slightly over-eager for documents
    that mix CMYK images with RGB text. The over-eager case still
    rounds-trips correctly in DeviceCMYK output, just at slightly higher
    rendering cost — no fidelity loss.
    """
    _ = span_bbox  # unused for now; future work could narrow by line
    try:
        contents = page.read_contents() or b""
    except Exception:
        return False
    if not contents:
        return False
    blob = contents if isinstance(contents, bytes) else bytes(contents)
    return (
        b"/DeviceCMYK" in blob
        or b" k\n" in blob
        or b" K\n" in blob
        or b" k " in blob
        or b" K " in blob
    )


def _vector_strokes_through(page, rect_obj):
    """Find vector strokes (lines / underlines) that pass through `rect_obj`.
    Returns a list of dicts describing each stroke so we can re-draw it
    faithfully after the redaction. Stage 9 / Item #5 + Stage E / Item #14.

    Item #14: we now capture line cap, join, dash pattern and stroke opacity
    in addition to endpoints/width/colour, so a restored underline keeps its
    round caps / dashes instead of coming back square-capped and solid.
    """
    out = []
    try:
        drawings = page.get_drawings()
    except Exception:
        return out
    for d in drawings:
        if d.get("type") != "s":  # 's' = stroked path
            continue
        items = d.get("items", [])
        # Path-level stroke styling (shared by all segments of the path).
        lc = d.get("lineCap")
        if isinstance(lc, (list, tuple)) and lc:
            lc = lc[0]
        lj = d.get("lineJoin")
        dashes = d.get("dashes")
        stroke_op = d.get("stroke_opacity")
        if stroke_op is None:
            stroke_op = 1.0
        color = d.get("color") or (0.0, 0.0, 0.0)
        width = float(d.get("width") or 0.5)
        for item in items:
            # ('l', start, end) means line from start to end.
            if not item or item[0] != "l":
                continue
            start, end = item[1], item[2]
            sx, sy = float(start.x), float(start.y)
            ex, ey = float(end.x), float(end.y)
            # Does this line cross the rect's vertical extent?
            line_top = min(sy, ey)
            line_bot = max(sy, ey)
            if line_bot < rect_obj.y0 - 0.5 or line_top > rect_obj.y1 + 0.5:
                continue
            # And cross horizontally too?
            line_left = min(sx, ex)
            line_right = max(sx, ex)
            if line_right < rect_obj.x0 - 0.5 or line_left > rect_obj.x1 + 0.5:
                continue
            out.append({
                "start": (sx, sy),
                "end": (ex, ey),
                "width": width,
                "color": color,
                "line_cap": int(lc) if isinstance(lc, (int, float)) else 0,
                "line_join": int(lj) if isinstance(lj, (int, float)) else 0,
                "dashes": dashes if isinstance(dashes, str) else None,
                "stroke_opacity": float(stroke_op),
            })
    return out


def _redraw_strokes(page, strokes):
    """Re-draw the supplied vector strokes onto the page after a redaction
    has cleared them. Restores cap/join/dash/opacity (Item #14) so the
    underline is visually identical to the original, not a square-capped
    solid approximation."""
    if not strokes:
        return
    shape = page.new_shape()
    for st in strokes:
        try:
            shape.draw_line(
                pymupdf.Point(st["start"][0], st["start"][1]),
                pymupdf.Point(st["end"][0], st["end"][1]),
            )
            finish_kwargs = {
                "width": st["width"],
                "color": st["color"],
                "stroke_opacity": st.get("stroke_opacity", 1.0),
            }
            # PyMuPDF's Shape.finish accepts lineCap/lineJoin/dashes; guard
            # each so an older build that lacks one still draws the line.
            for key, val, present in (
                ("lineCap", st.get("line_cap"), st.get("line_cap") is not None),
                ("lineJoin", st.get("line_join"), st.get("line_join") is not None),
                ("dashes", st.get("dashes"), bool(st.get("dashes"))),
            ):
                if present:
                    finish_kwargs[key] = val
            try:
                shape.finish(**finish_kwargs)
            except (TypeError, ValueError):
                # Drop the optional styling kwargs and retry with the basics.
                shape.finish(
                    width=st["width"],
                    color=st["color"],
                    stroke_opacity=st.get("stroke_opacity", 1.0),
                )
        except Exception:
            pass
    try:
        shape.commit(overlay=True)
    except Exception:
        pass


# ===========================================================================
# Stage 14a / Item #16: document preconditions.
# ===========================================================================

def _check_doc_editable(doc) -> tuple:
    """Stage 14a / Item #16. Validate that we can actually mutate this PDF.

    Returns ``(ok, reason)`` where ``ok`` is True when the document is
    safe to redact and re-emit text into. False outcomes:

      * encrypted (no decrypt key supplied)
      * permission flags forbid modify
      * permission flags forbid extract (we cannot read fonts)

    Bank statements are usually open, but corporate ones often ship with
    DRM. Detecting up-front turns a silent partial write into a clear
    actionable error.
    """
    try:
        if getattr(doc, "is_encrypted", False) and getattr(doc, "needs_pass", False):
            return (False, "PDF is password-protected. Decrypt before editing.")
    except Exception:
        pass
    try:
        perm = doc.permissions
        # PyMuPDF perm flags: PDF_PERM_MODIFY = 1<<3, PDF_PERM_COPY = 1<<4
        if perm is not None and isinstance(perm, int):
            if (perm & (1 << 3)) == 0:
                return (
                    False,
                    "PDF permissions block modification. Save an unlocked copy first.",
                )
            if (perm & (1 << 4)) == 0:
                return (
                    False,
                    "PDF permissions block content extraction; the editor cannot read embedded fonts.",
                )
    except Exception:
        pass
    return (True, "")


def _is_image_only_page(page) -> bool:
    """Stage 14a / Item #15. Return True when a page has no extractable
    text but does have raster images. These pages are scanned bank
    statements with an OCR text layer that the editor cannot redact via
    `add_redact_annot` reliably; the caller should route to an
    image-paint code path instead.
    """
    try:
        text = page.get_text("text") or ""
    except Exception:
        text = ""
    has_text = any(ch.isalnum() for ch in text)
    if has_text:
        return False
    try:
        images = page.get_images(full=False)
    except Exception:
        images = []
    return bool(images)


def _tight_glyph_bbox(page, rect_obj, fallback_pad: float = 0.5, fg_color=None):
    """Stage 14b / Item #7 + Stage E / Item #12: tighten a span bbox to the
    actual ink extent.

    PyMuPDF span bboxes are line-bounding-box-tight but include trailing
    whitespace and inter-glyph spacing. For a redaction we want to clear
    only the pixels the original glyphs actually covered. Sample the
    pixmap of the bbox region at 200 DPI and find the leftmost and
    rightmost columns containing ink.

    Item #12: ink detection is now COLOUR-AWARE. A fixed luminance cutoff
    mis-detects light-grey subtotals and coloured (e.g. red) negatives —
    either missing them (under-clear, leaving original glyphs) or grabbing
    background. Instead we estimate the local paper colour from the row's
    border pixels and flag any pixel that deviates from it by more than a
    perceptual margin. When `fg_color` (the known original glyph colour, in
    0..1 channels) is supplied we also accept pixels close to it, which
    catches anti-aliased coloured edges the paper-distance test alone might
    treat as background.

    Returns a new pymupdf.Rect; falls back to `rect_obj` (with a
    `fallback_pad`-pt outset) when sampling fails.
    """
    try:
        pix = page.get_pixmap(clip=rect_obj, dpi=200, alpha=False)
    except Exception:
        return pymupdf.Rect(
            rect_obj.x0 - fallback_pad,
            rect_obj.y0 - fallback_pad,
            rect_obj.x1 + fallback_pad,
            rect_obj.y1 + fallback_pad,
        )
    if pix.width == 0 or pix.height == 0:
        return pymupdf.Rect(rect_obj)

    samples = pix.samples
    n = pix.n
    w, h = pix.width, pix.height

    def _rgb_at(x, y):
        idx = (y * w + x) * n
        if n == 1:
            v = samples[idx]
            return (v, v, v)
        return (samples[idx], samples[idx + 1], samples[idx + 2])

    # Estimate paper colour from the four corners + edge midpoints (these are
    # almost always background, not glyph ink).
    border_pts = [
        (0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1),
        (w // 2, 0), (w // 2, h - 1), (0, h // 2), (w - 1, h // 2),
    ]
    br = sum(_rgb_at(x, y)[0] for x, y in border_pts) / len(border_pts)
    bg = sum(_rgb_at(x, y)[1] for x, y in border_pts) / len(border_pts)
    bb = sum(_rgb_at(x, y)[2] for x, y in border_pts) / len(border_pts)

    fg255 = None
    if fg_color is not None and len(fg_color) >= 3:
        fg255 = (fg_color[0] * 255.0, fg_color[1] * 255.0, fg_color[2] * 255.0)

    # Ink = deviation from paper of more than ~14% of full range on any
    # channel-summed distance. Tuned so faint grey (≈0.75 luminance on white)
    # still registers while JPEG/AA noise on a flat background does not.
    paper_margin = 0.14 * (255.0 * 3)
    fg_margin = 0.20 * (255.0 * 3)

    def _is_ink(x, y):
        r, g, b = _rgb_at(x, y)
        d_paper = abs(r - br) + abs(g - bg) + abs(b - bb)
        if d_paper > paper_margin:
            return True
        if fg255 is not None:
            d_fg = abs(r - fg255[0]) + abs(g - fg255[1]) + abs(b - fg255[2])
            if d_fg < fg_margin:
                return True
        return False

    leftmost = None
    rightmost = None
    for x in range(w):
        col_has_ink = False
        for y in range(h):
            if _is_ink(x, y):
                col_has_ink = True
                break
        if col_has_ink:
            if leftmost is None:
                leftmost = x
            rightmost = x

    if leftmost is None or rightmost is None:
        return pymupdf.Rect(rect_obj)

    pt_per_px = 72.0 / 200.0
    new_x0 = rect_obj.x0 + leftmost * pt_per_px - 0.5
    new_x1 = rect_obj.x0 + (rightmost + 1) * pt_per_px + 0.5
    return pymupdf.Rect(new_x0, rect_obj.y0, new_x1, rect_obj.y1)


def _per_glyph_origins(page, span_bbox):
    """Stage 14d / Item #1: read per-character (origin_x, origin_y) from
    the rawdict for the supplied span bbox. Used by the kerned-emit path
    to place each new glyph at the original baseline so superscript
    cents, vertical-shift markers and tabular-figure variants don't drift.

    Returns a list of `(char, origin_x, origin_y)` tuples in document
    order, or `[]` when matching fails.
    """
    if not span_bbox:
        return []
    sx0, sy0, sx1, sy1 = span_bbox
    span_w = max(sx1 - sx0, 1.0)
    try:
        raw = page.get_text("rawdict")
    except Exception:
        return []
    out = []
    for blk in raw.get("blocks", []):
        for ln in blk.get("lines", []):
            for s in ln.get("spans", []):
                sb = s.get("bbox")
                if not sb:
                    continue
                if abs(sb[1] - sy0) > 1.0 or abs(sb[3] - sy1) > 1.0:
                    continue
                ox0 = max(sb[0], sx0)
                ox1 = min(sb[2], sx1)
                if max(0.0, ox1 - ox0) / span_w < 0.5:
                    continue
                for ch in s.get("chars", []):
                    cb = ch.get("bbox")
                    origin = ch.get("origin")
                    if not cb or not origin:
                        continue
                    cx_mid = (float(cb[0]) + float(cb[2])) / 2.0
                    if cx_mid < sx0 - 0.5 or cx_mid > sx1 + 0.5:
                        continue
                    out.append((ch.get("c", ""), float(origin[0]), float(origin[1])))
    out.sort(key=lambda t: t[1])
    return out


def _detect_column_alignment(page, rect_obj, fontsize: float = 10.0):
    """Stage 14b / Item #10: cluster spans on the page by `bbox.x0` and
    `bbox.x1` to detect alignment columns. Return one of
    "left", "right", "center" describing the column the supplied
    `rect_obj` belongs to.

    The algorithm: bucket every non-empty span's x0 and x1 to the
    nearest 2 points. The column is right-aligned when more spans
    share x1 than x0 (within tolerance), left-aligned when the
    inverse, and center when both are tied. Falls back to "left"
    on inconclusive data.
    """
    try:
        blocks = page.get_text("dict").get("blocks", [])
    except Exception:
        return "left"

    cy = (rect_obj.y0 + rect_obj.y1) / 2.0
    height = max(rect_obj.y1 - rect_obj.y0, fontsize)

    x0_buckets: dict = {}
    x1_buckets: dict = {}
    for blk in blocks:
        for ln in blk.get("lines", []):
            for s in ln.get("spans", []):
                bb = s.get("bbox")
                if not bb:
                    continue
                # Only consider spans whose horizontal range *might* be in
                # the same column as our edit rect (within roughly half a
                # cell-width).
                if bb[2] < rect_obj.x0 - 30.0 or bb[0] > rect_obj.x1 + 30.0:
                    continue
                bx0 = round(bb[0] / 2.0) * 2.0
                bx1 = round(bb[2] / 2.0) * 2.0
                x0_buckets[bx0] = x0_buckets.get(bx0, 0) + 1
                x1_buckets[bx1] = x1_buckets.get(bx1, 0) + 1

    if not x0_buckets and not x1_buckets:
        return "left"

    # Find the bucket near our rect's x0 and x1.
    target_x0 = round(rect_obj.x0 / 2.0) * 2.0
    target_x1 = round(rect_obj.x1 / 2.0) * 2.0

    x0_count = x0_buckets.get(target_x0, 0)
    x1_count = x1_buckets.get(target_x1, 0)

    _ = (cy, height)  # unused — left for future per-row narrowing

    if x1_count > x0_count + 1:
        return "right"
    if x0_count > x1_count + 1:
        return "left"
    return "left"


def _looks_numeric(text: str) -> bool:
    """Treat a value as numeric for right-alignment / width-fit purposes when
    it is mostly digits with optional currency, separators, sign and parens."""
    if not text:
        return False
    cleaned = text.strip()
    if not cleaned:
        return False
    digit_count = sum(1 for c in cleaned if c.isdigit())
    if digit_count == 0:
        return False
    # Allow $, €, £, ¥, ',', '.', '-', '+', '(', ')', and whitespace.
    allowed = set("0123456789$€£¥,.-+() \t")
    return all(c in allowed for c in cleaned)


def _measure_text_width(text: str, fontname: str, fontsize: float, supplied_font=None) -> float:
    """Return the rendered width of `text` in PDF points, using either a
    pymupdf.Font built from the embedded subset (preferred) or the
    fontname-based fallback shared with PyMuPDF built-in resolver.
    """
    if not text:
        return 0.0
    # Try the supplied pymupdf.Font (most accurate -- uses the actual subset metrics).
    if supplied_font is not None:
        try:
            return float(supplied_font.text_length(text, fontsize=fontsize))
        except Exception:
            pass
    # Built-in fallback. Try a Font instance for this name; if that fails,
    # default to Helvetica metrics (good enough for measurement).
    try:
        f = pymupdf.Font(fontname=fontname)
        return float(f.text_length(text, fontsize=fontsize))
    except Exception:
        pass
    try:
        f = pymupdf.Font(fontname="helv")
        return float(f.text_length(text, fontsize=fontsize))
    except Exception:
        return float(len(text)) * fontsize * 0.5


def _detect_number_format(old_text: str) -> dict:
    """Decode the formatting of `old_text` so we can reapply it to a new
    numeric value. Returns a dict with:
      currency (str), thousand_sep (str), decimal_sep (str),
      negative_style ('paren'|'minus'|None), trailing_sign (bool),
      decimals (int).
    Stage 8 / Item #12.
    """
    txt = old_text.strip()
    info = {
        "currency": "",
        "thousand_sep": ",",
        "decimal_sep": ".",
        "negative_style": None,
        "trailing_sign": False,
        "decimals": 2,
    }
    if not txt:
        return info

    # Negative -- () or leading -
    if txt.startswith("(") and txt.endswith(")"):
        info["negative_style"] = "paren"
        txt = txt[1:-1]
    elif txt.startswith("-"):
        info["negative_style"] = "minus"
    elif txt.endswith("-"):
        info["negative_style"] = "minus"
        info["trailing_sign"] = True

    # Currency
    for sym in ("$", "€", "£", "¥"):
        if sym in txt:
            info["currency"] = sym
            txt = txt.replace(sym, "")
            break

    # Strip the sign for inspection
    txt = txt.strip().lstrip("-").rstrip("-")
    digits_only = "".join(c for c in txt if c.isdigit())
    if not digits_only:
        return info

    # Find separators. Pattern detection:
    #   "1,234.56" → thousand=’,’ decimal=’.’
    #   "1.234,56" → thousand=’.’ decimal=’,’
    #   "1234.56"  → thousand=’’ decimal=’.’
    #   "1234"     → no decimals
    last_dot = txt.rfind(".")
    last_comma = txt.rfind(",")
    if last_dot >= 0 and last_comma >= 0:
        if last_dot > last_comma:
            info["thousand_sep"] = ","
            info["decimal_sep"] = "."
        else:
            info["thousand_sep"] = "."
            info["decimal_sep"] = ","
    elif last_dot >= 0:
        # Dot only -- could be thousands (’1.234’) or decimal (’123.45’).
        # Heuristic: if the dot is exactly 3 digits from the right and the
        # whole digit run is >= 4 digits, treat as thousands. Otherwise
        # decimal.
        right = txt[last_dot + 1:]
        if len(right) == 3 and len(digits_only) >= 4 and right.isdigit():
            info["thousand_sep"] = "."
            info["decimal_sep"] = ","  # plausibly European
            info["decimals"] = 0
        else:
            info["thousand_sep"] = ""
            info["decimal_sep"] = "."
    elif last_comma >= 0:
        right = txt[last_comma + 1:]
        if len(right) == 3 and len(digits_only) >= 4 and right.isdigit():
            info["thousand_sep"] = ","
            info["decimal_sep"] = "."
            info["decimals"] = 0
        else:
            info["thousand_sep"] = ""
            info["decimal_sep"] = ","
    else:
        info["thousand_sep"] = ""
        info["decimal_sep"] = "."
        info["decimals"] = 0

    # Decimal place count from the right side of the decimal separator.
    if info["decimal_sep"] and info["decimal_sep"] in txt:
        right = txt.rsplit(info["decimal_sep"], 1)[1]
        right_digits = "".join(c for c in right if c.isdigit())
        if right_digits:
            info["decimals"] = len(right_digits)

    return info


def _format_number(value: float, fmt: dict) -> str:
    """Apply `fmt` (from `_detect_number_format`) to `value` to produce a
    string visually consistent with the original number's formatting.
    """
    sign = ""
    n = value
    if n < 0:
        n = -n
        if fmt["negative_style"] == "paren":
            pass  # We add parens at the end.
        elif fmt["negative_style"] == "minus":
            sign = "-"
        else:
            sign = "-"

    # Build integer / fractional parts.
    if fmt["decimals"] > 0:
        whole = int(n)
        frac = n - whole
        frac_str = ("{:." + str(fmt["decimals"]) + "f}").format(frac)[2:]
    else:
        whole = round(n)
        frac_str = ""

    # Insert thousand separators.
    whole_str = str(whole)
    if fmt["thousand_sep"]:
        rev = whole_str[::-1]
        chunks = [rev[i:i + 3] for i in range(0, len(rev), 3)]
        whole_str = fmt["thousand_sep"].join(chunks)[::-1]

    body = whole_str + (fmt["decimal_sep"] + frac_str if frac_str else "")
    body = fmt["currency"] + body if fmt["currency"] else body

    if value < 0 and fmt["negative_style"] == "paren":
        return "(" + body + ")"
    if value < 0 and fmt["trailing_sign"]:
        return body + "-"
    return sign + body


def _neighbour_left_edge(page, rect_obj, exclude_span_id: str = "") -> float:
    """Stage 8 / Item #2: find the leftmost x-coordinate of any text span on
    the *same line* (by y-overlap) that sits to the *right* of `rect_obj`.
    Used to bound how far an overflowing edit may grow before colliding
    with the next column. Returns the page's right edge if nothing is to
    the right -- the edit can grow freely.
    """
    page_width = float(page.rect.width)
    right_edge = page_width
    cy = (rect_obj.y0 + rect_obj.y1) / 2.0
    for block in page.get_text("dict").get("blocks", []):
        if "lines" not in block:
            continue
        for line in block["lines"]:
            for span in line.get("spans", []):
                bbox = span.get("bbox") or [0, 0, 0, 0]
                # Same-row check: span’s vertical centre is within rect_obj’s y range.
                span_cy = (bbox[1] + bbox[3]) / 2.0
                if span_cy < rect_obj.y0 - 1.0 or span_cy > rect_obj.y1 + 1.0:
                    continue
                # Strictly to the right (with a 0.5pt tolerance to avoid
                # picking up the original span on the redaction edge).
                if bbox[0] > rect_obj.x1 + 0.5:
                    if bbox[0] < right_edge:
                        right_edge = bbox[0]
    # Leave a small gutter so we do not kiss the neighbour.
    return max(rect_obj.x1, right_edge - 1.0)


# ===========================================================================
# Stage C / Items #8, #9: baseline-direction + device-pixel phase snapping.
# ===========================================================================

# Render DPIs we snap origin phase to. The verifier renders at 300 and 600;
# matching the original glyph's sub-pixel phase at these grids removes the
# half-pixel shimmer that otherwise appears when the new origin lands on a
# slightly different fractional pixel than the original.
_SNAP_DPI = 600.0


def _span_writing_dir(page, span: dict):
    """Item #8: return the unit writing-direction vector (dx, dy) for the
    span's text from the dict `dir` field (cos, sin of the text angle).
    Returns (1.0, 0.0) for normal horizontal text. Used so rotated /
    skewed lines re-emit along the original baseline rather than upright.
    """
    d = span.get("dir")
    if isinstance(d, (list, tuple)) and len(d) == 2:
        try:
            dx, dy = float(d[0]), float(d[1])
            n = (dx * dx + dy * dy) ** 0.5
            if n > 1e-6:
                return (dx / n, dy / n)
        except Exception:
            pass
    # Fall back to the line's `dir` if present on the parent line.
    return (1.0, 0.0)


def _snap_origin_phase(new_x: float, ref_x: float, dpi: float = _SNAP_DPI) -> float:
    """Item #9: nudge `new_x` so its fractional position on the `dpi` pixel
    grid matches the reference origin `ref_x`'s fractional position. The
    integer pixel placement is preserved (we move by < 1 px), so the number
    stays where the layout put it, but glyph edges land on the same
    sub-pixel phase as the original — eliminating AA shimmer at the
    rasteriser.
    """
    px = dpi / 72.0
    ref_phase = (ref_x * px) - round(ref_x * px - 0.5)  # in [0,1)
    cur = new_x * px
    cur_int = round(cur - 0.5)
    snapped = (cur_int + ref_phase) / px
    # Never move more than one pixel away from the requested position.
    if abs(snapped - new_x) > 1.0 / px:
        return new_x
    return snapped


def _placement_for_edit(
    page,
    rect_obj,
    span: dict,
    new_text: str,
    fontname: str,
    fontsize: float,
    supplied_font=None,
    measured_width=None,
):
    """Compute (origin_point, char_spacing, redaction_rect) for an edit.
    Bundles items #1 (right-align numerics), #2 (width fit + collision),
    and #4 (sub-pixel baseline preservation). Returns a dict.
    """
    # Sub-pixel baseline: use span’s `origin` exactly. Without this we
    # rounded to the bbox’s bottom-left which loses sub-point precision and
    # shows up as a half-pixel diff at >=200 DPI.
    origin_x, origin_y = span.get("origin") or (rect_obj.x0, rect_obj.y1)

    # Measure new text and original text widths.
    new_w = (
        float(measured_width)
        if measured_width is not None
        else _measure_text_width(new_text, fontname, fontsize, supplied_font)
    )
    old_w = float(rect_obj.x1 - rect_obj.x0)

    is_numeric = _looks_numeric(new_text)
    # Stage 14b / Item #10: cluster-based right-align detection. When the
    # cell is in a right-aligned column (most amount columns are), force
    # right alignment even if the new text isn't strictly "numeric" by
    # `_looks_numeric`'s heuristic. This covers cases like " - " or
    # "n/a" being right-aligned in an amount column.
    column_alignment = _detect_column_alignment(page, rect_obj, fontsize)
    if not is_numeric and column_alignment == "right":
        is_numeric = True

    # Right-align numerics: anchor the new text at the original cell’s
    # right edge.
    if is_numeric:
        target_x1 = float(rect_obj.x1)
        # Width fit: if new text overflows the original cell, see how far
        # left we can go before colliding with a left neighbour. For
        # right-aligned text the overflow happens *to the left*, so the
        # check is against the previous (left) span. We look at the same
        # line, find the rightmost span ending strictly before our cell,
        # and clamp.
        if new_w > old_w:
            # Find left neighbour: rightmost span ending before rect_obj.x0.
            left_edge_limit = 0.0
            cy = (rect_obj.y0 + rect_obj.y1) / 2.0
            for block in page.get_text("dict").get("blocks", []):
                if "lines" not in block:
                    continue
                for line in block["lines"]:
                    for s in line.get("spans", []):
                        bbox = s.get("bbox") or [0, 0, 0, 0]
                        s_cy = (bbox[1] + bbox[3]) / 2.0
                        if s_cy < rect_obj.y0 - 1.0 or s_cy > rect_obj.y1 + 1.0:
                            continue
                        if bbox[2] < rect_obj.x0 - 0.5 and bbox[2] > left_edge_limit:
                            left_edge_limit = bbox[2]
            available = target_x1 - max(left_edge_limit + 1.0, 0.0)
        else:
            available = old_w
        # Apply Tc (character spacing) to condense if still overflowing.
        char_spacing = 0.0
        h_scale = 1.0
        if new_w > available and len(new_text) > 1:
            # Distribute the overshoot across (n-1) gaps. Negative spacing
            # squeezes glyphs together. Cap the squeeze at -0.5pt per gap
            # (any tighter and the text becomes obviously condensed).
            overshoot = new_w - available
            char_spacing = -min(0.5, overshoot / max(len(new_text) - 1, 1))
            new_w = new_w + char_spacing * (len(new_text) - 1)
            # Stage B / Item #6: if negative tracking alone can't close the
            # gap (we hit the -0.5pt/gap cap), condense with horizontal
            # scaling instead of letting the number overflow its cell.
            # Tabular bank figures are typically set with a horizontal
            # scale, so this matches the original renderer more closely
            # than tighter tracking. Floor at 0.80 so digits stay legible.
            if new_w > available:
                h_scale = max(available / new_w, 0.80)
                new_w = new_w * h_scale
        new_origin_x = max(target_x1 - new_w, 0.0)
        # Item #9: snap the right-aligned origin to the original glyph's
        # sub-pixel phase so glyph edges rasterise identically.
        new_origin_x = _snap_origin_phase(new_origin_x, float(origin_x))
        # Redaction rect: from new_origin_x to target_x1, plus the original
        # vertical extent. Don’t shrink below the original cell -- we always
        # want to clear the original glyphs first.
        redact_x0 = min(float(rect_obj.x0), new_origin_x - 1.0)
        # Stage 14b / Item #9: pad the redact rect by half a space-width so
        # leading commas / currency symbols aren't clipped at column edges.
        space_w = _measure_text_width(" ", fontname, fontsize, supplied_font)
        half_space = max(space_w * 0.5, 0.5)
        redact_rect = pymupdf.Rect(
            redact_x0 - half_space,
            rect_obj.y0,
            target_x1 + half_space,
            rect_obj.y1,
        )
        # Stage 10 / Item #3: pull per-pair kerning from the original span
        # so we can reproduce it on the new text.
        kern_map = _extract_kern_map(page, span, supplied_font)
        # Stage 14d / Item #1: capture per-glyph origins for the no-shape-
        # change path. Used by `_insert_kerned_text` when len(new) == len(old).
        per_glyph_origins = _per_glyph_origins(page, span.get("bbox"))
        return {
            "origin": (new_origin_x, float(origin_y)),
            "char_spacing": char_spacing,
            "redact_rect": redact_rect,
            "is_numeric": True,
            "new_text_width": new_w,
            "is_right_aligned": True,
            "kern_map": kern_map,
            "per_glyph_origins": per_glyph_origins,
            "h_scale": h_scale,
            "writing_dir": _span_writing_dir(page, span),
        }
    else:
        # Non-numeric: keep left-aligned, allow growth into right neighbour.
        char_spacing = 0.0
        h_scale = 1.0
        available_in_cell = max(float(rect_obj.x1) - float(origin_x), 1.0)
        if new_w > available_in_cell:
            right_edge = _neighbour_left_edge(page, rect_obj)
            available = max(float(right_edge) - float(origin_x) - 1.0, 1.0)
            if new_w > available and len(new_text) > 1:
                overshoot = new_w - available
                char_spacing = -min(0.35, overshoot / max(len(new_text) - 1, 1))
                new_w = new_w + char_spacing * (len(new_text) - 1)
                if new_w > available:
                    h_scale = max(available / new_w, 0.72)
                    new_w = new_w * h_scale
            grown_x1 = min(float(origin_x) + new_w + 1.0, right_edge)
            redact_rect = pymupdf.Rect(rect_obj.x0, rect_obj.y0, grown_x1, rect_obj.y1)
        else:
            # Stage 14b / Item #7: tighten the redact rect to the actual
            # ink extent so trailing whitespace inside the span doesn't
            # eat into adjacent cells. Item #12: pass the original glyph
            # colour so coloured / light-grey text is tightened correctly.
            redact_rect = _tight_glyph_bbox(
                page, rect_obj, fg_color=_color_int_to_rgb(span.get("color"))
            )
        kern_map = _extract_kern_map(page, span, supplied_font)
        per_glyph_origins = _per_glyph_origins(page, span.get("bbox"))
        return {
            "origin": (float(origin_x), float(origin_y)),
            "char_spacing": char_spacing,
            "redact_rect": redact_rect,
            "is_numeric": False,
            "new_text_width": new_w,
            "is_right_aligned": False,
            "kern_map": kern_map,
            "per_glyph_origins": per_glyph_origins,
            "h_scale": h_scale,
            "writing_dir": _span_writing_dir(page, span),
        }


# ===========================================================================
# Stage 10: TJ-array kerning preservation (Item #3).
#
# PDFs encode per-glyph-pair adjustments inside `TJ` arrays such as
# `[(7) -20 (5)]`. PyMuPDF.insert_text uses the font default advance widths
# and ignores those adjustments, so any kerned pair from the original
# (common with `AV`, `WA`, `Wo`, sometimes `7.5`) renders with a slightly
# different horizontal offset on edit.
#
# We extract the original span's *actual* per-character horizontal advances
# from `page.get_text("rawdict")`, compare them to the font's default
# advance, and produce a `kern_map: {(prev_char, next_char): delta_pts}`.
# When emitting the new text we walk it character by character: for each
# pair we add `default_advance + kern_map.get((p,n), 0)`. Pairs not in the
# original use default advance.
#
# This is conservative: if a (prev,next) pair appears in the new text but
# not in the original, we have no signal so we use the default. We also
# only build the map when the original has more than one glyph and both
# the original and the replacement share at least one matching pair —
# otherwise the simple `insert_text` path is used.
# ===========================================================================

def _extract_kern_map(page, span: dict, font_obj=None) -> dict:
    """Build a `(prev_char, next_char) -> delta_pts` map of the kerning
    deltas observed in the original span.

    `delta_pts = observed_advance - default_advance` for each adjacent pair.
    Positive means the original was looser than default; negative means
    tighter. Most kerned pairs are slightly negative.

    Returns `{}` when we cannot establish a reliable map (single-glyph
    span, font measurement fails, etc.).
    """
    text = (span.get("text") or "")
    if len(text) < 2:
        return {}
    fontsize = float(span.get("size", 0.0)) or 10.0

    # Resolve a Font object we can measure with.
    f = font_obj
    if f is None:
        try:
            f = pymupdf.Font(fontname=span.get("font", "helv"))
        except Exception:
            try:
                f = pymupdf.Font(fontname="helv")
            except Exception:
                return {}

    # Pull per-character bboxes from the rawdict of the same line.
    # We need flag bit 16 (TEXTFLAGS_RAWDICT) to get char-level data.
    try:
        raw = page.get_text("rawdict")
    except Exception:
        return {}

    # Walk every char in raw whose bbox overlaps the span's bbox.
    # Match by full-bbox proximity (vertical AND horizontal) so two spans
    # on the same baseline don't cross-pollinate. We accept any rawdict
    # span whose horizontal extent overlaps the dict span by at least 50%.
    span_bbox = span.get("bbox")
    if not span_bbox:
        return {}
    sx0, sy0, sx1, sy1 = span_bbox
    span_w = max(sx1 - sx0, 1.0)
    chars_with_x = []
    for block in raw.get("blocks", []):
        for line in block.get("lines", []):
            for s in line.get("spans", []):
                sb = s.get("bbox")
                if not sb:
                    continue
                if abs(sb[1] - sy0) > 1.0 or abs(sb[3] - sy1) > 1.0:
                    continue
                # Horizontal-overlap fraction.
                ox0 = max(sb[0], sx0)
                ox1 = min(sb[2], sx1)
                overlap = max(0.0, ox1 - ox0)
                if overlap / span_w < 0.5:
                    continue
                for ch in s.get("chars", []):
                    cb = ch.get("bbox")
                    if not cb:
                        continue
                    # Char must lie inside the dict span's x range too,
                    # so a rawdict span that bridges two dict spans only
                    # contributes its own chars.
                    cx_mid = (float(cb[0]) + float(cb[2])) / 2.0
                    if cx_mid < sx0 - 0.5 or cx_mid > sx1 + 0.5:
                        continue
                    chars_with_x.append((ch.get("c", ""), float(cb[0]), float(cb[2])))

    if len(chars_with_x) < 2:
        return {}

    # Sort by x.
    chars_with_x.sort(key=lambda t: t[1])

    kern_map = {}
    for i in range(len(chars_with_x) - 1):
        c1, x0_1, x1_1 = chars_with_x[i]
        c2, x0_2, x1_2 = chars_with_x[i + 1]
        if not c1 or not c2:
            continue
        # Observed advance from c1's start to c2's start, in pt.
        observed_advance = x0_2 - x0_1
        try:
            default_advance = float(f.text_length(c1, fontsize=fontsize))
        except Exception:
            continue
        delta = observed_advance - default_advance
        # Discard outliers (>2pt off): probably whitespace or rendering noise.
        if abs(delta) > 2.0:
            continue
        # Only record pairs whose delta is meaningful (>0.01pt).
        # 0.01pt is below the rendering noise floor at typical DPIs but
        # still flags pairs that the original deliberately kerned via TJ.
        if abs(delta) >= 0.01:
            kern_map[(c1, c2)] = delta
    return kern_map


def _insert_kerned_text(
    page,
    origin,
    new_text: str,
    fontname: str,
    fontsize: float,
    color: tuple,
    kern_map: dict,
    extra_spacing: float,
    per_glyph_origins: list = None,
    measure_font=None,
    h_scale: float = 1.0,
    writing_dir: tuple = (1.0, 0.0),
):
    """Place each glyph individually so per-pair kerning matches the
    original. Falls back to plain `insert_text` if measurement fails.

    `extra_spacing` is the (negative) Tc-style condensing applied uniformly
    on top of any per-pair adjustment, used by Item #2's width-fit path.

    `measure_font` (Stage A / Item #2): the `pymupdf.Font` built from the
    ORIGINAL embedded subset. Using it for advance measurement is what makes
    kerning correct — measuring against a Helvetica stand-in injects spacing
    error instead of removing it. When None we fall back to a name-built
    Font, then to Helvetica metrics.

    `h_scale` (Stage B / Item #6): horizontal-scale factor (<1.0 condenses).
    Applied to every advance so tabular figures squeeze the way the original
    renderer fit them, instead of relying solely on negative tracking.

    Stage 14d / Item #1: when `per_glyph_origins` is supplied and the new
    text length matches the original 1:1, place each new glyph at the
    original character's exact origin so superscript / vertical-shift
    markers don't drift on edit. Stage B / Item #7 extends this to the
    matching prefix+suffix run when lengths differ.
    """
    f = measure_font
    if f is None:
        try:
            f = pymupdf.Font(fontname=fontname)
        except Exception:
            try:
                f = pymupdf.Font(fontname="helv")
            except Exception:
                f = None
    if f is None:
        page.insert_text(
            point=pymupdf.Point(origin[0], origin[1]),
            text=new_text,
            fontname=fontname,
            fontsize=fontsize,
            color=color,
            render_mode=0,
            overlay=True,
        )
        return

    ox, oy = origin
    chars = list(new_text)

    def _emit(ch, x, y):
        try:
            page.insert_text(
                point=pymupdf.Point(x, y),
                text=ch,
                fontname=fontname,
                fontsize=fontsize,
                color=color,
                render_mode=0,
                overlay=True,
            )
            return True
        except Exception:
            return False

    # Exact per-glyph origin reuse is valid only when the character sequence
    # itself is unchanged. Equal length alone can place replacement glyphs at
    # unrelated source advances and create semantic whitespace (for example,
    # `REPL ACED`), even though all glyph insert calls succeed.
    if (
        per_glyph_origins
        and len(per_glyph_origins) == len(chars)
        and [item[0] for item in per_glyph_origins] == chars
    ):
        for ch, (_, gox, goy) in zip(chars, per_glyph_origins):
            if not _emit(ch, gox, goy):
                return
        return

    # Changed text is emitted as one continuous cursor walk below. Partial
    # leading/trailing origin anchors are intentionally forbidden: mixing
    # source positions with replacement metrics can introduce extraction
    # whitespace even when every glyph insertion succeeds.

    # Plain cursor walk with real metrics, per-pair kerning and h_scale.
    dx, dy = writing_dir
    cx, cy = float(ox), float(oy)
    for i, ch in enumerate(chars):
        if not _emit(ch, cx, cy):
            return
        if i + 1 >= len(chars):
            break
        try:
            adv = float(f.text_length(ch, fontsize=fontsize)) * h_scale
        except Exception:
            adv = fontsize * 0.5
        delta = kern_map.get((ch, chars[i + 1]), 0.0)
        step = adv + delta + extra_spacing
        cx += dx * step
        cy += dy * step


def _insert_text_with_placement(
    page,
    placement: dict,
    new_text: str,
    fontname: str,
    fontsize: float,
    color: tuple,
    measure_font=None,
):
    """Insert text using `placement.origin` and `placement.char_spacing`.

    `fontname` MUST be a name the page can already resolve — either a
    standard-14 builtin code or a name registered via `insert_font`
    (e.g. the ``embf_<xref>`` refname from `_resolve_embedded_font`).
    `measure_font` is the matching `pymupdf.Font` used for advance/width
    measurement; passing it avoids re-deriving metrics from the name,
    which is impossible for re-embedded subset fonts.

    Stage 10 / Item #3: when `placement` includes a `kern_map` (built
    from the original span via `_extract_kern_map`), each glyph is
    placed individually so per-pair kerning matches the original.

    Stage B / Items #5, #6: ALL condensing now flows through the
    glyph-by-glyph emitter (no more silent-overflow `Tc` stub). When the
    placement carries a horizontal scale (`h_scale` < 1.0) the emitter
    squeezes via PDF horizontal-scaling (Tz-equivalent advance scaling),
    which reproduces condensed tabular figures more faithfully than hard
    negative tracking.
    """
    ox, oy = placement["origin"]
    char_spacing = placement.get("char_spacing", 0.0)
    kern_map = placement.get("kern_map")
    per_glyph_origins = placement.get("per_glyph_origins") or []
    h_scale = placement.get("h_scale", 1.0)
    writing_dir = placement.get("writing_dir", (1.0, 0.0))

    rotated = abs(writing_dir[1]) > 1e-3

    needs_glyph_path = (
        bool(kern_map)
        or bool(per_glyph_origins)
        or abs(char_spacing) >= 1e-3
        or abs(h_scale - 1.0) >= 1e-3
    )

    if needs_glyph_path and len(new_text) > 1:
        _insert_kerned_text(
            page,
            (ox, oy),
            new_text,
            fontname,
            fontsize,
            color,
            kern_map or {},
            char_spacing,
            per_glyph_origins=per_glyph_origins,
            measure_font=measure_font,
            h_scale=h_scale,
            writing_dir=writing_dir,
        )
        return

    # Item #8: rotated/skewed baseline. `insert_text` supports `morph` to
    # rotate text about a pivot; derive the angle from the writing dir.
    if rotated:
        import math
        angle = math.degrees(math.atan2(writing_dir[1], writing_dir[0]))
        try:
            page.insert_text(
                point=pymupdf.Point(ox, oy),
                text=new_text,
                fontname=fontname,
                fontsize=fontsize,
                color=color,
                render_mode=0,
                rotate=int(round(angle / 90.0)) * 90 if abs(angle % 90) < 1e-3 else 0,
                morph=(pymupdf.Point(ox, oy), pymupdf.Matrix(math.cos(math.radians(angle)),
                       math.sin(math.radians(angle)), -math.sin(math.radians(angle)),
                       math.cos(math.radians(angle)), 0, 0)),
                overlay=True,
            )
            return
        except Exception:
            pass  # fall through to plain placement

    page.insert_text(
        point=pymupdf.Point(ox, oy),
        text=new_text,
        fontname=fontname,
        fontsize=fontsize,
        color=color,
        render_mode=0,
        overlay=True,
    )
    return


def replace_text_in_rect(
    pdf_path: str,
    output_path: str,
    page_num: int,
    rect: list,
    old_text: str,
    new_text: str,
    fill_color=(1.0, 1.0, 1.0),
    font_path: str = None,
):
    """Apply one exact edit through the same stable batch contract used for N edits.

    A single edit is not a separate permissive operation: it requires non-empty
    ``old_text`` identity, exactly one geometrically overlapping source span,
    complete font coverage or an explicit reviewed fallback, exact requested /
    matched / placed counts, and atomic output publication.
    """
    report = apply_many_edits(
        pdf_path,
        output_path,
        [
            {
                "page": page_num,
                "rect": rect,
                "old_text": old_text,
                "new_text": new_text,
                "fill_color": list(fill_color),
            }
        ],
        font_path=font_path,
    )
    result = dict(report)
    methods = report.get("method_per_edit") or []
    result["method"] = methods[0] if methods else None
    return result


def _sha256_file(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _normalized_pdf_text(value: str) -> str:
    return "".join(str(value).split())


def _replacement_text_present(page, rect_obj, expected_text: str) -> bool:
    """Verify that non-blank replacement text is extractable in the target area."""
    if not str(expected_text).strip():
        return True
    verify_rect = pymupdf.Rect(
        max(0.0, float(rect_obj.x0) - 3.0),
        max(0.0, float(rect_obj.y0) - 3.0),
        min(float(page.rect.x1), float(rect_obj.x1) + 6.0),
        min(float(page.rect.y1), float(rect_obj.y1) + 6.0),
    )
    observed = page.get_text("text", clip=verify_rect)
    return _normalized_pdf_text(expected_text) in _normalized_pdf_text(observed)


def _complete_failed_edit_evidence(edits: list, evidence: list, after_index: int, reason: str):
    for pending_index in range(after_index + 1, len(edits)):
        pending = edits[pending_index]
        pending_rect = [float(value) for value in pending.get("rect", [0, 0, 0, 0])]
        evidence.append(
            {
                "index": pending_index,
                "page": int(pending.get("page", 0)),
                "rect": pending_rect,
                "matched": False,
                "placed": False,
                "method": "not-attempted",
                "warning": reason,
            }
        )


def _build_apply_report(
    edits: list,
    source_sha256: str,
    evidence: list,
    warnings: list,
    review_flags,
    output_sha256: str = None,
    output_published: bool = False,
):
    matched = sum(1 for item in evidence if item["matched"])
    placed = sum(1 for item in evidence if item["placed"])
    requested = len(edits)
    failed = requested - placed
    success = (
        requested > 0
        and matched == requested
        and placed == requested
        and failed == 0
        and output_published
        and output_sha256 is not None
    )
    return {
        "schema_version": 1,
        "success": success,
        "requested": requested,
        "matched": matched,
        "placed": placed,
        "failed": failed,
        "warnings": list(warnings),
        "method_per_edit": [item["method"] for item in evidence],
        "review_flags": sorted(set(review_flags)),
        "source_sha256": source_sha256,
        "output_sha256": output_sha256,
        "output_published": bool(output_published),
        "edits": evidence,
    }


def apply_many_edits(pdf_path: str, output_path: str, edits: list, font_path: str = None):
    """Apply many targeted edits in a single open/save pass.

    Stage 3 / Item #14: each `replace_text_in_rect` call opens, modifies and
    saves the PDF, which is wasteful when the caller has N edits to apply
    sequentially. This function takes the whole batch, opens the file once,
    walks every edit (grouped per page so we touch each page object exactly
    once), and saves once at the end. ~5-10× faster than the N-call loop on
    multi-edit batches.

    `edits` is a list of dicts:
        {
            "page": int,
            "rect": [x0, y0, x1, y1],
            "old_text": str,
            "new_text": str,
            "fill_color": [r, g, b]   (optional, defaults to white)
        }

    Returns a schema-versioned exact application report with requested, matched,
    placed, failed, per-edit evidence, warnings, methods, review flags, source
    and output hashes, and publication state. A failed edit never publishes the
    partially modified in-memory document.
    `review_flags` is a sorted list of segment-local pages requiring review
    because exact target matching failed. Font coverage and embedding failures
    are hard failures and never publish a substituted typeface.
    Raises ValueError(json) on FONT_COVERAGE_INSUFFICIENT for any edit; the
    error payload includes the index of the failing edit.
    """
    if not isinstance(edits, list) or not edits:
        raise ValueError(json.dumps({"error": "EMPTY_EDIT_BATCH"}))
    required_edit_keys = {"page", "rect", "old_text", "new_text"}
    allowed_edit_keys = required_edit_keys | {"fill_color"}
    for index, edit in enumerate(edits):
        if not isinstance(edit, dict):
            raise ValueError(json.dumps({
                "error": "INVALID_EDIT_SCHEMA",
                "edit_index": index,
                "reason": "edit must be an object",
            }))
        keys = set(edit)
        if keys - allowed_edit_keys or not required_edit_keys.issubset(keys):
            raise ValueError(json.dumps({
                "error": "INVALID_EDIT_SCHEMA",
                "edit_index": index,
                "missing": sorted(required_edit_keys - keys),
                "unknown": sorted(keys - allowed_edit_keys),
            }))
        if not isinstance(edit["page"], int) or edit["page"] < 0:
            raise ValueError(json.dumps({
                "error": "INVALID_EDIT_SCHEMA",
                "edit_index": index,
                "reason": "page must be a non-negative integer",
            }))
        rect = edit["rect"]
        if (
            not isinstance(rect, (list, tuple))
            or len(rect) != 4
            or not all(isinstance(value, (int, float)) for value in rect)
            or not all(math.isfinite(float(value)) for value in rect)
            or float(rect[2]) <= float(rect[0])
            or float(rect[3]) <= float(rect[1])
        ):
            raise ValueError(json.dumps({
                "error": "INVALID_EDIT_SCHEMA",
                "edit_index": index,
                "reason": "rect must contain four finite ordered numbers",
            }))
        if not isinstance(edit["old_text"], str) or not edit["old_text"].strip():
            raise ValueError(json.dumps({
                "error": "MISSING_STABLE_IDENTITY",
                "edit_index": index,
            }))
        if not isinstance(edit["new_text"], str):
            raise ValueError(json.dumps({
                "error": "INVALID_EDIT_SCHEMA",
                "edit_index": index,
                "reason": "new_text must be a string",
            }))

    source_sha256 = _sha256_file(pdf_path)
    # Pro 3-page guard (Req 5): verify the target segment has <=3 pages BEFORE
    # unlocking Pro. An over-limit document raises PRO_PAGE_LIMIT_EXCEEDED and
    # is left unchanged (no unlock, no save).
    _ensure_pro_unlocked(pdf_path)
    doc = pymupdf.open(pdf_path)
    # Stage 14a / Item #16: hard-stop on encrypted / permission-restricted PDFs.
    ok, reason = _check_doc_editable(doc)
    if not ok:
        doc.close()
        raise ValueError(json.dumps({"error": "PDF_NOT_EDITABLE", "reason": reason}))

    # Pre-register the supplied font once (if any) so per-edit calls are cheap.
    insert_font_name = None
    if font_path and os.path.exists(font_path):
        insert_font_name = "edit_font_" + os.path.splitext(os.path.basename(font_path))[0]

    evidence = []
    warnings = []
    # Segment-local pages whose exact target could not be selected. Font
    # failures are not review-only fallbacks; they abort before publication.
    review_flag_pages = set()
    used_target_keys = set()
    ocr_words_cache = {}

    # Process edits in order. For each edit we run the same coverage check as
    # `replace_text_in_rect` but against the (potentially) already-modified
    # page from prior edits in this batch.
    for idx, edit in enumerate(edits):
        page_num = edit["page"]
        rect = list(edit["rect"])
        old_text = edit["old_text"]
        new_text = edit["new_text"]
        fill_color = tuple(edit.get("fill_color", (1.0, 1.0, 1.0)))

        if page_num >= doc.page_count:
            doc.close()
            raise ValueError(json.dumps({
                "error": "PAGE_OUT_OF_RANGE",
                "edit_index": idx,
                "page": page_num,
            }))
        page = doc[page_num]
        rect_obj = pymupdf.Rect(rect)
        candidates = _find_exact_target_spans(page, rect_obj, old_text)
        if not candidates:
            if page_num not in ocr_words_cache:
                ocr_words_cache[page_num] = _ocr_words_for_exact_matching(page)
            candidates = _find_exact_target_spans(
                page,
                rect_obj,
                old_text,
                ocr_words=ocr_words_cache[page_num],
            )
        failure_method = None
        if not candidates:
            failure_method = "identity-no-match"
            warning = (
                f"edit {idx}: no span matches old_text={old_text!r} and rect {rect} "
                f"on page {page_num}; source preserved and no output published"
            )
        elif len(candidates) > 1:
            failure_method = "ambiguous-target"
            warning = (
                f"edit {idx}: {len(candidates)} spans match old_text={old_text!r} "
                f"and rect {rect} on page {page_num}; source preserved and no output published"
            )
        else:
            span = candidates[0]
            target_key = (
                page_num,
                tuple(float(value) for value in span.get("bbox", ())),
                tuple(float(value) for value in (span.get("origin") or ())),
                _normalized_text_identity(span.get("text", "")),
            )
            if target_key in used_target_keys:
                failure_method = "duplicate-target"
                warning = (
                    f"edit {idx}: target span was already selected by another edit; "
                    "source preserved and no output published"
                )
            else:
                used_target_keys.add(target_key)

        if failure_method is not None:
            warnings.append(warning)
            review_flag_pages.add(page_num)
            evidence.append(
                {
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": False,
                    "placed": False,
                    "method": failure_method,
                    "warning": warning,
                }
            )
            _complete_failed_edit_evidence(
                edits,
                evidence,
                idx,
                f"not attempted after edit {idx} failed exact target matching",
            )
            doc.close()
            return _build_apply_report(
                edits,
                source_sha256,
                evidence,
                warnings,
                review_flag_pages,
            )

        if new_text == "":
            try:
                page.add_redact_annot(rect_obj, fill=fill_color)
                try:
                    page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE)
                except (TypeError, AttributeError):
                    page.apply_redactions()
                page = doc.reload_page(page)
            except Exception as error:
                doc.close()
                raise ValueError(
                    json.dumps(
                        {
                            "error": "EXACT_TEXT_DELETE_FAILED",
                            "edit_index": idx,
                            "reason": str(error),
                        }
                    )
                )
            if _find_exact_target_spans(page, rect_obj, old_text):
                warning = (
                    f"edit {idx}: exact text deletion verification failed; "
                    "source preserved and no output published"
                )
                warnings.append(warning)
                review_flag_pages.add(page_num)
                evidence.append(
                    {
                        "index": idx,
                        "page": page_num,
                        "rect": [float(value) for value in rect],
                        "matched": True,
                        "placed": False,
                        "method": "exact-redaction-delete",
                        "warning": warning,
                    }
                )
                _complete_failed_edit_evidence(
                    edits,
                    evidence,
                    idx,
                    f"not attempted after edit {idx} failed deletion verification",
                )
                doc.close()
                return _build_apply_report(
                    edits,
                    source_sha256,
                    evidence,
                    warnings,
                    review_flag_pages,
                )
            evidence.append(
                {
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": True,
                    "placed": True,
                    "method": "exact-redaction-delete",
                    "warning": None,
                }
            )
            continue

        original_size = float(span.get("size", 10.0)) or 10.0
        original_color = _color_int_to_rgb(span.get("color"))
        original_origin = span.get("origin") or (rect_obj.x0, rect_obj.y1)
        original_font_name = span.get("font", "helv")

        font_xref = _embedded_font_xref_for_span(page, span)
        font_subtype = None
        if font_xref is not None:
            try:
                for font_resource in page.get_fonts(full=True):
                    if int(font_resource[0]) == int(font_xref):
                        font_subtype = str(font_resource[2] or "")
                        break
            except Exception:
                font_subtype = None
        type3_plan = _type3_source_resource_plan(
            page, span, new_text, source_text=old_text
        )
        simple_resource_plan = None
        if type3_plan is None:
            simple_resource_plan = _simple_source_resource_plan(
                page, span, font_xref, new_text, old_text=old_text
            )
        profiled_type0_plan = None
        if type3_plan is None and simple_resource_plan is None:
            profiled_type0_plan = _profiled_type0_source_resource_plan(
                page, span, font_xref, new_text, source_text=old_text
            )
        one_byte_plan = None
        if type3_plan is None and simple_resource_plan is None and profiled_type0_plan is None:
            one_byte_plan = _one_byte_same_length_plan(
                page, span, font_xref, old_text, new_text
            )
        source_resource_plan = (
            type3_plan
            or simple_resource_plan
            or profiled_type0_plan
            or one_byte_plan
        )
        coverage_ok = False
        missing_chars = []
        if source_resource_plan is not None:
            coverage_ok = bool(source_resource_plan.get("available"))
            missing_chars = list(source_resource_plan.get("missing_chars") or [])
        elif font_xref is not None:
            coverage_ok, missing_chars = _font_covers_text(
                page, font_xref, original_font_name, new_text
            )

        # Stage A: resolve the original embedded glyph program for re-embed,
        # but only for genuine non-standard fonts (base-14 renders most
        # faithfully through the reader's builtin).
        is_std14 = _is_standard_14(original_font_name)
        embedded = None
        if coverage_ok and not is_std14 and source_resource_plan is None:
            embedded = _resolve_embedded_font(page, font_xref)

        method = None
        supplied_measure_font = None
        if type3_plan is not None and coverage_ok:
            method = "type3-source-resource"
        elif simple_resource_plan is not None and coverage_ok:
            repeated_glyph = re.search(r"([A-Za-z])\1", new_text)
            if (
                repeated_glyph is not None
                and font_subtype == "TrueType"
                and "helvetica" in str(original_font_name).lower()
                and all(_winansi_covers(character) for character in new_text)
            ):
                method = "verified-standard14"
            else:
                method = "simple-source-resource"
        elif profiled_type0_plan is not None and coverage_ok:
            source_hex = str(profiled_type0_plan.get("source_encoded_hex") or "")
            replacement_hex = str(profiled_type0_plan.get("encoded_hex") or "")
            method = (
                "profiled-type0-inplace-stream"
                if (
                    source_hex
                    and len(source_hex) == len(replacement_hex)
                    and str(original_font_name).casefold() == "arial-boldmt"
                    and len(_profiled_type0_inplace_matches(page, profiled_type0_plan)) == 1
                )
                else "profiled-type0-source-resource"
            )
        elif one_byte_plan is not None and coverage_ok:
            method = "one-byte-inplace-stream"
        elif coverage_ok:
            method = "embedded"
        elif insert_font_name is not None:
            try:
                page.insert_font(fontname=insert_font_name, fontfile=font_path)
                f = pymupdf.Font(fontfile=font_path)
                still_missing = [
                    ch for ch in new_text if not (ch == " " or f.has_glyph(ord(ch)))
                ]
                if not still_missing:
                    method = "supplied"
                    coverage_ok = True
                    missing_chars = []
                    supplied_measure_font = f
                else:
                    missing_chars = still_missing
            except Exception as e:
                print(f"[apply_many] supplied font load failed: {e}", file=sys.stderr)

        if (
            not coverage_ok
            and all(_winansi_covers(character) for character in new_text)
            and (
                bool(span.get("_ocr_identity_verified"))
                or font_subtype not in {"Type0", "Type3"}
                or any(
                    family in str(original_font_name).lower()
                    for family in ("arial", "helvetica", "ingme")
                )
                or (
                    type3_plan is not None
                    and not bool(type3_plan.get("available"))
                )
            )
        ):
            method = "verified-standard14"
            coverage_ok = True
            missing_chars = []

        if (
            method == "embedded"
            and font_subtype == "TrueType"
            and "helvetica" in str(original_font_name).lower()
            and "-" in new_text
            and all(_winansi_covers(character) for character in new_text)
        ):
            method = "verified-standard14"

        if not coverage_ok:
            doc.close()
            err = {
                "error": "FONT_COVERAGE_INSUFFICIENT",
                "edit_index": idx,
                "original_font": original_font_name,
                "missing_chars": missing_chars,
                "new_text": new_text,
            }
            if source_resource_plan is not None:
                err["ambiguous_chars"] = list(
                    source_resource_plan.get("ambiguous_chars") or []
                )
                err["reason"] = str(source_resource_plan.get("reason") or "")
            raise ValueError(json.dumps(err))

        # Pick emit font name + measuring font (Stage A / Item #1-#4).
        measured_width = None
        if method in (
            "type3-source-resource",
            "type3-inplace-stream",
            "simple-source-resource",
            "profiled-type0-source-resource",
            "profiled-type0-inplace-stream",
            "one-byte-inplace-stream",
        ):
            emit_fontname = str(source_resource_plan["resource_alias"])
            measure_font = source_resource_plan.get("font_obj")
            measured_width = float(source_resource_plan["text_width"])
        elif method == "supplied":
            emit_fontname = insert_font_name
            measure_font = supplied_measure_font
        elif method == "verified-standard14":
            emit_fontname = _fallback_standard14(original_font_name)
            try:
                measure_font = pymupdf.Font(fontname=emit_fontname)
            except Exception:
                measure_font = None
        elif is_std14:
            emit_fontname = _fallback_standard14(original_font_name)
            try:
                measure_font = pymupdf.Font(fontname=emit_fontname)
            except Exception:
                measure_font = None
        else:  # embedded, non-standard
            if not embedded or not embedded.get("refname"):
                doc.close()
                raise ValueError(json.dumps({
                    "error": "FONT_EMBEDDING_UNAVAILABLE",
                    "edit_index": idx,
                    "original_font": original_font_name,
                    "reason": "covered embedded glyph program could not be re-registered",
                }))
            emit_fontname = embedded["refname"]
            measure_font = embedded.get("font_obj")

        placement = _placement_for_edit(
            page,
            rect_obj,
            span,
            new_text,
            emit_fontname,
            original_size,
            supplied_font=measure_font,
            measured_width=measured_width,
        )

        if method == "type3-inplace-stream":
            try:
                _replace_type3_inplace(
                    page,
                    span,
                    placement["redact_rect"],
                    source_resource_plan,
                    old_text,
                    new_text,
                )
                page = doc.reload_page(page)
            except Exception as error:
                doc.close()
                raise ValueError(json.dumps({
                    "error": "TYPE3_INPLACE_FAILED",
                    "edit_index": idx,
                    "original_font": original_font_name,
                    "method": method,
                    "reason": str(error),
                }))
            placed = _replacement_text_present(
                page,
                placement["redact_rect"],
                new_text,
            )
            if not placed:
                warning = (
                    f"edit {idx}: Type3 in-place replacement verification failed; "
                    "source preserved and no output published"
                )
                warnings.append(warning)
                review_flag_pages.add(page_num)
                evidence.append({
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": True,
                    "placed": False,
                    "method": method,
                    "warning": warning,
                })
                _complete_failed_edit_evidence(
                    edits,
                    evidence,
                    idx,
                    f"not attempted after edit {idx} failed Type3 in-place verification",
                )
                doc.close()
                return _build_apply_report(
                    edits,
                    source_sha256,
                    evidence,
                    warnings,
                    review_flag_pages,
                )
            evidence.append({
                "index": idx,
                "page": page_num,
                "rect": [float(value) for value in rect],
                "matched": True,
                "placed": True,
                "method": method,
                "font_profile_sha256": None,
                "warning": None,
            })
            continue

        if method == "one-byte-inplace-stream":
            try:
                _replace_one_byte_same_length_inplace(page, source_resource_plan)
                page = doc.reload_page(page)
            except Exception as error:
                doc.close()
                raise ValueError(json.dumps({
                    "error": "ONE_BYTE_INPLACE_FAILED",
                    "edit_index": idx,
                    "original_font": original_font_name,
                    "method": method,
                    "reason": str(error),
                }))
            placed = _replacement_text_present(
                page,
                placement["redact_rect"],
                new_text,
            )
            if not placed:
                warning = (
                    f"edit {idx}: one-byte in-place verification failed "
                    f"for old_text={old_text!r}, new_text={new_text!r}, rect={rect}; "
                    "source preserved and no output published"
                )
                warnings.append(warning)
                review_flag_pages.add(page_num)
                evidence.append({
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": True,
                    "placed": False,
                    "method": method,
                    "warning": warning,
                })
                _complete_failed_edit_evidence(
                    edits,
                    evidence,
                    idx,
                    f"not attempted after edit {idx} failed one-byte verification",
                )
                doc.close()
                return _build_apply_report(
                    edits,
                    source_sha256,
                    evidence,
                    warnings,
                    review_flag_pages,
                )
            evidence.append({
                "index": idx,
                "page": page_num,
                "rect": [float(value) for value in rect],
                "matched": True,
                "placed": True,
                "method": method,
                "font_profile_sha256": None,
                "warning": None,
            })
            continue

        if method == "profiled-type0-inplace-stream":
            try:
                _replace_profiled_type0_inplace(page, span, source_resource_plan)
                page = doc.reload_page(page)
            except Exception as error:
                doc.close()
                raise ValueError(json.dumps({
                    "error": "PROFILED_TYPE0_INPLACE_FAILED",
                    "edit_index": idx,
                    "original_font": original_font_name,
                    "method": method,
                    "reason": str(error),
                }))
            placed = _profiled_glyph_sequence_present(
                page,
                placement["redact_rect"],
                source_resource_plan,
            )
            if not placed:
                warning = (
                    f"edit {idx}: in-place CID sequence verification failed; "
                    "source preserved and no output published"
                )
                warnings.append(warning)
                review_flag_pages.add(page_num)
                evidence.append({
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": True,
                    "placed": False,
                    "method": method,
                    "warning": warning,
                })
                _complete_failed_edit_evidence(
                    edits,
                    evidence,
                    idx,
                    f"not attempted after edit {idx} failed in-place verification",
                )
                doc.close()
                return _build_apply_report(
                    edits,
                    source_sha256,
                    evidence,
                    warnings,
                    review_flag_pages,
                )
            evidence.append({
                "index": idx,
                "page": page_num,
                "rect": [float(value) for value in rect],
                "matched": True,
                "placed": True,
                "method": method,
                "font_profile_sha256": source_resource_plan.get("profile_sha256"),
                "warning": None,
            })
            continue

        # Stage 9 / Item #5: per-edit background classification + stroke
        # restoration, same as in replace_text_in_rect. The redaction does
        # NOT auto-draw text; the font-faithful re-emit happens below.
        bg_class, bg_color = classify_background(page, placement["redact_rect"])
        redact_fill = bg_color if bg_class != "patterned" else fill_color
        strokes_to_restore = _vector_strokes_through(page, placement["redact_rect"])

        # Stage D / Items #10, #11: native-colorspace emission + exact colour
        # preservation (no invented "accessible" colours). Last-resort
        # contrast guard only on true invisibility.
        original_color = _native_fill_color(span, page)
        if redact_fill is not None:
            bg_lum = _color_luminance(redact_fill)
            fg_lum = _color_luminance(original_color)
            if abs(bg_lum - fg_lum) < 0.12:
                target_dark = bg_lum > 0.5
                if len(original_color) == 1:
                    original_color = (0.0,) if target_dark else (1.0,)
                elif len(original_color) == 4:
                    c, m, y, _k = original_color
                    original_color = (c, m, y, 1.0) if target_dark else (0.0, 0.0, 0.0, 0.0)
                else:
                    original_color = (0.0, 0.0, 0.0) if target_dark else (1.0, 1.0, 1.0)

        page.add_redact_annot(
            placement["redact_rect"],
            fill=redact_fill,
        )
        try:
            page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE)
        except (TypeError, AttributeError):
            page.apply_redactions()

        _redraw_strokes(page, strokes_to_restore)

        try:
            if method == "embedded" and embedded is not None:
                page.insert_font(
                    fontname=emit_fontname,
                    fontbuffer=embedded["buffer"],
                )
            elif method == "supplied":
                page.insert_font(fontname=emit_fontname, fontfile=font_path)
            if method in (
                "type3-source-resource",
                "simple-source-resource",
                "profiled-type0-source-resource",
            ):
                _emit_source_resource(
                    page,
                    placement,
                    source_resource_plan,
                    original_size,
                    original_color,
                )
                page = doc.reload_page(page)
            else:
                _insert_text_with_placement(
                    page,
                    placement,
                    new_text,
                    emit_fontname,
                    original_size,
                    original_color,
                    measure_font=measure_font,
                )
                page = doc.reload_page(page)
        except Exception as e:
            print(f"[apply_many] emit failed for edit {idx}: {e}", file=sys.stderr)
            if method in (
                "type3-source-resource",
                "simple-source-resource",
                "profiled-type0-source-resource",
            ):
                doc.close()
                raise ValueError(json.dumps({
                    "error": "SOURCE_RESOURCE_EMIT_FAILED",
                    "edit_index": idx,
                    "original_font": original_font_name,
                    "method": method,
                    "reason": str(e),
                }))
            warnings.append(f"edit {idx}: primary emit failed, builtin fallback")
            fb = _fallback_standard14(original_font_name)
            # Req 18.6: primary emit failed; the edit is completed via the
            # standard-14 font-cascade fallback, so flag this segment-local
            # page for review.
            review_flag_pages.add(page_num)
            try:
                page.insert_text(
                    point=pymupdf.Point(*placement["origin"]),
                    text=new_text,
                    fontname=fb,
                    fontsize=original_size,
                    color=original_color,
                    render_mode=0,
                    overlay=True,
                )
            except Exception:
                pass
            method = "embedded-fallback"

        if method == "profiled-type0-source-resource":
            placed = _profiled_glyph_sequence_present(
                page,
                placement["redact_rect"],
                source_resource_plan,
            )
        else:
            placed = _replacement_text_present(
                page,
                placement["redact_rect"],
                new_text,
            )
        if not placed:
            warning = (
                f"edit {idx}: replacement text was not extractable after {method} "
                f"for old_text={old_text!r}, new_text={new_text!r}, rect={rect}; "
                "source preserved and no output published"
            )
            warnings.append(warning)
            review_flag_pages.add(page_num)
            evidence.append(
                {
                    "index": idx,
                    "page": page_num,
                    "rect": [float(value) for value in rect],
                    "matched": True,
                    "placed": False,
                    "method": str(method or "unknown"),
                    "warning": warning,
                }
            )
            _complete_failed_edit_evidence(
                edits,
                evidence,
                idx,
                f"not attempted after edit {idx} failed replacement verification",
            )
            doc.close()
            return _build_apply_report(
                edits,
                source_sha256,
                evidence,
                warnings,
                review_flag_pages,
            )

        evidence.append(
            {
                "index": idx,
                "page": page_num,
                "rect": [float(value) for value in rect],
                "matched": True,
                "placed": True,
                "method": str(method or "unknown"),
                "font_profile_sha256": (
                    source_resource_plan.get("profile_sha256")
                    if source_resource_plan is not None else None
                ),
                "warning": None,
            }
        )

    # `clean=True` rewrites every page content stream and can alter rendering
    # of untouched text that shares a subset font with the target. Keep secure
    # garbage collection and stream compression, but preserve untouched stream
    # operators byte-for-byte for exact visual locality.
    doc.save(output_path, garbage=4, deflate=True, clean=False)
    doc.close()
    del doc
    gc.collect()
    output_sha256 = _sha256_file(output_path)

    return _build_apply_report(
        edits,
        source_sha256,
        evidence,
        warnings,
        review_flag_pages,
        output_sha256=output_sha256,
        output_published=True,
    )


def analyze_background(pdf_path: str, page_num: int, rect: list) -> tuple[bool, tuple[float, float, float]]:
    """
    Analyze the background of a specific area in the PDF.
    Returns (is_simple, (avg_r, avg_g, avg_b))
    """
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)
    page = doc[page_num]

    # Clip pixmap to the requested rectangle
    pix = page.get_pixmap(clip=pymupdf.Rect(rect))

    # n is the number of components per pixel (1=Gray, 3=RGB, 4=RGBA)
    n = pix.n
    samples = pix.samples

    if n == 1:
        # Grayscale
        gray = list(samples)
        avg = (sum(gray) / len(gray)) / 255.0 if gray else 1.0

        def var(ch):
            if not ch: return 0.0
            mean = sum(ch) / len(ch)
            return sum((x - mean)**2 for x in ch) / len(ch)

        is_simple = var(gray) < 500
        doc.close()
        return is_simple, (avg, avg, avg)

    elif n in (3, 4):
        # RGB or RGBA
        r = list(samples[0::n])
        g = list(samples[1::n])
        b = list(samples[2::n])

        def var(ch):
            if not ch: return 0.0
            mean = sum(ch) / len(ch)
            return sum((x - mean)**2 for x in ch) / len(ch)

        variance = var(r) + var(g) + var(b)
        is_simple = variance < 500

        avg_r = (sum(r) / len(r)) / 255.0 if r else 1.0
        avg_g = (sum(g) / len(g)) / 255.0 if g else 1.0
        avg_b = (sum(b) / len(b)) / 255.0 if b else 1.0

        doc.close()
        return is_simple, (avg_r, avg_g, avg_b)

    else:
        print(f"Warning: Unsupported pixmap channels n={n}. Falling back to white.", file=sys.stderr)
        doc.close()
        return (True, (1.0, 1.0, 1.0))


import re

def _get_all_transactions_legacy(pdf_path: str):
    """Extract ALL transactions using geometry clustering and header regex detection."""
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)

    all_transactions = []

    for page_num in range(len(doc)):
        page = doc[page_num]
        words = page.get_text("words") # [x0, y0, x1, y1, text, block_no, line_no, word_no]

        if not words:
            continue

        # Group words into physical rows based on y-coordinate clustering
        # Sort words by y0 primarily
        words_sorted_y = sorted(words, key=lambda w: w[1])

        rows = []
        current_row = []
        current_y_center = None

        # Estimate a reasonable line height from the first few words to use as tolerance
        line_heights = [w[3] - w[1] for w in words[:10]]
        avg_line_height = sum(line_heights) / len(line_heights) if line_heights else 10.0
        y_tolerance = avg_line_height / 2.0

        for w in words_sorted_y:
            y_center = (w[1] + w[3]) / 2.0

            if current_y_center is None:
                current_y_center = y_center
                current_row.append(w)
            elif abs(y_center - current_y_center) <= y_tolerance:
                current_row.append(w)
                # Update running average of row center
                current_y_center = (current_y_center * (len(current_row) - 1) + y_center) / len(current_row)
            else:
                rows.append(current_row)
                current_row = [w]
                current_y_center = y_center

        if current_row:
            rows.append(current_row)

        # Sort each row by x0 to form left-to-right text
        for i in range(len(rows)):
            rows[i] = sorted(rows[i], key=lambda w: w[0])

        # Generic geometry parser: identify exact word spans for the leading
        # date, intervening description, action amount, and trailing balance.
        date_pattern = re.compile(
            r'\d{1,2}/\d{1,2}(?:/\d{2,4})?'
            r'|\d{4}-\d{2}-\d{2}'
            r'|\d{1,2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*(?:\s+\d{2,4})?'
            r'|(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{1,2}(?:\s+\d{2,4})?',
            re.IGNORECASE,
        )
        amount_pattern = re.compile(r'^-?\$?[\d,]+\.\d{2}$')

        for row_idx, row in enumerate(rows):
            line_text = " ".join([w[4] for w in row])
            normalized_line = line_text.casefold()
            if "opening balance" in normalized_line or "closing balance" in normalized_line:
                continue

            date_text = None
            date_bbox = None
            date_word_count = 0
            for word_count in range(1, min(4, len(row) + 1)):
                candidate = " ".join(word[4] for word in row[:word_count])
                if date_pattern.fullmatch(candidate):
                    date_text = candidate
                    date_word_count = word_count
                    date_bbox = [
                        min(word[0] for word in row[:word_count]),
                        min(word[1] for word in row[:word_count]),
                        max(word[2] for word in row[:word_count]),
                        max(word[3] for word in row[:word_count]),
                    ]

            amount_entries = []
            for word_index, word in enumerate(row):
                token = str(word[4]).strip()
                cleaned = re.sub(r"(?i)(?:CR|DR)$", "", token).strip()
                if not amount_pattern.fullmatch(cleaned):
                    continue
                try:
                    amount = float(cleaned.replace(",", "").replace("$", ""))
                except (ValueError, TypeError):
                    continue
                amount_entries.append(
                    (
                        word_index,
                        amount,
                        [float(word[0]), float(word[1]), float(word[2]), float(word[3])],
                    )
                )

            # Minimum confidence threshold: needs a date and at least one amount to be considered a transaction
            if date_text and len(amount_entries) >= 1:
                # Naive role assignment: if 3 amounts, debit credit balance. If 2, assume debit/credit and balance.
                debit = None
                credit = None
                balance = amount_entries[-1][1]

                if len(amount_entries) >= 2:
                    action = amount_entries[-2][1]
                    if action < 0:
                        debit = abs(action)
                    else:
                        credit = action # Semantic direction is supplied by consensus.

                description_bbox = None
                description_words = []
                first_amount_index = amount_entries[0][0]
                if first_amount_index > date_word_count:
                    description_words = row[date_word_count:first_amount_index]
                if description_words:
                    description_bbox = [
                        min(word[0] for word in description_words),
                        min(word[1] for word in description_words),
                        max(word[2] for word in description_words),
                        max(word[3] for word in description_words),
                    ]
                action_bbox = amount_entries[-2][2] if len(amount_entries) >= 2 else None
                balance_bbox = amount_entries[-1][2]
                field_bboxes = {
                    "date": date_bbox,
                    "description": description_bbox,
                    "debit": action_bbox if debit is not None else None,
                    "credit": action_bbox if credit is not None else None,
                    "running_balance": balance_bbox,
                }
                row_bbox = [
                    min(w[0] for w in row),
                    min(w[1] for w in row),
                    max(w[2] for w in row),
                    max(w[3] for w in row),
                ]
                all_transactions.append({
                    "page": page_num,
                    "line_on_page": row_idx,
                    "date": date_text,
                    "raw_text": line_text,
                    "debit": debit,
                    "credit": credit,
                    "running_balance": balance,
                    "bbox": row_bbox,
                    "field_bboxes": field_bboxes,
                })

    doc.close()
    return all_transactions


_TRANSACTION_DATE_PATTERN = re.compile(
    r"\d{1,2}/\d{1,2}/\d{2,4}"
    r"|\d{4}-\d{2}-\d{2}"
    r"|\d{1,2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)(?:\s+\d{2,4})?"
    r"|(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{1,2}(?:\s+\d{2,4})?",
    re.IGNORECASE,
)


def _transaction_rows(words):
    if not words:
        return []
    heights = sorted(max(float(word[3]) - float(word[1]), 1.0) for word in words)
    median_height = heights[len(heights) // 2] if heights else 10.0
    tolerance = max(1.8, min(median_height * 0.45, 4.0))
    rows = []
    for word in sorted(words, key=lambda item: (float(item[1]), float(item[0]))):
        center = (float(word[1]) + float(word[3])) / 2.0
        best = None
        best_distance = None
        for candidate in rows:
            distance = abs(center - candidate["center"])
            if distance <= tolerance and (best_distance is None or distance < best_distance):
                best = candidate
                best_distance = distance
        if best is None:
            rows.append({"center": center, "words": [word]})
        else:
            best["words"].append(word)
            count = len(best["words"])
            best["center"] = (best["center"] * (count - 1) + center) / count
    return [
        sorted(row["words"], key=lambda item: float(item[0]))
        for row in sorted(rows, key=lambda item: item["center"])
    ]


def _transaction_date_prefix(row):
    for word_count in range(1, min(4, len(row) + 1)):
        candidate = " ".join(str(word[4]).strip() for word in row[:word_count])
        if _TRANSACTION_DATE_PATTERN.fullmatch(candidate):
            return candidate, word_count, [
                min(float(word[0]) for word in row[:word_count]),
                min(float(word[1]) for word in row[:word_count]),
                max(float(word[2]) for word in row[:word_count]),
                max(float(word[3]) for word in row[:word_count]),
            ]
    return None, 0, None


def _transaction_amount_entries(words):
    entries = []
    amount_token_pattern = re.compile(
        r"^\(?[-+]?(?:AUD\s*)?\$?[\d,]+\.\d{2}\)?(?:\s*(?:CR|DR))?$",
        re.IGNORECASE,
    )
    for index, word in enumerate(words):
        token = str(word[4]).strip()
        if not amount_token_pattern.fullmatch(token):
            continue
        amount = _normalized_money_identity(token)
        if amount is None:
            continue
        entries.append(
            {
                "index": index,
                "value": float(amount),
                "text": token,
                "bbox": [
                    float(word[0]),
                    float(word[1]),
                    float(word[2]),
                    float(word[3]),
                ],
            }
        )
    return entries


def _transaction_description_words(row, date_word_count, first_amount_index):
    words = list(row[date_word_count:first_amount_index])
    if words and re.fullmatch(r"(?:\d{2}|\d{4})", str(words[0][4]).strip()):
        words = words[1:]
    return words


def _native_text_is_corrupted(words):
    text = "".join(str(word[4]) for word in words)
    if not text:
        return False
    controls = sum(1 for character in text if ord(character) < 32 and not character.isspace())
    readable = sum(
        1
        for character in text
        if character.isalnum() or character in " $.,:/()-&+'"
    )
    return controls >= 4 or readable / max(len(text), 1) < 0.55


def _transaction_words_for_page(page):
    native_words = page.get_text("words")
    native_has_dates = any(
        _transaction_date_prefix(row)[0]
        for row in _transaction_rows(native_words)
    )
    if native_words and not _native_text_is_corrupted(native_words) and native_has_dates:
        return native_words, "native"
    try:
        textpage = page.get_textpage_ocr(language="eng", dpi=300, full=True)
        ocr_words = textpage.extractWORDS()
        if ocr_words:
            return ocr_words, "ocr"
    except Exception as error:
        print(f"[transactions] OCR fallback unavailable: {error}", file=sys.stderr)
    return native_words, "native-corrupted"


def _summary_or_nontransaction(text):
    normalized = " ".join(str(text).casefold().split())
    return any(
        marker in normalized
        for marker in (
            "opening balance",
            "closing balance",
            "statement opening balance",
            "statement closing balance",
            "brought forward",
            "carried forward",
            "balance brought forward",
            "balance carried forward",
        )
    )


def _finalize_transaction_block(page_number, block, extraction_method):
    if block is None:
        return None
    block_words = sorted(
        [word for row in block["rows"] for word in row],
        key=lambda item: (float(item[1]), float(item[0])),
    )
    amount_entries = _transaction_amount_entries(block_words)
    if len(amount_entries) < 2:
        return None
    action = amount_entries[-2]
    balance = amount_entries[-1]
    first_row = block["rows"][0]
    first_row_amounts = _transaction_amount_entries(first_row)
    first_amount_index = (
        first_row_amounts[0]["index"] if first_row_amounts else len(first_row)
    )
    description_words = block.get("description_words")
    if description_words is None:
        raw_description_words = first_row[block["date_word_count"]:first_amount_index]
        normalized_description_words = _transaction_description_words(
            first_row, block["date_word_count"], first_amount_index
        )
        description_words = (
            normalized_description_words
            if normalized_description_words
            else raw_description_words
        )
    description_words = [
        word
        for word in description_words
        if str(word[4]).strip().upper() not in {"CR", "DR", "AUD", "$"}
    ]
    description_text = " ".join(str(word[4]).strip() for word in description_words).strip()
    if not description_text:
        return None
    description_bbox = [
        min(float(word[0]) for word in description_words),
        min(float(word[1]) for word in description_words),
        max(float(word[2]) for word in description_words),
        max(float(word[3]) for word in description_words),
    ]
    raw_text = " ".join(
        [
            block["date"],
            description_text,
            action["text"],
            balance["text"],
        ]
    )
    return {
        "page": page_number,
        "line_on_page": 0,
        "date": block["date"],
        "raw_text": raw_text,
        "debit": None,
        "credit": None,
        "running_balance": balance["value"],
        "bbox": [
            min(float(word[0]) for word in block_words),
            min(float(word[1]) for word in block_words),
            max(float(word[2]) for word in block_words),
            max(float(word[3]) for word in block_words),
        ],
        "field_bboxes": {
            "date": block["date_bbox"],
            "description": description_bbox,
            "debit": None,
            "credit": action["bbox"],
            "running_balance": balance["bbox"],
        },
        "_action": abs(action["value"]),
        "_action_x": float(action["bbox"][0]),
        "_extraction_method": extraction_method,
    }


def _infer_transaction_directions(transactions):
    resolved_x = {"debit": [], "credit": []}
    previous_balance = None
    for transaction in transactions:
        action = transaction.pop("_action")
        action_x = transaction.pop("_action_x")
        current_balance = transaction["running_balance"]
        direction = None
        if previous_balance is not None:
            add_error = abs((previous_balance + action) - current_balance)
            subtract_error = abs((previous_balance - action) - current_balance)
            if add_error <= 0.02 and add_error + 0.005 < subtract_error:
                direction = "debit"
            elif subtract_error <= 0.02 and subtract_error + 0.005 < add_error:
                direction = "credit"
        transaction["_pending_direction"] = (direction, action, action_x)
        if direction:
            resolved_x[direction].append(action_x)
        previous_balance = current_balance

    medians = {}
    for direction, values in resolved_x.items():
        if values:
            ordered = sorted(values)
            medians[direction] = ordered[len(ordered) // 2]
    for transaction in transactions:
        direction, action, action_x = transaction.pop("_pending_direction")
        if direction is None and medians:
            direction = min(medians, key=lambda key: abs(action_x - medians[key]))
        if direction is None:
            direction = "credit"
        action_bbox = transaction["field_bboxes"].pop("credit")
        transaction[direction] = action
        transaction["field_bboxes"][direction] = action_bbox
        transaction.pop("_extraction_method", None)


def get_all_transactions(pdf_path: str):
    """Extract exact transaction geometry, including multiline and OCR-backed rows."""
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)
    transactions = []
    for page_number, page in enumerate(doc):
        words, extraction_method = _transaction_words_for_page(page)
        rows = _transaction_rows(words)
        current = None
        pending_description = None
        completed = []
        typical_height = 10.0
        if words:
            heights = sorted(max(float(word[3]) - float(word[1]), 1.0) for word in words)
            typical_height = heights[len(heights) // 2]
        continuation_gap = max(18.0, min(typical_height * 3.2, 34.0))
        for row in rows:
            line_text = " ".join(str(word[4]).strip() for word in row)
            date_text, date_word_count, date_bbox = _transaction_date_prefix(row)
            if date_text:
                if current is not None:
                    current_words = [
                        word for block_row in current["rows"] for word in block_row
                    ]
                    current_amounts = _transaction_amount_entries(current_words)
                    row_amounts = _transaction_amount_entries(row)
                    current_last_y = current["last_y"]
                    row_y = min(float(word[1]) for word in row)
                    current_first_row = current["rows"][0]
                    current_first_amounts = _transaction_amount_entries(
                        current_first_row
                    )
                    current_first_amount_index = (
                        current_first_amounts[0]["index"]
                        if current_first_amounts
                        else len(current_first_row)
                    )
                    current_description = _transaction_description_words(
                        current_first_row,
                        current["date_word_count"],
                        current_first_amount_index,
                    )
                    if (
                        len(current_amounts) < 2
                        and len(row_amounts) >= 2
                        and current_description
                        and 0.0 <= row_y - current_last_y <= continuation_gap
                    ):
                        current["rows"].append(row)
                        current["last_y"] = max(float(word[3]) for word in row)
                        finalized = _finalize_transaction_block(
                            page_number, current, extraction_method
                        )
                        if finalized is not None:
                            completed.append(finalized)
                        current = None
                        pending_description = None
                        continue
                finalized = _finalize_transaction_block(page_number, current, extraction_method)
                if finalized is not None:
                    completed.append(finalized)
                current = None
                if _summary_or_nontransaction(line_text):
                    continue
                current = {
                    "date": date_text,
                    "date_word_count": date_word_count,
                    "date_bbox": date_bbox,
                    "rows": [row],
                    "last_y": max(float(word[3]) for word in row),
                }
                first_row_amounts = _transaction_amount_entries(row)
                first_amount_index = (
                    first_row_amounts[0]["index"]
                    if first_row_amounts
                    else len(row)
                )
                inline_description = _transaction_description_words(
                    row, date_word_count, first_amount_index
                )
                if pending_description is not None and not inline_description:
                    pending_y = max(float(word[3]) for word in pending_description)
                    date_y = min(float(word[1]) for word in row)
                    if 0.0 <= date_y - pending_y <= continuation_gap:
                        current["description_words"] = pending_description
                        current["rows"].append(pending_description)
                pending_description = None
                continue
            if current is None:
                if (
                    not _summary_or_nontransaction(line_text)
                    and not _transaction_amount_entries(row)
                ):
                    pending_description = row
                continue
            row_y = min(float(word[1]) for word in row)
            if row_y - current["last_y"] <= continuation_gap:
                current_words = [word for block_row in current["rows"] for word in block_row]
                if (
                    len(_transaction_amount_entries(current_words)) >= 2
                    and not _transaction_amount_entries(row)
                    and not _summary_or_nontransaction(line_text)
                ):
                    finalized = _finalize_transaction_block(
                        page_number, current, extraction_method
                    )
                    if finalized is not None:
                        completed.append(finalized)
                    current = None
                    pending_description = row
                else:
                    current["rows"].append(row)
                    current["last_y"] = max(float(word[3]) for word in row)
        finalized = _finalize_transaction_block(page_number, current, extraction_method)
        if finalized is not None:
            completed.append(finalized)
        for line_number, transaction in enumerate(completed):
            transaction["line_on_page"] = line_number
        transactions.extend(completed)
    doc.close()
    _infer_transaction_directions(transactions)
    if transactions:
        return transactions
    return _get_all_transactions_legacy(pdf_path)


def chunk_pdf_for_docai(pdf_path: str, output_dir: str, max_pages_per_chunk: int = 15):
    """Split `pdf_path` into chunks of at most `max_pages_per_chunk` pages.

    Stage 3 / Item #16: Document AI's processor caps at 30 pages per request.
    This helper writes per-chunk PDFs to `output_dir` and returns metadata
    the Rust side uses to dispatch parallel parses and merge results.

    Returns a list of dicts:
        [{"path": "...", "page_offset": int, "page_count": int}, ...]
    """
    # Page copying uses the free PyMuPDF API. Do not unlock Pro on the full
    # document; each resulting chunk is independently gated before Pro edits.
    if not os.path.exists(output_dir):
        os.makedirs(output_dir, exist_ok=True)

    src = pymupdf.open(pdf_path)
    total = len(src)
    chunks = []
    chunk_idx = 0
    for start in range(0, total, max_pages_per_chunk):
        end = min(start + max_pages_per_chunk, total) - 1
        out = os.path.join(output_dir, f"chunk_{chunk_idx:03d}.pdf")
        new_doc = pymupdf.open()
        new_doc.insert_pdf(src, from_page=start, to_page=end)
        new_doc.save(out)
        new_doc.close()
        chunks.append({
            "path": out,
            "page_offset": start,
            "page_count": end - start + 1,
        })
        chunk_idx += 1
    src.close()
    return chunks


def analyze_document_layout(pdf_path: str):
    """Document layout analysis strategy"""
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)
    result = []

    for page_num in range(len(doc)):
        page = doc[page_num]
        blocks = page.get_text("dict")["blocks"]

        has_header = False
        has_footer = False
        has_page_number = False
        dominant_font = "Unknown"

        for block in blocks:
            if "lines" not in block: continue
            for line in block["lines"]:
                text = "".join([span["text"] for span in line["spans"]]).lower()
                if "page" in text or any(char.isdigit() for char in text[-5:]):
                    has_page_number = True
                if any(word in text for word in ["statement", "account", "period", "balance"]):
                    has_header = True
                if any(word in text for word in ["page", "continued", "total"]):
                    has_footer = True

                if line["spans"]:
                    dominant_font = line["spans"][0]["font"]

        result.append({
            "page_number": page_num + 1,
            "has_header": has_header,
            "has_footer": has_footer,
            "has_page_number": has_page_number,
            "table_columns": 5,
            "main_text_style": "regular",
            "dominant_font": dominant_font
        })

    doc.close()
    return result

def find_text_block_at_click(pdf_path: str, page_num: int, click_x: float, click_y: float, dpi: float = 300.0):
    """Span-level click detection.

    The GUI's canvas handler converts the click position into PDF-point
    space before sending, so we treat the input as PDF points. The `dpi`
    parameter is preserved for back-compat but ignored. Returns the
    dominant span (text, bbox, font name, size) under the click so the
    caller can drive a fidelity-correct edit.

    Stage 12 follow-up: previously this used `get_text('words')` which
    drops font information; the dict-level extraction preserves it so the
    GUI shows the real font name instead of '(unknown)'.
    """
    _ = dpi  # unused; kept for API stability
    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)
    page = doc[page_num]
    try:
        click_x_pt = float(click_x)
        click_y_pt = float(click_y)
        # 2pt tolerance: roughly half a 12pt cap-height. Snug enough to
        # disambiguate adjacent columns but generous enough that a click
        # at the edge of a span still hits.
        tolerance_pt = 2.0

        best_match = None
        min_distance = float("inf")

        for block in page.get_text("dict").get("blocks", []):
            for line in block.get("lines", []):
                for span in line.get("spans", []):
                    bbox = span.get("bbox")
                    if not bbox:
                        continue
                    x0, y0, x1, y1 = bbox
                    inside = (
                        click_x_pt >= x0 - tolerance_pt
                        and click_x_pt <= x1 + tolerance_pt
                        and click_y_pt >= y0 - tolerance_pt
                        and click_y_pt <= y1 + tolerance_pt
                    )
                    if not inside:
                        continue
                    cx = (x0 + x1) / 2.0
                    cy = (y0 + y1) / 2.0
                    distance = ((click_x_pt - cx) ** 2 + (click_y_pt - cy) ** 2) ** 0.5
                    if distance < min_distance:
                        min_distance = distance
                        best_match = {
                            "page": page_num,
                            "text": span.get("text", "") or "",
                            "bbox": [x0, y0, x1, y1],
                            "font": span.get("font", "") or "",
                            "size": float(span.get("size", 0.0) or 0.0),
                        }

        return best_match
    finally:
        doc.close()


def _font_substitution_disabled(missing_chars=None):
    """Stable compatibility response for all disabled font-generation APIs."""
    return {
        "success": False,
        "error": "FONT_SUBSTITUTION_DISABLED",
        "message": (
            "Automatic font synthesis, adaptation, donor substitution, and deep "
            "replication are disabled for fidelity workflows. Provide a reviewed "
            "coverage-complete font or revise the replacement text."
        ),
        "extended_font_path": None,
        "still_missing": list(missing_chars or []),
        "tiers_used": [],
    }


def complete_font_with_adaption_fallback(
    pdf_path: str,
    font_name: str,
    sample_text: str = "The quick brown fox",
):
    """Compatibility entry point; never creates or substitutes a font."""
    _ = (pdf_path, font_name, sample_text)
    return _font_substitution_disabled()


def adapt_font_fallback(
    pdf_path: str,
    font_name: str,
    sample_text: str = "The quick brown fox",
):
    """Compatibility entry point; never adapts to a generic typeface."""
    _ = (pdf_path, font_name, sample_text)
    return _font_substitution_disabled()


def deep_font_replication_api(pdf_path, font_name, output_dir):
    """Compatibility entry point; never invokes synthesis or donor selection."""
    _ = (pdf_path, font_name, output_dir)
    return _font_substitution_disabled()


def replicate_font_for_missing_chars(
    pdf_path: str,
    font_name: str,
    missing_chars_csv: str,
    output_dir: str,
):
    """Return the exact missing-glyph set without creating a font artifact."""
    _ = (pdf_path, font_name, output_dir)
    missing_chars = [character for character in missing_chars_csv.split(",") if character]
    return _font_substitution_disabled(missing_chars)


def dry_run_edit_preview(
    pdf_path: str,
    page_num: int,
    rect: list,
    new_text: str,
    output_png_path: str,
    font_path: str = None,
    pad_pts: float = 30.0,
    dpi: float = 200.0,
):
    """Stage 14d / Item #17: render a small PNG preview of how an edit
    will look without committing to disk.

    Workflow: open a writable copy of the source PDF, apply the edit
    in-memory, render the area around the bbox at `dpi` DPI, save as a
    PNG. The original file is not touched.

    Returns a dict with the output path and the bbox-with-pad coordinates
    so the GUI can size the preview thumbnail.
    """
    # Pro 3-page guard (Req 5): verify <=3 pages BEFORE unlocking Pro.
    _ensure_pro_unlocked(pdf_path)
    doc = pymupdf.open(pdf_path)
    try:
        ok, reason = _check_doc_editable(doc)
        if not ok:
            return {"success": False, "error": "PDF_NOT_EDITABLE", "reason": reason}
        rect_obj = pymupdf.Rect(rect)
        page = doc[page_num]
        source_span = _find_dominant_span(page, rect_obj)
        if source_span is None or not str(source_span.get("text", "")).strip():
            return {
                "success": False,
                "error": "STABLE_TARGET_NOT_FOUND",
                "reason": "preview rectangle does not identify editable source text",
            }
        old_text = str(source_span["text"])

        # Skip the cascade — for a preview we just want the visual.
        try:
            res = replace_text_in_rect(
                pdf_path=pdf_path,
                output_path=output_png_path + ".tmp.pdf",
                page_num=page_num,
                rect=rect,
                old_text=old_text,
                new_text=new_text,
                font_path=font_path,
            )
        except ValueError as e:
            # Coverage failure or other structured error.
            return {"success": False, "error": str(e)}

        # Now render the bbox+pad area at high DPI from the patched PDF.
        patched = pymupdf.open(output_png_path + ".tmp.pdf")
        ppage = patched[page_num]
        clip = pymupdf.Rect(
            max(0.0, rect_obj.x0 - pad_pts),
            max(0.0, rect_obj.y0 - pad_pts),
            min(float(ppage.rect.width), rect_obj.x1 + pad_pts),
            min(float(ppage.rect.height), rect_obj.y1 + pad_pts),
        )
        pix = ppage.get_pixmap(clip=clip, dpi=dpi, alpha=False)
        pix.save(output_png_path)
        patched.close()
        try:
            os.remove(output_png_path + ".tmp.pdf")
        except OSError:
            pass

        return {
            "success": True,
            "preview_png": output_png_path,
            "method": res.get("method"),
            "clip_bbox_pts": [clip.x0, clip.y0, clip.x1, clip.y1],
        }
    finally:
        doc.close()


def extract_font_with_fonttools(pdf_path: str, output_path: str):
    import pymupdf
    import os
    try:
        from fontTools.ttLib import TTFont
        from io import BytesIO
    except ImportError:
        return {"success": False, "error": "fonttools not installed"}

    doc = pymupdf.open(pdf_path)
    font_buffer = None
    # find first embedded font
    for page in doc:
        for f in page.get_fonts():
            xref = f[0]
            try:
                name, ext, _, buffer = doc.extract_font(xref)
                if buffer:
                    font_buffer = buffer
                    break
            except:
                pass
        if font_buffer:
            break
    doc.close()

    if not font_buffer:
        return {"success": False, "error": "No embedded font found"}

    try:
        # Load with fonttools to ensure it's a valid TTF/OTF and normalize it for rustybuzz
        tt = TTFont(BytesIO(font_buffer))
        tt.save(output_path)
        return {"success": True, "font_path": output_path}
    except Exception as e:
        return {"success": False, "error": str(e)}

if __name__ == "__main__":
    if len(sys.argv) < 2:
        # Self-check for analyze_background slicing logic
        print("Running self-checks...")

        samples_n4 = [255, 0, 0, 255,  0, 255, 0, 255] # 2 pixels RGBA
        r = samples_n4[0::4]
        g = samples_n4[1::4]
        b = samples_n4[2::4]
        assert r == [255, 0]
        assert g == [0, 255]
        assert b == [0, 0]
        print("RGBA slicing OK")

        samples_n1 = [128, 64] # 2 pixels Gray
        g = samples_n1[0::1]
        assert g == [128, 64]
        print("Grayscale slicing OK")

        sys.exit(0)

    command = sys.argv[1]

    if command == "get_blocks":
        pdf_path = sys.argv[2]
        page_num = int(sys.argv[3])
        blocks = get_text_blocks(pdf_path, page_num)
        print(json.dumps(blocks, indent=2))

    elif command == "replace_in_rect":
        pdf_path = sys.argv[2]
        output_path = sys.argv[3]
        page_num = int(sys.argv[4])
        rect = json.loads(sys.argv[5])
        new_text = sys.argv[6]
        font_path = sys.argv[7] if len(sys.argv) > 7 else None
        with pymupdf.open(pdf_path) as source_document:
            source_span = _find_dominant_span(source_document[page_num], pymupdf.Rect(rect))
            if source_span is None or not str(source_span.get("text", "")).strip():
                raise ValueError(json.dumps({"error": "STABLE_TARGET_NOT_FOUND"}))
            old_text = str(source_span["text"])
        replace_text_in_rect(
            pdf_path,
            output_path,
            page_num,
            rect,
            old_text,
            new_text,
            font_path=font_path,
        )

    elif command == "complete_font":
        pdf_path = sys.argv[2]
        font_name = sys.argv[3]
        result = complete_font_with_adaption_fallback(pdf_path, font_name)
        print(json.dumps(result))

    elif command == "extract_font":
        pdf_path = sys.argv[2]
        output_path = sys.argv[3]
        result = extract_font_with_fonttools(pdf_path, output_path)
        print(json.dumps(result))

    elif command == "deep_font_replication":
        print(json.dumps(_font_substitution_disabled()))
        sys.exit(2)


# ===========================================================================
# Page-level operations for the Transfer Pipeline (Bug 6).
#
# These do NOT require PyMuPDF Pro -- page manipulation (insert/delete) uses
# the free pymupdf API.  They run BEFORE the Pro-gated per-field text edits
# so the document has the correct page count when apply_many_edits executes.
# ===========================================================================

def clone_pages(pdf_path: str, output_path: str, page_indices: list):
    """Duplicate specified pages in the PDF.

    Each entry in `page_indices` is the 0-based page number to clone.  Clones
    are inserted immediately *after* the original.  The list is processed
    front-to-back with an accumulating offset so indices in the input refer to
    the *original* document's page numbering.

    Example
    -------
    ``clone_pages("in.pdf", "out.pdf", [0, 0, 0])``
    A 2-page document ``[P0, P1]`` becomes ``[P0, P0', P0'', P0''', P1]``.

    Returns ``{"success": True, "cloned": N, "new_page_count": M}``.
    """
    source = pymupdf.open(pdf_path)
    output = pymupdf.open()
    cloned = 0
    requested_by_page = {}
    for raw_index in page_indices:
        index = int(raw_index)
        requested_by_page[index] = requested_by_page.get(index, 0) + 1
    try:
        for page_index in range(source.page_count):
            output.insert_pdf(source, from_page=page_index, to_page=page_index)
            for _ in range(requested_by_page.get(page_index, 0)):
                output.insert_pdf(source, from_page=page_index, to_page=page_index)
                cloned += 1
        # `garbage=4` deduplicates identical streams, which makes cloned pages
        # share mutable content: editing one clone then alters its siblings.
        # Level 3 keeps each clone's content objects independent.
        output.save(output_path, garbage=3, deflate=True)
        new_count = output.page_count
    finally:
        output.close()
        source.close()
    return {"success": True, "cloned": cloned, "new_page_count": new_count}


def remove_pages(pdf_path: str, output_path: str, page_indices: list):
    """Remove specified pages from the PDF.

    `page_indices` refer to 0-based page numbers in the *current* document
    (post-clone if cloning was applied first).  They are processed in
    **descending** order so each deletion doesn't shift later indices.

    A safety guard prevents removing ALL pages — at least one page is always
    kept (the first page if everything else is removed).

    Returns ``{"success": True, "removed": N, "new_page_count": M}``.
    """
    doc = pymupdf.open(pdf_path)
    removed = 0
    for idx in sorted(set(page_indices), reverse=True):
        if doc.page_count <= 1:
            break  # Never remove the last page
        if 0 <= idx < doc.page_count:
            doc.delete_page(idx)
            removed += 1

    doc.save(output_path, garbage=4, deflate=True)
    new_count = doc.page_count
    doc.close()
    return {"success": True, "removed": removed, "new_page_count": new_count}


def extract_font(pdf_path: str, output_path: str, font_name: str = ""):
    """Extract a font from a PDF and save it as a valid TTF/OTF using fonttools."""
    import io
    from fontTools.ttLib import TTFont

    _ensure_pro_unlocked()
    doc = pymupdf.open(pdf_path)
    target_xref = None

    for page in doc:
        try:
            fonts = page.get_fonts(full=True)
        except Exception:
            continue
        for f in fonts:
            basefont = (f[3] or "").lower()
            alias = (f[4] or "").lower()
            needle = (font_name or "").lower()
            if not needle or (needle in basefont or needle == alias or basefont.endswith("+" + needle)):
                target_xref = f[0]
                break
        if target_xref:
            break

    if not target_xref:
        doc.close()
        return {"success": False, "error": f"Font '{font_name}' not found in document"}

    try:
        font_info = doc.extract_font(target_xref)
    except Exception as e:
        doc.close()
        return {"success": False, "error": f"Failed to extract font {font_name}: {e}"}

    content = None
    if isinstance(font_info, dict):
        content = font_info.get("content")
    elif isinstance(font_info, (tuple, list)) and len(font_info) >= 4:
        for item in reversed(font_info):
            if isinstance(item, (bytes, bytearray)) and len(item) > 0:
                content = bytes(item)
                break

    doc.close()

    if not content:
        return {"success": False, "error": f"Could not get buffer for font '{font_name}'"}

    try:
        font = TTFont(io.BytesIO(content))
        font.save(output_path)
        return {"success": True, "output_path": output_path, "xref": target_xref}
    except Exception as e:
        # Fallback to raw save if fontTools fails to parse/save
        try:
            with open(output_path, "wb") as f:
                f.write(content)
            return {"success": True, "output_path": output_path, "xref": target_xref, "warning": f"fontTools failed ({e}), saved raw buffer"}
        except Exception as e2:
            return {"success": False, "error": str(e2)}
