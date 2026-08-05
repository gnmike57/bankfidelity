#!/usr/bin/env python3
"""Generate or verify the deterministic packaged Python runtime manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON_DIR = ROOT / "python"
MANIFEST_PATH = PYTHON_DIR / "runtime-manifest.json"
SOURCE_FILES = (
    "bridge_protocol.py",
    "font-resource-profiles.json",
    "pymupdf_pro_integration.py",
    "verify_runtime_manifest.py",
    "verify_runtime_versions.py",
    "worker.py",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def requirements(path: Path, seen: set[Path] | None = None) -> dict[str, str]:
    resolved = path.resolve()
    visited = set() if seen is None else seen
    if resolved in visited:
        raise SystemExit(f"recursive runtime requirements include: {path}")
    visited.add(resolved)
    packages: dict[str, str] = {}
    for raw_line in resolved.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("-r ") or line.startswith("--requirement "):
            include = line.split(maxsplit=1)[1]
            packages.update(requirements(resolved.parent / include, visited))
            continue
        if "==" not in line:
            raise SystemExit(f"runtime requirement must be exactly pinned: {line}")
        name, version = line.split("==", 1)
        if not name or not version:
            raise SystemExit(f"invalid pinned runtime requirement: {line}")
        packages[name] = version
    visited.remove(resolved)
    return dict(sorted(packages.items(), key=lambda item: item[0].lower()))


def build_manifest() -> dict[str, object]:
    sys.path.insert(0, str(PYTHON_DIR))
    from bridge_protocol import PROTOCOL_VERSION  # pylint: disable=import-outside-toplevel

    missing = [name for name in SOURCE_FILES if not (PYTHON_DIR / name).is_file()]
    if missing:
        raise SystemExit(f"runtime source files missing: {', '.join(missing)}")
    return {
        "schema_version": 1,
        "python": {"major": 3, "minor": 12},
        "protocol_version": PROTOCOL_VERSION,
        "entrypoint": "worker.py",
        "packages": {
            "base": requirements(ROOT / "requirements-ci.txt"),
            "pro": requirements(ROOT / "requirements-pro.txt"),
        },
        "source_files": {
            name: sha256(PYTHON_DIR / name) for name in sorted(SOURCE_FILES)
        },
    }


def canonical(manifest: dict[str, object]) -> str:
    return json.dumps(manifest, indent=2, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = canonical(build_manifest())
    if args.check:
        if not MANIFEST_PATH.is_file():
            print(f"missing runtime manifest: {MANIFEST_PATH}", file=sys.stderr)
            return 1
        if MANIFEST_PATH.read_text(encoding="utf-8") != rendered:
            print("Python runtime manifest is stale; regenerate it", file=sys.stderr)
            return 1
        print("Python runtime manifest is current")
        return 0
    MANIFEST_PATH.write_text(rendered, encoding="utf-8", newline="\n")
    print(f"wrote {MANIFEST_PATH.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
