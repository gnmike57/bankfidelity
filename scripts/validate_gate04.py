#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "docs" / "remediation" / "evidence" / "phase-04"
CANDIDATE = "85e15fca5574068035eccab9e3a700958ee94b92"
RUN_ID = 30707260996
REQUIRED_JOBS = {
    "rustfmt",
    "clippy production surfaces",
    "base state (ubuntu-latest)",
    "base state (windows-latest)",
    "base state (macos-14)",
    "optional PyMuPDF Pro import (windows-latest)",
    "optional PyMuPDF Pro import (macos-14)",
    "P0 integrity regressions (ubuntu-latest)",
    "P0 integrity regressions (windows-latest)",
    "P0 integrity regressions (macos-14)",
}
ALLOWED_CLOSURE_PATHS = {
    "docs/remediation/STATUS.md",
    "scripts/validate_gate04.py",
}


def fail(message: str) -> None:
    raise AssertionError(message)


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", *args], cwd=ROOT, text=True, stderr=subprocess.STDOUT
    ).rstrip()


def verify_checksums() -> None:
    checksums = EVIDENCE / "SHA256SUMS"
    if not checksums.is_file():
        fail("missing evidence checksums")
    for line in checksums.read_text(encoding="utf-8").splitlines():
        digest, filename = line.split(maxsplit=1)
        path = EVIDENCE / filename
        if not path.is_file():
            fail(f"missing checksummed evidence: {filename}")
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            fail(f"checksum mismatch for {filename}")


def verify_ci() -> None:
    run = json.loads((EVIDENCE / "ci-run.json").read_text(encoding="utf-8"))
    if run.get("id") != RUN_ID or run.get("head_sha") != CANDIDATE:
        fail("workflow identity does not match the declared candidate")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        fail("workflow is not a completed success")

    payload = json.loads((EVIDENCE / "ci-jobs.json").read_text(encoding="utf-8"))
    jobs = {job["name"]: job for job in payload.get("jobs", [])}
    missing = REQUIRED_JOBS - jobs.keys()
    if missing:
        fail(f"missing mandatory CI jobs: {sorted(missing)}")
    failed = {
        name: jobs[name].get("conclusion")
        for name in REQUIRED_JOBS
        if jobs[name].get("status") != "completed"
        or jobs[name].get("conclusion") != "success"
    }
    if failed:
        fail(f"mandatory CI jobs did not pass: {failed}")
    advisory = jobs.get("deferred hardening inventory")
    if not advisory or advisory.get("conclusion") != "failure":
        fail("expected deferred hardening advisory outcome is not recorded")


