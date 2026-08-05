#!/usr/bin/env python3
"""Validate the Phase 02 critical-integrity evidence and repository invariants."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "docs" / "remediation" / "evidence" / "phase-02"
EXPECTED_CODE_CANDIDATE = "500167b3caff3090d477c15e1f28f17e92073b57"
EXPECTED_RUN = 30673473276
ALLOWED_CLOSURE_PATHS = {
    "docs/remediation/STATUS.md",
    "docs/remediation/evidence/phase-02/ci-jobs-30673473276.tsv",
    "docs/remediation/evidence/phase-02/ci-run-30673473276.json",
    "docs/remediation/evidence/phase-02/commits.tsv",
    "docs/remediation/evidence/phase-02/evidence.sha256",
    "docs/remediation/evidence/phase-02/manifest.md",
    "docs/remediation/evidence/phase-02/validator.log",
    "scripts/validate_gate02.py",
}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"Gate 02 validation failed: {message}")


def read(relative: str) -> str:
    path = ROOT / relative
    require(path.is_file(), f"missing {relative}")
    return path.read_text(encoding="utf-8")


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(ROOT), *args], text=True, stderr=subprocess.STDOUT
    ).rstrip()


def validate_checksums() -> None:
    checksum_file = EVIDENCE / "evidence.sha256"
    require(checksum_file.is_file(), "missing evidence.sha256")
    for line in checksum_file.read_text(encoding="utf-8").splitlines():
        expected, relative = line.split(maxsplit=1)
        path = ROOT / relative.lstrip("*")
        require(path.is_file(), f"checksum target missing: {relative}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        require(actual == expected, f"checksum mismatch: {relative}")


def main() -> None:
    head = git("rev-parse", "HEAD")
    ancestor = subprocess.run(
        ["git", "-C", str(ROOT), "merge-base", "--is-ancestor", EXPECTED_CODE_CANDIDATE, head],
        check=False,
    )
    require(ancestor.returncode == 0, f"{head} does not descend from the verified code candidate")
    changed = set(filter(None, git("diff", "--name-only", EXPECTED_CODE_CANDIDATE).splitlines()))
    status_paths = {
        line[3:]
        for line in git("status", "--porcelain", "--untracked-files=all").splitlines()
        if len(line) > 3
    }
    closure_paths = changed | status_paths
    require(
        closure_paths <= ALLOWED_CLOSURE_PATHS,
        f"non-evidence changes after verified candidate: {sorted(closure_paths - ALLOWED_CLOSURE_PATHS)}",
    )

    run_path = EVIDENCE / f"ci-run-{EXPECTED_RUN}.json"
    require(run_path.is_file(), "missing final CI run evidence")
    run = json.loads(run_path.read_text(encoding="utf-8"))
    require(
        run.get("headSha") == EXPECTED_CODE_CANDIDATE,
        "CI run SHA does not match verified code candidate",
    )
    require(run.get("status") == "completed", "CI run is not completed")
    require(run.get("conclusion") == "success", "CI run did not conclude success")

    jobs = {job["name"]: job for job in run.get("jobs", [])}
    mandatory = {
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
    require(mandatory <= jobs.keys(), "mandatory CI job is missing")
    for name in sorted(mandatory):
        require(jobs[name].get("conclusion") == "success", f"mandatory CI job failed: {name}")

    workflow = read(".github/workflows/ci.yml")
    require("p0-integrity:" in workflow, "mandatory P0 CI job missing")
    for runner in ("ubuntu-latest", "windows-latest", "macos-14"):
        require(runner in workflow, f"P0/base matrix missing {runner}")
    for command in (
        "python python/test_apply_many_edits_contract.py",
        "cargo test --locked --lib app::audit::tests",
        "cargo test --locked --lib app::commit::tests",
        "cargo test --locked --test integrity_regressions",
    ):
        require(command in workflow, f"P0 workflow command missing: {command}")

    python_contract = read("python/test_apply_many_edits_contract.py")
    for test_name in (
        "test_no_overlap_is_non_destructive_and_not_success",
        "test_exact_success_has_complete_counts_hashes_and_evidence",
        "test_twenty_edit_transaction_is_exact_and_repeatable",
    ):
        require(test_name in python_contract, f"Python P0 regression missing: {test_name}")

    runtime = read("src/app/runtime.rs")
    require("InstantBackgroundApply" not in runtime, "obsolete per-keystroke runtime writer remains")
    require("PythonJobResult::ApplyReport" in runtime, "typed Python application result is not consumed")
    require("materialize_preview_edits" in runtime, "preview-to-render materialization is missing")

    gui = read("src/app/gui.rs")
    require("InstantBackgroundApply" not in gui, "GUI still dispatches per-keystroke PDF writes")

    audit = read("src/app/audit.rs")
    require("std::fs::hard_link" not in audit, "audit snapshots still use hard links")
    require("create_content_addressed_snapshot" in audit, "content-addressed snapshots are missing")
    require("verify_snapshot_record" in audit, "snapshot evidence verification is missing")

    native = read("src/pdf/native_engine.rs")
    engine = read("src/pdf/engine.rs")
    require("content_span_to_top_left" in native, "native extraction is not canonicalized")
    require("top_left_to_content" in native, "native editing is not canonicalized")
    require("origin at the\n/// visible top-left" in engine, "shared bbox contract is undocumented")

    release = read(".github/workflows/release.yml")
    require(
        "publication remains intentionally frozen" in release.lower(),
        "release publication freeze marker is missing",
    )
    require("contents: read" in release, "release workflow is not read-only")
    require("Enforce release publication freeze" in release, "release freeze enforcement step is missing")

    manifest = read("docs/remediation/evidence/phase-02/manifest.md")
    status = read("docs/remediation/STATUS.md")
    require("**Final disposition:** `PASS`" in manifest, "manifest is not closed PASS")
    require(f"{EXPECTED_CODE_CANDIDATE}" in manifest, "manifest candidate is incorrect")
    require(f"{EXPECTED_RUN}" in manifest, "manifest CI run is incorrect")
    require("Gate 02 — `PASS`" in status, "live status is not Gate 02 PASS")

    validate_checksums()

    tracked = git("ls-files")
    private_patterns = re.compile(r"(^|/)(id_rsa|id_ed25519|.*private.*key.*)$", re.IGNORECASE)
    require(not any(private_patterns.search(path) for path in tracked.splitlines()), "tracked private-key-like path")
    secret_scan = subprocess.run(
        [
            "git",
            "-C",
            str(ROOT),
            "grep",
            "-n",
            "-I",
            "-E",
            "BEGIN (OPENSSH|RSA|EC) PRIVATE KEY|gh[pousr]_[A-Za-z0-9_]+",
            "--",
            ".",
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    require(secret_scan.returncode in (0, 1), "secret scan execution failed")
    require(secret_scan.returncode == 1, "private key or GitHub token material is tracked")

    print("Gate 02 validation: PASS")
    print(f"code_candidate={EXPECTED_CODE_CANDIDATE}")
    print(f"ci_run={EXPECTED_RUN}")
    print(f"mandatory_jobs={len(mandatory)}")


if __name__ == "__main__":
    main()
