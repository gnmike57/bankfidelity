#!/usr/bin/env python3
"""Exercise the packaged worker layout without relying on PATH or online startup."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
import venv
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_PYTHON = ROOT / "python"
REQUIREMENTS = ROOT / "requirements-ci.txt"


def bundled_python(runtime_root: Path) -> Path:
    if os.name == "nt":
        return runtime_root / "Scripts" / "python.exe"
    return runtime_root / "bin" / "python3"


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    subprocess.run(command, check=True, cwd=ROOT, env=env)


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="python-bundle-smoke-") as temporary:
        temporary_root = Path(temporary)
        wheelhouse = temporary_root / "wheelhouse"
        bundle_root = temporary_root / "resources" / "python"
        runtime_root = bundle_root / "runtime"
        wheelhouse.mkdir()
        bundle_root.mkdir(parents=True)

        # Network access is permitted only while preparing build inputs.
        run(
            [
                sys.executable,
                "-m",
                "pip",
                "download",
                "--only-binary=:all:",
                "--dest",
                str(wheelhouse),
                "--requirement",
                str(REQUIREMENTS),
            ]
        )
        venv.EnvBuilder(with_pip=True, clear=True, symlinks=False).create(runtime_root)
        python = bundled_python(runtime_root)
        if not python.is_file():
            raise SystemExit(f"bundled interpreter missing: {python}")
        run(
            [
                str(python),
                "-m",
                "pip",
                "install",
                "--no-index",
                "--find-links",
                str(wheelhouse),
                "--requirement",
                str(REQUIREMENTS),
            ]
        )

        manifest = json.loads(
            (SOURCE_PYTHON / "runtime-manifest.json").read_text(encoding="utf-8")
        )
        runtime_files = set(manifest["source_files"])
        runtime_files.add("runtime-manifest.json")
        for relative_name in runtime_files:
            shutil.copy2(SOURCE_PYTHON / relative_name, bundle_root / relative_name)

        # Remove build inputs and make runtime discovery independent of system PATH.
        shutil.rmtree(wheelhouse)
        environment = os.environ.copy()
        environment["PATH"] = ""
        environment["PYTHONNOUSERSITE"] = "1"
        environment["PYTHONPATH"] = str(bundle_root)
        environment.pop("PYMUPDF_PRO_KEY", None)

        run(
            [
                str(python),
                str(bundle_root / "verify_runtime_manifest.py"),
                "--tier",
                "base",
            ],
            env=environment,
        )
        process = subprocess.Popen(
            [str(python), str(bundle_root / "worker.py")],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            env=environment,
            cwd=bundle_root,
        )
        try:
            assert process.stdout is not None
            assert process.stdin is not None
            handshake_line = process.stdout.readline()
            if not handshake_line:
                stderr = process.stderr.read() if process.stderr is not None else ""
                raise AssertionError(f"worker emitted no handshake: {stderr}")
            handshake = json.loads(handshake_line)
            if not handshake.get("ready"):
                raise AssertionError(f"bundled worker unavailable: {handshake}")
            if handshake.get("protocol_version") != "1.0.0":
                raise AssertionError(f"unexpected protocol: {handshake}")
            if handshake.get("pro_available"):
                raise AssertionError("base bundle unexpectedly exposed Pro capability")

            now_ms = int(time.time() * 1000)
            request = {
                "protocol_version": "1.0.0",
                "operation": "ping",
                "operation_id": str(uuid.uuid4()),
                "submitted_at_unix_ms": now_ms,
                "deadline_unix_ms": now_ms + 30_000,
                "input_sha256": None,
                "payload": {},
            }
            process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
            process.stdin.flush()
            response = json.loads(process.stdout.readline())
            if response.get("disposition") != "succeeded":
                raise AssertionError(f"bundled worker ping failed: {response}")
            if response.get("operation_id") != request["operation_id"]:
                raise AssertionError("bundled worker response identity mismatch")
        finally:
            if process.stdin is not None:
                process.stdin.close()
            try:
                return_code = process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                return_code = process.wait(timeout=5)
            if return_code != 0:
                stderr = process.stderr.read() if process.stderr is not None else ""
                raise AssertionError(f"bundled worker exited {return_code}: {stderr}")

    print("offline Python bundle layout smoke passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
