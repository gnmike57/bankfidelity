#!/usr/bin/env python3
"""Independent closure checks for Gate 07."""
from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "docs/remediation/evidence/phase-07/manifest.md"
CANDIDATE = "c354094b83e31ca7e026e7749c567931a97a43f4"
RUN_ID = "30730576656"


def fail(message: str) -> None:
    raise SystemExit(f"Gate 07 FAIL: {message}")


def read(relative: str) -> str:
    path = ROOT / relative
    if not path.is_file():
        fail(f"missing {relative}")
    return path.read_text(encoding="utf-8")


def require_markers(label: str, text: str, markers: list[str]) -> None:
    for marker in markers:
        if marker not in text:
            fail(f"{label} missing marker: {marker}")


def main() -> None:
    manifest = MANIFEST.read_text(encoding="utf-8") if MANIFEST.is_file() else fail("missing manifest")
    for ticket in [f"VER-{number:03d}" for number in range(1, 11)]:
        if f"| {ticket} |" not in manifest:
            fail(f"manifest does not map {ticket}")
    require_markers("manifest", manifest, ["**Decision:** **PASS", CANDIDATE, RUN_ID, "309 passed", "Three-run repeatability"])

    subprocess.run(["git", "merge-base", "--is-ancestor", CANDIDATE, "HEAD"], cwd=ROOT, check=True)

    verifier = read("src/engine/verification.rs")
    structural = read("src/engine/verification_structural.rs")
    content = read("src/engine/verification_content.rs")
    workflow = read("src/engine/workflow.rs")
    legacy = read("src/engine/verification_v2.rs")
    vision = read("src/ai/vision.rs")
    pdfrest = read("src/ai/pdfrest.rs")
    calibration = json.loads(read("assets/verification-calibration-v2.json"))

    require_markers("verifier", verifier, [
        "mandatory_local_pass", "VerificationEvidencePackage", "calibration_manifest_sha256",
        "rendered", "expected_transactions", "VerificationGateStatus::Unavailable",
    ])
    require_markers("structural verifier", structural, ["page_count", "media_box", "crop_box", "rotation", "fonts", "metadata"])
    require_markers("content verifier", content, ["old_text", "new_text", "expected exactly one", "stale_old_matches"])
    require_markers("immutable policy", workflow + legacy, ["mask_padding_for_attempt", "caller_threshold_overrides_are_ignored"])
    require_markers("Vision provider", vision, ["Unavailable", "timeout", "missing_key"])
    require_markers("pdfRest provider", pdfrest, ["polling", "timeout", "PNG"])

    if calibration.get("schema_version") != 1:
        fail("calibration manifest schema is not the supported version")
    if calibration.get("renderer", {}).get("adaptive_thresholds") is not False:
        fail("adaptive thresholds are not disabled")
    if calibration.get("renderer", {}).get("adaptive_mask_padding") is not False:
        fail("adaptive mask padding is not disabled")

    tests = {
        "tests/verification_structural_tests.rs": ["page_count_and_order_negative_controls_fail", "blank_page_and_geometry_drift_fail", "font_and_metadata_policy_negative_controls_fail"],
        "tests/verification_content_tests.rs": ["stale_or_wrong_replacement_text_fails", "duplicate_replacement_is_over_applied_and_fails", "blanked_target_without_replacement_fails"],
        "tests/engine_verification_tests.rs": ["evidence_persistence_failure_blocks_verification_result"],
        "tests/verifier_cli_contract.rs": ["identical_pair_exits_success", "unrequested_visible_mutation"],
        "tests/verification_repeatability.rs": ["bitwise_repeatable_across_three_runs"],
    }
    for path, markers in tests.items():
        require_markers(path, read(path), markers)

    live = verifier + read("src/app/gui.rs") + read("src/app/modals.rs")
    for prohibited in ["100% Fidelity", "perfect fidelity", "Proceed anyway"]:
        if prohibited in live:
            fail(f"live verifier surface retains prohibited claim: {prohibited}")

    print("Gate 07 PASS")


if __name__ == "__main__":
    main()
