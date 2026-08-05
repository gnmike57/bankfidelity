#!/usr/bin/env python3
"""Verify the Python runtime against the deterministic packaging manifest."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import sys
from pathlib import Path
from typing import Any

PYTHON_DIR = Path(__file__).resolve().parent
MANIFEST_PATH = PYTHON_DIR / "runtime-manifest.json"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class RuntimeManifestError(RuntimeError):
    """The installed Python runtime does not match the packaging manifest."""


def fail(message: str) -> None:
    raise RuntimeManifestError(message)


def load_manifest() -> dict[str, Any]:
    try:
        value = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {MANIFEST_PATH.name}: {type(error).__name__}")
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        fail("unsupported or malformed manifest schema")
    return value


def verify(tier: str) -> dict[str, Any]:
    manifest = load_manifest()
    expected_python = manifest.get("python")
    actual_python = {"major": sys.version_info.major, "minor": sys.version_info.minor}
    if expected_python != actual_python:
        fail(f"Python {actual_python} does not match {expected_python}")

    sys.path.insert(0, str(PYTHON_DIR))
    from bridge_protocol import PROTOCOL_VERSION  # pylint: disable=import-outside-toplevel

    if manifest.get("protocol_version") != PROTOCOL_VERSION:
        fail("worker protocol version does not match manifest")
    entrypoint = manifest.get("entrypoint")
    if not isinstance(entrypoint, str) or not (PYTHON_DIR / entrypoint).is_file():
        fail("worker entrypoint is missing")

    packages = manifest.get("packages", {}).get(tier)
    if not isinstance(packages, dict) or not packages:
        fail(f"unknown or empty package tier: {tier}")
    verified_packages: dict[str, str] = {}
    for name, expected_version in packages.items():
        try:
            actual_version = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            fail(f"required package is missing: {name}")
        if actual_version != expected_version:
            fail(f"{name} {actual_version} does not match {expected_version}")
        verified_packages[name] = actual_version

    source_files = manifest.get("source_files")
    if not isinstance(source_files, dict) or not source_files:
        fail("source file hash inventory is empty")
    verified_files: dict[str, str] = {}
    for relative_name, expected_hash in source_files.items():
        if not isinstance(relative_name, str) or not isinstance(expected_hash, str):
            fail("source hash inventory contains invalid values")
        source_path = PYTHON_DIR / relative_name
        if not source_path.is_file():
            fail(f"runtime source file is missing: {relative_name}")
        actual_hash = sha256(source_path)
        if actual_hash != expected_hash:
            fail(f"runtime source hash mismatch: {relative_name}")
        verified_files[relative_name] = actual_hash

    return {
        "schema_version": manifest["schema_version"],
        "tier": tier,
        "python": f"{sys.version_info.major}.{sys.version_info.minor}",
        "protocol_version": PROTOCOL_VERSION,
        "packages": verified_packages,
        "source_files": verified_files,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tier", choices=("base", "pro"), default="base")
    args = parser.parse_args()
    try:
        report = verify(args.tier)
    except RuntimeManifestError as error:
        print(f"Python runtime manifest verification failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
