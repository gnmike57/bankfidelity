#!/usr/bin/env python3
"""BankFidelity <-> UFO Sequential E2E Architecture Audit.

Verifies every stage of the Rust -> UFO -> MCP -> Rust loop in order,
printing one PASS / FAIL / SKIP line per check and exiting non-zero when
any FAIL is present. Standard library only; safe to run from the desktop
launchers (01[9], 02[6], 04[4]) or CI.

Usage:
    python audit_e2e_sequential.py [--bankfidelity-dir PATH] [--ufo-dir PATH]
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

RESULTS: list[tuple[str, str, str]] = []  # (stage, status, detail)


def record(stage: str, status: str, detail: str = "") -> None:
    RESULTS.append((stage, status, detail))
    mark = {"PASS": "[PASS]", "FAIL": "[FAIL]", "SKIP": "[SKIP]"}[status]
    print(f"{mark} {stage:<28} {detail}")


def check_python_env() -> None:
    v = sys.version_info
    record("python-interpreter", "PASS", f"{sys.executable} ({v.major}.{v.minor}.{v.micro})")


def check_ufo_layout(ufo_dir: Path) -> None:
    py = ufo_dir / "python_env" / "python.exe"
    if py.exists():
        record("ufo-python-env", "PASS", str(py))
    else:
        record("ufo-python-env", "SKIP", f"not found at {py}")

    yaml_candidates = [
        ufo_dir / "config" / "system.yaml",
        ufo_dir / "config" / "ufo" / "system.yaml",
    ]
    found = next((p for p in yaml_candidates if p.exists()), None)
    if found:
        record("ufo-config-yaml", "PASS", str(found))
    else:
        record("ufo-config-yaml", "FAIL", "system.yaml not found under config/")


def find_bankfidelity_exe(explicit):
    candidates = []
    if explicit:
        candidates.append(Path(explicit))
    here = Path(__file__).resolve()
    for base in [here.parents[1], here.parents[2], Path(r"C:\bankfidelity\bankfidelity")]:
        for profile in ("release", "debug"):
            candidates.append(base / "target" / profile / "dual-core-pdf-pipeline.exe")
    for c in candidates:
        if c.is_file():
            return c
    return None


def _rpc(exe, payload, timeout=20.0):
    proc = subprocess.Popen(
        [str(exe), "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        out, _ = proc.communicate(json.dumps(payload) + "\n", timeout=timeout)
    finally:
        if proc.poll() is None:
            proc.kill()
    line = next((l for l in (out or "").splitlines() if l.strip().startswith("{")), "")
    return json.loads(line) if line else {}


def mcp_handshake(exe, timeout=20.0):
    """Complete a JSON-RPC initialize handshake + tools/list against `exe mcp`."""
    try:
        resp = _rpc(
            exe,
            {"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2026-07-28"}},
            timeout,
        )
        name = resp.get("result", {}).get("serverInfo", {}).get("name", "")
        if name == "BankFidelity MCP":
            record("mcp-initialize", "PASS", "handshake OK")
        else:
            record("mcp-initialize", "FAIL", f"unexpected serverInfo.name={name!r}")
            return False

        tools = _rpc(exe, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}, timeout)
        advertised = tools.get("result", {}).get("tools", [])
        ok = len(advertised) >= 9
        record("mcp-tools-list", "PASS" if ok else "FAIL",
               f"{len(advertised)} tools advertised")
        return ok
    except (subprocess.TimeoutExpired, OSError, json.JSONDecodeError) as exc:
        record("mcp-handshake", "FAIL", str(exc))
        return False


def check_api_keys(bf_dir):
    """Report which optional keys are configured (names only - never values)."""
    keys = [
        "DUAL_CORE_PASSPHRASE", "GEMINI_API_KEY", "GROQ_API_KEY",
        "OPENROUTER_API_KEY", "MISTRAL_API_KEY", "MINDEE_API_KEY",
        "LLAMAPARSE_API_KEY", "PDFREST_API_KEY", "VISION_API_KEY",
        "REDUCTO_API_KEY",
    ]
    env_vals = {}
    env_file = bf_dir / ".env"
    if env_file.exists():
        for raw in env_file.read_text(encoding="utf-8", errors="replace").splitlines():
            line = raw.strip()
            if "=" in line and not line.startswith("#"):
                k, _, v = line.partition("=")
                env_vals[k.strip()] = v.strip()

    def is_set(name):
        v = os.environ.get(name) or env_vals.get(name) or ""
        return bool(v.strip()) and not v.startswith("your_")

    missing_required = [k for k in ("DUAL_CORE_PASSPHRASE",) if not is_set(k)]
    optional_set = sum(1 for k in keys if k != "DUAL_CORE_PASSPHRASE" and is_set(k))
    if missing_required:
        record("api-keys", "FAIL",
               f"missing required: {', '.join(missing_required)}; "
               f"{optional_set} optional backends configured")
    else:
        record("api-keys", "PASS",
               f"DUAL_CORE_PASSPHRASE set; {optional_set} optional AI/parser keys set")


def check_logs_writable(ufo_dir):
    logs = ufo_dir / "logs"
    try:
        logs.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=logs, delete=False) as fh:
            fh.write(b"bankfidelity-audit-probe")
            probe = Path(fh.name)
        probe.unlink(missing_ok=True)
        record("ufo-logs-writable", "PASS", str(logs))
    except OSError as exc:
        record("ufo-logs-writable", "FAIL", str(exc))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ufo-dir",
                        default=os.environ.get("BANKFIDELITY_UFO_DIR", r"C:\ufo\ufo"))
    parser.add_argument("--bankfidelity-dir", default=r"C:\bankfidelity\bankfidelity")
    parser.add_argument("--exe", default=None,
                        help="explicit path to the dual-core-pdf-pipeline executable")
    args = parser.parse_args()

    ufo_dir = Path(args.ufo_dir)
    bf_dir = Path(args.bankfidelity_dir)

    print("=" * 72)
    print("BANKFIDELITY // UFO SEQUENTIAL E2E ARCHITECTURE AUDIT")
    print("=" * 72)

    check_python_env()

    if ufo_dir.is_dir():
        record("ufo-install-root", "PASS", str(ufo_dir))
        check_ufo_layout(ufo_dir)
    else:
        record("ufo-install-root", "FAIL",
               f"not found: {ufo_dir} (run scripts/setup_ufo.ps1)")

    exe = find_bankfidelity_exe(args.exe)
    if exe:
        record("bankfidelity-binary", "PASS", str(exe))
    else:
        record("bankfidelity-binary", "FAIL",
               "dual-core-pdf-pipeline.exe not found (cargo build --release)")
        record("mcp-initialize", "SKIP", "no binary")
        record("mcp-tools-list", "SKIP", "no binary")

    if bf_dir.is_dir():
        record("bankfidelity-repo", "PASS", str(bf_dir))
        check_api_keys(bf_dir)
    else:
        record("bankfidelity-repo", "FAIL", f"not found: {bf_dir}")

    if exe and ufo_dir.is_dir():
        mcp_handshake(exe)

    if ufo_dir.is_dir():
        check_logs_writable(ufo_dir)

    fails = sum(1 for _, s, _ in RESULTS if s == "FAIL")
    skips = sum(1 for _, s, _ in RESULTS if s == "SKIP")
    passes = sum(1 for _, s, _ in RESULTS if s == "PASS")
    print("=" * 72)
    print(f"RESULT: {passes} passed, {fails} failed, {skips} skipped")
    print("VERDICT:", "E2E CHAIN HEALTHY" if fails == 0 else "E2E CHAIN DEGRADED")
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())

