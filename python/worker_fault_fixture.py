#!/usr/bin/env python3
"""Fault-injection worker used only by Rust supervisor lifecycle tests."""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

from bridge_protocol import OPERATIONS, PROTOCOL_VERSION, build_response, canonical_json, parse_request

MODE = os.environ.get("PYTHON_WORKER_FAULT_MODE", "none")
STATE = Path(os.environ.get("PYTHON_WORKER_FAULT_STATE", "worker-fault.state"))
LOG = Path(os.environ.get("PYTHON_WORKER_FAULT_LOG", "worker-fault.log"))


def emit(value: dict[str, object]) -> None:
    sys.stdout.write(canonical_json(value) + "\n")
    sys.stdout.flush()


def handshake() -> dict[str, object]:
    return {
        "event": "handshake",
        "protocol_version": PROTOCOL_VERSION,
        "worker_pid": os.getpid(),
        "python_version": sys.version.split()[0],
        "platform": sys.platform,
        "ready": True,
        "bridge_error_class": None,
        "pymupdf_version": "fault-fixture",
        "pymupdf_pro_version": None,
        "pro_version_compatible": False,
        "pro_package_available": False,
        "pro_import_error_class": None,
        "operations": list(OPERATIONS),
    }


def first_fault() -> bool:
    try:
        STATE.parent.mkdir(parents=True, exist_ok=True)
        with STATE.open("x", encoding="utf-8") as stream:
            stream.write(MODE)
        return True
    except FileExistsError:
        return False


def main() -> int:
    emit(handshake())
    for line in sys.stdin:
        request = parse_request(line)
        LOG.parent.mkdir(parents=True, exist_ok=True)
        with LOG.open("a", encoding="utf-8") as stream:
            stream.write(request["operation_id"] + "\n")
        if first_fault():
            if MODE == "crash_once":
                print("FAULT_FIXTURE_CRASH", file=sys.stderr, flush=True)
                os._exit(17)
            if MODE == "hang_once":
                time.sleep(30)
            if MODE == "malformed_once":
                sys.stdout.write("{malformed-response\n")
                sys.stdout.flush()
                continue
        emit(
            build_response(
                request,
                disposition="succeeded",
                capability_tier="core",
                payload={"fixture": "ok"},
            )
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
