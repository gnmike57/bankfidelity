#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))

from bridge_protocol import OPERATIONS, PROTOCOL_VERSION, build_response, parse_request  # noqa: E402

OUTPUT = ROOT / "python" / "contract_fixtures" / "v1" / "golden_operations.json"
INPUT_HASH = "11" * 32
OUTPUT_HASH = "22" * 32

PAYLOADS = {
    "ping": {},
    "get_text_blocks": {"pdf_path": "fixtures/input.pdf", "page_num": 0},
    "replace_text_in_rect": {
        "pdf_path": "fixtures/input.pdf",
        "output_path": "scratch/output.pdf",
        "page_num": 0,
        "rect": [10.0, 20.0, 110.0, 40.0],
        "old_text": "original",
        "new_text": "replacement",
        "font_path": None,
    },
    "find_text_block_at_click": {
        "pdf_path": "fixtures/input.pdf",
        "page_num": 0,
        "x": 50.0,
        "y": 30.0,
    },
    "get_all_transactions": {"pdf_path": "fixtures/input.pdf"},
    "analyze_document_layout": {"pdf_path": "fixtures/input.pdf"},
    "complete_font_with_adaption": {
        "pdf_path": "fixtures/input.pdf",
        "font_name": "FixtureFont",
    },
    "deep_font_replication": {
        "pdf_path": "fixtures/input.pdf",
        "font_name": "FixtureFont",
        "output_dir": "scratch/fonts",
    },
    "apply_many_edits": {
        "pdf_path": "fixtures/input.pdf",
        "output_path": "scratch/output.pdf",
        "edits": [
            {
                "page": 0,
                "rect": [10.0, 20.0, 110.0, 40.0],
                "old_text": "original",
                "new_text": "replacement",
            }
        ],
        "font_path": None,
    },
    "chunk_pdf_for_docai": {
        "pdf_path": "fixtures/input.pdf",
        "output_dir": "scratch/chunks",
        "max_pages_per_chunk": 30,
    },
    "analyze_fonts": {"pdf_path": "fixtures/input.pdf"},
    "replicate_font_for_missing_chars": {
        "pdf_path": "fixtures/input.pdf",
        "font_name": "FixtureFont",
        "missing_chars": ["€"],
        "output_dir": "scratch/fonts",
    },
    "clone_pages": {
        "pdf_path": "fixtures/input.pdf",
        "output_path": "scratch/output.pdf",
        "page_indices": [0],
    },
    "remove_pages": {
        "pdf_path": "fixtures/input.pdf",
        "output_path": "scratch/output.pdf",
        "page_indices": [1],
    },
    "render_page_to_png": {
        "pdf_path": "fixtures/input.pdf",
        "page_num": 0,
        "dpi": 144.0,
    },
}


def make_request(operation: str, index: int) -> dict[str, object]:
    request = {
        "protocol_version": PROTOCOL_VERSION,
        "operation_id": f"00000000-0000-4000-8000-{index:012d}",
        "operation": operation,
        "submitted_at_unix_ms": 1_000,
        "deadline_unix_ms": 61_000,
        "input_sha256": None if operation == "ping" else INPUT_HASH,
        "payload": PAYLOADS[operation],
    }
    return parse_request(request)


def build_fixture() -> dict[str, object]:
    cases: list[dict[str, object]] = []
    mutating = {"replace_text_in_rect", "apply_many_edits", "clone_pages", "remove_pages"}
    pro_operations = {
        "replace_text_in_rect",
        "complete_font_with_adaption",
        "deep_font_replication",
        "apply_many_edits",
        "replicate_font_for_missing_chars",
    }
    for index, operation in enumerate(OPERATIONS, start=1):
        request = make_request(operation, index)
        is_mutating = operation in mutating
        requested = 1 if is_mutating else None
        response = build_response(
            request,
            disposition="succeeded",
            capability_tier="pro" if operation in pro_operations else "core",
            output_sha256=OUTPUT_HASH if is_mutating else None,
            requested_count=requested,
            applied_count=requested,
            metrics={
                "duration_ms": index,
                "rss_before_bytes": 1_000_000,
                "rss_after_bytes": 1_000_000,
                "open_handles_before": 4,
                "open_handles_after": 4,
                "gc_collections": 0,
            },
            payload={"fixture": "ok"},
        )
        cases.append({"request": request, "response": response})
    return {"protocol_version": PROTOCOL_VERSION, "cases": cases}


def render() -> str:
    return json.dumps(build_fixture(), indent=2, ensure_ascii=False, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = render()
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_text(encoding="utf-8") != expected:
            print(f"fixture drift: run {Path(__file__).relative_to(ROOT)}", file=sys.stderr)
            return 1
        print(f"Python protocol fixtures are current: {len(OPERATIONS)} operations")
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(expected, encoding="utf-8")
    print(f"wrote {OUTPUT.relative_to(ROOT)} with {len(OPERATIONS)} operations")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
