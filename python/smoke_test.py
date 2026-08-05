#!/usr/bin/env python3
"""Deterministic executable-base-state smoke test for the production PDF bridge."""

from __future__ import annotations

import base64
import importlib.util
import json
import tempfile
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
BRIDGE_PATH = ROOT / "python" / "pymupdf_pro_integration.py"


def load_bridge():
    spec = importlib.util.spec_from_file_location("pymupdf_pro_integration", BRIDGE_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load bridge from {BRIDGE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    bridge = load_bridge()
    with tempfile.TemporaryDirectory(prefix="bank-statement-python-smoke-") as temp_dir:
        pdf_path = Path(temp_dir) / "smoke.pdf"
        document = pymupdf.open()
        page = document.new_page(width=612, height=792)
        page.insert_text((72, 96), "Bank Statement Fidelity Editor Python Smoke")
        document.save(pdf_path)
        document.close()

        page_count = bridge._count_pages_without_pro_unlock(str(pdf_path))
        if page_count != 1:
            raise AssertionError(f"expected one page, got {page_count!r}")

        rendered = bridge.render_page_to_png(str(pdf_path), page_num=0, dpi=72.0)
        png = base64.b64decode(rendered["png_base64"], validate=True)
        if not png.startswith(b"\x89PNG\r\n\x1a\n"):
            raise AssertionError("rendered payload is not a PNG")
        if rendered["width_pts"] != 612 or rendered["height_pts"] != 792:
            raise AssertionError(f"unexpected page geometry: {rendered}")

        result = {
            "status": "pass",
            "python_bridge": BRIDGE_PATH.name,
            "pymupdf_version": getattr(pymupdf, "__version__", "unknown"),
            "pro_package_available": bool(bridge._PYMUPDF_PRO_AVAILABLE),
            "page_count": page_count,
            "rendered_png_bytes": len(png),
            "width_pts": rendered["width_pts"],
            "height_pts": rendered["height_pts"],
        }
        print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
