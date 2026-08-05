#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "docs" / "remediation" / "evidence" / "phase-05"
CANDIDATE = "9e5e8a1ca7ee32b3a8c8ee4bf73c3cbb2958c64f"
RUN_ID = 30712005667
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
    "scripts/validate_gate05.py",
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
    checksum_path = EVIDENCE / "SHA256SUMS"
    if not checksum_path.is_file():
        fail("missing evidence checksums")
    for line in checksum_path.read_text(encoding="utf-8").splitlines():
        digest, filename = line.split(maxsplit=1)
        path = EVIDENCE / filename
        if not path.is_file():
            fail(f"missing checksummed evidence: {filename}")
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            fail(f"checksum mismatch for {filename}")


def verify_ci() -> None:
    run = json.loads((EVIDENCE / "ci-run.json").read_text(encoding="utf-8"))
    if run.get("id") != RUN_ID or run.get("head_sha") != CANDIDATE:
        fail("workflow identity does not match the Phase 05 candidate")
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
    manifest = read("docs/remediation/evidence/phase-05/manifest.md")
    for ticket in [f"EXT-{number:03d}" for number in range(1, 12)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    for required in [
        "**Decision:** **PASS**",
        CANDIDATE,
        str(RUN_ID),
        "30-row",
        "selected provider",
        "provider-free transfer",
        "Phase 06 — exact PDF editing",
    ]:
        if required not in manifest:
            fail(f"manifest missing: {required}")


def verify_source_invariants() -> None:
    model = read("src/engine/model.rs")
    runtime = read("src/app/runtime.rs")
    workflow = read("src/engine/workflow.rs")
    offline = read("src/engine/offline_parser.rs")
    transfer = read("src/engine/transfer.rs")
    config = read("src/app/config.rs")
    modals = read("src/app/modals.rs")
    cli = read("src/app/cli.rs")
    verification = read("src/engine/verification.rs")
    integrity = read("tests/integrity_regressions.rs")

    for marker in [
        "pub struct CanonicalMetadata",
        "stable_row_id",
        "pub currency: Option<String>",
        "pub locale: Option<String>",
        "pub confidence: Option<f32>",
        "pub review_required: bool",
        "ensure_canonical_metadata",
        "legacy_transaction_json_deserializes_then_normalizes",
    ]:
        if marker not in model:
            fail(f"canonical model invariant missing: {marker}")

    for marker in [
        "fn extraction_provider_order",
        "DocumentParserMode::LlamaParse",
        "DocumentParserMode::DocumentAi",
        "DocumentParserMode::OfflineHeuristic",
        "validated extraction cache hit",
        "deterministic_parse_issues",
        "extraction_router_honors_selected_provider_without_unrelated_cloud_calls",
    ]:
        if marker not in runtime:
            fail(f"router invariant missing: {marker}")

    for marker in [
        "deterministic_parse_issues",
        "requires review",
        "edit targets missing row",
        "invalid debit value",
        "build_preview_cascades_exact_balances_across_pages",
        "final output ledger validation failed",
    ]:
        if marker not in workflow and marker not in runtime:
            fail(f"financial/completeness invariant missing: {marker}")

    for marker in [
        "representative_two_page_statement_extracts_all_rows_offline",
        "calibrate_offline_confidence",
        "extract_spatial_amounts",
    ]:
        if marker not in offline:
            fail(f"offline extraction invariant missing: {marker}")

    if (
        "is_v1_selectable" not in config
        or "Local OCR PDF parsing is not part of v1" not in modals
    ):
        fail("Local OCR v1 exclusion is not explicit")
    if "plan_transaction_transfer_deterministic" not in transfer:
        fail("provider-free deterministic transfer planner is missing")
    if "required: true" not in verification or "validate_math_inputs" not in verification:
        fail("strict financial verification policy is missing")
    if "ExtractBatch" not in cli or "batch_manifest.json" not in cli:
        fail("bounded batch extraction contract is missing")
    for marker in [
        "ordered_offline_router_extracts_complete_canonical_ledger",
        "zero_row_statement_is_never_reported_as_extraction_or_balance_success",
    ]:
        if marker not in integrity:
            fail(f"integrity regression missing: {marker}")


def verify_data_evidence() -> None:
    batch = json.loads((EVIDENCE / "batch-manifest.json").read_text(encoding="utf-8"))
    if len(batch) != 2:
        fail("batch evidence does not contain every input file")
    for result in batch:
        if result.get("status") != "success" or result.get("row_count") != 30:
            fail(f"batch extraction result is not exact: {result}")
    fixture_record = (EVIDENCE / "fixture-sha256.txt").read_text(encoding="utf-8")
    fixture = ROOT / "tests" / "stress_pdfs" / "Standard_Bank_Statement_01.pdf"
    digest = hashlib.sha256(fixture.read_bytes()).hexdigest()
    if not fixture_record.startswith(digest):
        fail("representative fixture checksum does not match")


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
        and not path.startswith("docs/remediation/evidence/phase-05/")
    }
    if unexpected:
        fail(f"unexpected Gate 05 closure paths: {sorted(unexpected)}")

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
            and not path.startswith("docs/remediation/evidence/phase-05/")
        }
        if unexpected_committed:
            fail(f"post-candidate commit contains non-closure paths: {sorted(unexpected_committed)}")


def verify_hygiene() -> None:
    prohibited = re.compile(
        rb"BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY|github_pat_[A-Za-z0-9_]+|ghp_[A-Za-z0-9]+"
    )
    paths = [
        ROOT / "docs" / "remediation" / "STATUS.md",
        ROOT / "scripts" / "validate_gate05.py",
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
    verify_data_evidence()
    verify_closure_scope()
    verify_hygiene()
    print("Gate 05 validation: PASS")
    print(f"candidate={CANDIDATE}")
    print(f"workflow_run={RUN_ID}")
    print(f"mandatory_jobs={len(REQUIRED_JOBS)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"Gate 05 validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
