#!/usr/bin/env python3
"""Verify the permanent PyMuPDF runtime uses one exact, supported version pair."""

from __future__ import annotations

import importlib.metadata
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def pinned_version(path: Path, package: str) -> str:
    pattern = re.compile(rf"^{re.escape(package)}==([^\s#]+)$", re.IGNORECASE)
    for line in path.read_text(encoding="utf-8").splitlines():
        match = pattern.match(line.strip())
        if match:
            return match.group(1)
    raise RuntimeError(f"{package} does not have an exact pin in {path.name}")


def main() -> int:
    expected_core = pinned_version(ROOT / "requirements-ci.txt", "PyMuPDF")
    expected_pro = pinned_version(ROOT / "requirements-pro.txt", "PyMuPDFPro")
    installed_core = importlib.metadata.version("PyMuPDF")
    installed_pro = importlib.metadata.version("PyMuPDFPro")
    if expected_core != expected_pro:
        raise RuntimeError(
            f"repository pins are incompatible: PyMuPDF={expected_core}, "
            f"PyMuPDFPro={expected_pro}"
        )
    if installed_core != expected_core or installed_pro != expected_pro:
        raise RuntimeError(
            "installed Python PDF runtime does not match the repository pins: "
            f"expected={expected_core}, core={installed_core}, pro={installed_pro}"
        )

    import pymupdf
    import pymupdf.pro

    pymupdf.pro.unlock()
    print(
        json.dumps(
            {
                "status": "compatible",
                "pymupdf": installed_core,
                "pymupdf_pro": installed_pro,
                "python_api": getattr(pymupdf, "__version__", installed_core),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
