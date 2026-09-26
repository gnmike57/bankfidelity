#!/usr/bin/env python3
"""
scripts/test_mcp_bridge.py
End-to-end test verifying BankFidelity MCP Server stdio JSON-RPC 2.0 protocol compliance.
"""

import json
import subprocess
import sys
from pathlib import Path

def test_mcp_server():
    exe_path = Path("target/release/dual-core-pdf-pipeline.exe")
    if not exe_path.exists():
        exe_path = Path("target/debug/dual-core-pdf-pipeline.exe")
    if not exe_path.exists():
        print(f"[ERROR] Binary not found at {exe_path}")
        return False

    print(f"[*] Testing MCP Server with binary: {exe_path}")
    proc = subprocess.Popen(
        [str(exe_path), "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8"
    )

    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize"},
        {"jsonrpc": "2.0", "id": 2, "method": "ping"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 4, "method": "prompts/list"},
        {"jsonrpc": "2.0", "id": 5, "method": "prompts/get", "params": {"name": "bankfidelity_agent_instructions"}},
        {"jsonrpc": "2.0", "id": 6, "method": "resources/list"},
    ]

    all_passed = True
    for req in requests:
        payload = json.dumps(req) + "\n"
        proc.stdin.write(payload)
        proc.stdin.flush()
        line = proc.stdout.readline()
        if not line:
            print(f"[FAIL] No response for method {req['method']}")
            all_passed = False
            continue
        try:
            resp = json.loads(line)
            if "error" in resp:
                print(f"[FAIL] Error in {req['method']}: {resp['error']}")
                all_passed = False
            elif resp.get("id") == req["id"] and "result" in resp:
                print(f"  [PASS] {req['method']} -> ID {req['id']} OK")
            else:
                print(f"[FAIL] Unexpected format for {req['method']}: {resp}")
                all_passed = False
        except json.JSONDecodeError as e:
            print(f"[FAIL] Invalid JSON response for {req['method']}: {line} ({e})")
            all_passed = False

    proc.stdin.close()
    proc.terminate()
    proc.wait(timeout=5)

    if all_passed:
        print("[SUCCESS] All MCP stdio JSON-RPC protocol tests PASSED.")
    return all_passed

if __name__ == "__main__":
    success = test_mcp_server()
    sys.exit(0 if success else 1)
