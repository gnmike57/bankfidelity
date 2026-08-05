#!/usr/bin/env python3
"""Independent closure checks for Gate 08 no-go disposition."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    raise SystemExit(f"Gate 08 FAIL: {message}")


def read(relative: str) -> str:
    path = ROOT / relative
    if not path.is_file():
        fail(f"missing {relative}")
    return path.read_text(encoding="utf-8")


def main() -> None:
    manifest = read("docs/remediation/evidence/phase-08/manifest.md")
    adr = read("docs/remediation/adr/ADR-0003-conditional-local-llm.md")
    capabilities = read("src/app/capabilities.rs")

    for ticket in [f"LLM-{number:03d}" for number in range(1, 10)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    for marker in ["**NO-GO for shipping a local LLM in v1**", "Capability::LocalLlm", "NOT APPLICABLE"]:
        if marker not in manifest:
            fail(f"manifest missing: {marker}")
    for marker in ["No-go for v1", "Do not bundle or require a local LLM", "may not determine whether a statement is balanced"]:
        if marker not in adr:
            fail(f"ADR missing: {marker}")
    for marker in ["Capability::LocalLlm", "CapabilityStatus::unavailable", "No local LLM runtime has passed the Phase 08 benchmark and packaging gate"]:
        if marker not in capabilities:
            fail(f"capability registry missing: {marker}")

    forbidden_files = [
        "src/ai/local_llm.rs",
        "src/ai/ollama.rs",
        "src/ai/llama_cpp.rs",
        "python/local_llm.py",
    ]
    for relative in forbidden_files:
        if (ROOT / relative).exists():
            fail(f"unqualified local inference adapter ships: {relative}")

    print("Gate 08 PASS — explicit v1 no-go")


if __name__ == "__main__":
    main()
