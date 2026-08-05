#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "docs" / "remediation" / "evidence" / "phase-03"
CANDIDATE = "c84c631e62590a366608b9dd2d9c20c476568f78"
RUN_ID = 30698338534
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
    "scripts/validate_gate03.py",
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
    for line in (EVIDENCE / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        digest, filename = line.split(maxsplit=1)
        path = EVIDENCE / filename
        if not path.is_file():
            fail(f"missing checksummed evidence: {filename}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != digest:
            fail(f"checksum mismatch for {filename}")


def verify_ci() -> None:
    run = json.loads((EVIDENCE / "ci-run.json").read_text(encoding="utf-8"))
    if run.get("id") != RUN_ID:
        fail("unexpected workflow run ID")
    if run.get("head_sha") != CANDIDATE:
        fail("workflow did not validate the declared candidate")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        fail("workflow is not a completed success")

    jobs_payload = json.loads((EVIDENCE / "ci-jobs.json").read_text(encoding="utf-8"))
    jobs = {job["name"]: job for job in jobs_payload.get("jobs", [])}
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
        fail("deferred hardening inventory outcome is not recorded as the expected advisory failure")


def verify_manifest() -> None:
    manifest = read("docs/remediation/evidence/phase-03/manifest.md")
    for ticket in [f"RUN-{number:03d}" for number in range(1, 11)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    for required in [
        "**Decision:** **PASS**",
        CANDIDATE,
        str(RUN_ID),
        "Exactly one terminal result per job",
        "Bounded privacy-safe diagnostics",
        "Phase 04 — Permanent Python/PyMuPDF pipeline fortification",
    ]:
        if required not in manifest:
            fail(f"manifest missing: {required}")


def verify_source_invariants() -> None:
    runtime = read("src/app/runtime.rs")
    telemetry = read("src/app/telemetry.rs")
    config = read("src/app/config.rs")
    app_mod = read("src/app/mod.rs")
    release = read(".github/workflows/release.yml")

    required_runtime = [
        "pub struct JobMetadata",
        "pub struct RuntimeClient",
        "struct ResultSink",
        "pub enum OperationDisposition",
        "spawn_job_lifecycle_monitor",
        '"runtime_job"',
        "correlation_id = %metadata.correlation_id",
        "OperationDisposition::NoOp",
        "OperationDisposition::TimedOut",
    ]
    for marker in required_runtime:
        if marker not in runtime:
            fail(f"runtime invariant missing: {marker}")
    for marker in [
        "enforce_log_retention",
        "support_log_tail",
        "DEFAULT_LOG_RETENTION",
        "DEFAULT_MAX_FILES_PER_STREAM",
    ]:
        if marker not in telemetry:
            fail(f"telemetry invariant missing: {marker}")
    if "ConnectionMode" in config or "REMOTE_ENGINE_URL" in config:
        fail("unsupported v1 remote-engine configuration remains")
    if "pub mod workflow_state" in app_mod or "pub mod config_v2" in app_mod:
        fail("retired parallel state/config module remains exported")
    if (
        "Enforce release publication freeze" not in release
        or "Release publication is frozen until the final remediation gate passes." not in release
        or "startsWith(github.ref, 'refs/tags/v')" not in release
        or "contents: read" not in release
    ):
        fail("release publication freeze is not enforced")


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
        and not path.startswith("docs/remediation/evidence/phase-03/")
    }
    if unexpected:
        fail(f"unexpected Gate 03 closure paths: {sorted(unexpected)}")
    head = git("rev-parse", "HEAD")
    if head != CANDIDATE:
        ancestor = subprocess.run(
            ["git", "merge-base", "--is-ancestor", CANDIDATE, head],
            cwd=ROOT,
            check=False,
        )
        if ancestor.returncode != 0:
            fail("closure is not based on the verified candidate")
        committed_paths = set(
            git("diff", "--name-only", f"{CANDIDATE}..{head}").splitlines()
        )
        unexpected_committed = {
            path
            for path in committed_paths
            if path not in ALLOWED_CLOSURE_PATHS
            and not path.startswith("docs/remediation/evidence/phase-03/")
        }
        if unexpected_committed:
            fail(
                "post-candidate commit contains non-closure paths: "
                f"{sorted(unexpected_committed)}"
            )


def verify_hygiene() -> None:
    prohibited = re.compile(
        rb"BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY|github_pat_[A-Za-z0-9_]+|ghp_[A-Za-z0-9]+"
    )
    for path in [
        ROOT / "docs" / "remediation" / "STATUS.md",
        ROOT / "scripts" / "validate_gate03.py",
        *EVIDENCE.rglob("*"),
    ]:
        if path.is_file() and prohibited.search(path.read_bytes()):
            fail(f"prohibited credential material found in {path.relative_to(ROOT)}")


def main() -> int:
    verify_checksums()
    verify_ci()
    verify_manifest()
    verify_source_invariants()
    verify_closure_scope()
    verify_hygiene()
    print("Gate 03 validation: PASS")
    print(f"candidate={CANDIDATE}")
    print(f"workflow_run={RUN_ID}")
    print(f"mandatory_jobs={len(REQUIRED_JOBS)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"Gate 03 validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
