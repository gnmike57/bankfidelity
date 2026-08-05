#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "docs" / "remediation" / "evidence" / "phase-06"
CANDIDATE = "913672163fb5eee329a3b2b63e8ae601958fda63"
RUN_ID = 30720299347
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
    "scripts/validate_gate06.py",
}


def fail(message: str) -> None:
    raise AssertionError(message)


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def git(*args: str) -> str:
    return subprocess.check_output(
        ["git", *args], cwd=ROOT, text=True, stderr=subprocess.STDOUT
    ).rstrip()


def require_markers(label: str, text: str, markers: list[str]) -> None:
    for marker in markers:
        if marker not in text:
            fail(f"{label} invariant missing: {marker}")


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
        fail("workflow identity does not match the Phase 06 candidate")
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
        fail("expected deferred-hardening advisory outcome is not recorded")


def verify_manifest() -> None:
    manifest = read("docs/remediation/evidence/phase-06/manifest.md")
    for ticket in [f"PDF-{number:03d}" for number in range(1, 10)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    for required in [
        "**Decision:** **PASS**",
        CANDIDATE,
        str(RUN_ID),
        "stable target identity",
        "CTM",
        "CropBox",
        "pinned",
        "atomic publication",
        "Phase 07 — independent verification gates",
    ]:
        if required not in manifest:
            fail(f"manifest missing: {required}")


def verify_source_invariants() -> None:
    native = read("src/pdf/native_engine.rs")
    pro = read("python/pymupdf_pro_integration.py")
    worker = read("python/worker.py")
    runtime = read("src/app/runtime.rs")
    segments = read("src/engine/segments.rs")
    split_merge = read("src/engine/pdf_split_merge.rs")
    font_replication = read("src/engine/font_replication.rs")
    font_analysis = read("src/engine/font_analysis.rs")
    config = read("src/app/config.rs")
    protocol_generator = read("scripts/generate_python_protocol_fixtures.py")
    pdfium_manifest = json.loads(read("assets/pdfium-artifacts.json"))

    require_markers(
        "native exact target",
        native,
        [
            "old_text",
            "stable target",
            "is ambiguous",
            "CropBox",
            "crop_origin_rotation_mappings_are_exact",
            "persist",
        ],
    )
    require_markers(
        "Pro exact target",
        pro,
        ["old_text", "identity-no-match", "duplicate-target", "placed"],
    )
    require_markers(
        "runtime publication",
        runtime,
        [
            "FileCommitBarrier",
            "Visual-fidelity bypass is disabled",
            "Typst reconstruction is a non-fidelity export",
            "prior output was preserved",
        ],
    )
    require_markers(
        "segmentation",
        segments,
        ["validate_structure", "validate_segment_replacement", "apply_and_merge"],
    )
    require_markers(
        "split/merge publication",
        split_merge,
        ["tempfile::Builder::new", "staged merge page count mismatch", ".persist(output_path)"],
    )
    require_markers(
        "font policy",
        font_replication + font_analysis + worker + pro,
        ["FONT_SUBSTITUTION_DISABLED", "Font substitution blocked:"],
    )
    require_markers(
        "legacy engine selection",
        config,
        ["TypstReconstruct", "is_fidelity_selectable"],
    )
    if protocol_generator.count('"old_text": "original"') < 2:
        fail("generated single and batch protocol fixtures lack stable old-text identity")

    if pdfium_manifest.get("schema_version") != 1:
        fail("Pdfium artifact manifest schema is not pinned")
    if pdfium_manifest.get("release_tag") != "chromium/7961":
        fail("Pdfium release tag is not the verified immutable release")
    artifacts = pdfium_manifest.get("artifacts", {})
    for platform in ["windows-x86_64", "macos-aarch64", "macos-x86_64", "linux-x86_64"]:
        artifact = artifacts.get(platform)
        if not artifact:
            fail(f"missing Pdfium artifact: {platform}")
        for field in ["archive_sha256", "library_sha256"]:
            digest = artifact.get(field, "")
            if len(digest) != 64 or not all(char in "0123456789abcdef" for char in digest):
                fail(f"invalid Pdfium {field} for {platform}")

    test_markers = {
        "tests/native_characterization.rs": [
            "native_batch_rejects_old_text_mismatch_without_publishing",
            "native_batch_rejects_ambiguous_duplicate_operator_without_publishing",
            "native_batch_applies_one_cropped_target_at_visible_geometry",
            "native_batch_applies_one_rotated_target_at_visible_geometry",
        ],
        "tests/segment_transaction.rs": [
            "boundary_edits_keep_exact_segment_membership_and_global_order",
            "interrupted_segment_apply_preserves_output_and_retry_succeeds",
            "malformed_map_wrong_page_count_and_geometry_drift_never_replace_output",
        ],
        "tests/split_merge_fidelity.rs": [
            "split_merge_preserves_document_metadata_and_per_page_boxes",
            "merge_failures_preserve_existing_destination_bytes",
        ],
        "tests/integrity_regressions.rs": [
            "typst_reconstruction_is_disabled_and_preserves_destination"
        ],
    }
    for path, markers in test_markers.items():
        require_markers(path, read(path), markers)


def verify_critical_file_identities() -> None:
    record = EVIDENCE / "critical-files-sha256.txt"
    if not record.is_file():
        fail("missing critical-file checksum record")
    for line in record.read_text(encoding="utf-8").splitlines():
        digest, relative = line.split(maxsplit=1)
        path = ROOT / relative
        if not path.is_file():
            fail(f"missing critical file: {relative}")
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            fail(f"critical-file checksum mismatch: {relative}")


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
        and not path.startswith("docs/remediation/evidence/phase-06/")
    }
    if unexpected:
        fail(f"unexpected Gate 06 closure paths: {sorted(unexpected)}")

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
            and not path.startswith("docs/remediation/evidence/phase-06/")
        }
        if unexpected_committed:
            fail(f"post-candidate commit contains non-closure paths: {sorted(unexpected_committed)}")


def verify_hygiene() -> None:
    prohibited = re.compile(
        rb"BEGIN (?:RSA |OPENSSH |EC )?PRIVATE KEY|github_pat_[A-Za-z0-9_]+|ghp_[A-Za-z0-9]+"
    )
    paths = [
        ROOT / "docs" / "remediation" / "STATUS.md",
        ROOT / "scripts" / "validate_gate06.py",
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
    verify_critical_file_identities()
    verify_closure_scope()
    verify_hygiene()
    print("Gate 06 validation: PASS")
    print(f"candidate={CANDIDATE}")
    print(f"workflow_run={RUN_ID}")
    print(f"mandatory_jobs={len(REQUIRED_JOBS)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"Gate 06 validation: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