def verify_manifest() -> None:
    manifest = read("docs/remediation/evidence/phase-04/manifest.md")
    for ticket in [f"PY-{number:03d}" for number in range(1, 12)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    for required in [
        "**Decision:** **PASS**",
        CANDIDATE,
        str(RUN_ID),
        "No ambiguous replay",
        "Exact mutation accounting",
        "Phase 05 — extraction, financial algorithms, OCR, dates, and transfer workflows",
    ]:
        if required not in manifest:
            fail(f"manifest missing: {required}")


def verify_source_invariants() -> None:
    worker = read("python/worker.py")
    supervisor = read("src/ai/python_worker.rs")
    protocol = read("src/ai/python_protocol.rs")
    bridge_protocol = read("python/bridge_protocol.py")
    bridge = read("python/pymupdf_pro_integration.py")
    cargo = read("Cargo.toml")
    ci = read(".github/workflows/ci.yml")
    policy = read("docs/remediation/PYTHON_RUNTIME_POLICY.md")
    runtime_manifest = json.loads(read("python/runtime-manifest.json"))

    for marker in [
        "class MutationTransaction",
        "def _isolate_protocol_output",
        "os.dup2(sys.stderr.fileno(), sys.stdout.fileno())",
        'result.get("placed", 0)',
        'verify_runtime_manifest("base")',
    ]:
        if marker not in worker:
            fail(f"worker invariant missing: {marker}")
    for marker in [
        "pub struct PythonWorkerSupervisor",
        "max_operations_per_worker",
        "max_rss_growth_bytes",
        "max_handle_growth",
        "discover_bundled_python_root",
        "hundred_real_pdf_operations_close_handles_and_stay_bounded",
    ]:
        if marker not in supervisor:
            fail(f"supervisor invariant missing: {marker}")
    if "PYTHON_PROTOCOL_VERSION" not in protocol or "deny_unknown_fields" not in protocol:
        fail("strict Rust protocol invariant missing")
    if "PROTOCOL_VERSION" not in bridge_protocol or "_require_exact_keys" not in bridge_protocol:
        fail("strict Python protocol invariant missing")
    extraction = bridge.split("def get_text_blocks", 1)[1].split("\ndef ", 1)[0]
    if "_ensure_pro_unlocked" in extraction or "with pymupdf.open" not in extraction:
        fail("core extraction is not Pro-free with deterministic document closure")
    if re.search(r"\bpyo3\b", cargo, re.IGNORECASE):
        fail("embedded PyO3 dependency remains")
    if runtime_manifest.get("python") != {"major": 3, "minor": 12}:
        fail("runtime manifest does not pin Python 3.12")
    if runtime_manifest.get("packages", {}).get("base", {}).get("PyMuPDF") != "1.28.0":
        fail("runtime manifest does not pin PyMuPDF 1.28.0")
    for marker in [
        "Offline bundled Python runtime smoke",
        "Pro-installed worker protocol and exact-count contract",
        "python scripts/generate_python_runtime_manifest.py --check",
    ]:
        if marker not in ci:
            fail(f"CI invariant missing: {marker}")
    if "Python 3.12" not in policy or "bundled" not in policy.lower():
        fail("Python runtime policy is incomplete")


def verify_closure_scope() -> None:
    paths: set[str] = set()
    for line in git("status", "--porcelain=v1", "--untracked-files=all").splitlines():
        if not line:
            continue
        path = line[3:]
        if " -> " in path:
            path = path.split(" -> ", 1)[1]
        paths.add(path)
    unexpected = {
        path
        for path in paths
        if path not in ALLOWED_CLOSURE_PATHS
        and not path.startswith("docs/remediation/evidence/phase-04/")
    }
    if unexpected:
        fail(f"unexpected Gate 04 closure paths: {sorted(unexpected)}")

    head = git("rev-parse", "HEAD")
    if head != CANDIDATE:
        if subprocess.run(
            ["git", "merge-base", "--is-ancestor", CANDIDATE, head],
            cwd=ROOT,
            check=False,
        ).returncode != 0:
            fail("closure is not based on the verified candidate")
        changed = set(git("diff", "--name-only", f"{CANDIDATE}..{head}").splitlines())
        unexpected_committed = {
            path
            for path in changed
            if path not in ALLOWED_CLOSURE_PATHS
            and not path.startswith("docs/remediation/evidence/phase-04/")
        }
        if unexpected_committed:
            fail(f"post-candidate commit contains non-closure paths: {sorted(unexpected_committed)}")


def verify_hygiene() -> None:
    prohibited = re.compile(
        rb"BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY|github_pat_[A-Za-z0-9_]+|ghp_[A-Za-z0-9]+"
    )
    paths = [
        ROOT / "docs" / "remediation" / "STATUS.md",
        ROOT / "scripts" / "validate_gate04.py",
        *EVIDENCE.rglob("*"),
    ]
    for path in paths:
        if path.is_file() and prohibited.search(path.read_bytes()):
            fail(f"prohibited credential material found in {path.relative_to(ROOT)}")


def main() -> int:
    verify_checksums()
    verify_ci()
    verify_manifest()
    verify_source_invariants()
    verify_closure_scope()
    verify_hygiene()
    print("Gate 04 validation: PASS")
    print(f"candidate={CANDIDATE}")
    print(f"workflow_run={RUN_ID}")
    print(f"mandatory_jobs={len(REQUIRED_JOBS)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"Gate 04 validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
