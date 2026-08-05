from __future__ import annotations

import copy
import json
import unittest
from pathlib import Path

from bridge_protocol import (
    OPERATIONS,
    ProtocolError,
    build_response,
    parse_request,
    parse_response,
    validate_response_for_request,
)

FIXTURE_PATH = (
    Path(__file__).resolve().parent
    / "contract_fixtures"
    / "v1"
    / "golden_operations.json"
)


class BridgeProtocolTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fixture = json.loads(FIXTURE_PATH.read_text(encoding="utf-8"))

    def test_all_operations_round_trip_through_golden_contract(self) -> None:
        cases = self.fixture["cases"]
        self.assertEqual([case["request"]["operation"] for case in cases], list(OPERATIONS))
        for case in cases:
            with self.subTest(operation=case["request"]["operation"]):
                request = parse_request(case["request"])
                response = parse_response(case["response"])
                self.assertEqual(validate_response_for_request(request, response), response)

    def test_request_and_response_reject_unknown_fields(self) -> None:
        case = copy.deepcopy(self.fixture["cases"][0])
        case["request"]["unknown"] = True
        with self.assertRaisesRegex(ProtocolError, "unknown=\\['unknown'\\]"):
            parse_request(case["request"])

        case = copy.deepcopy(self.fixture["cases"][0])
        case["response"]["unknown"] = True
        with self.assertRaisesRegex(ProtocolError, "unknown=\\['unknown'\\]"):
            parse_response(case["response"])

    def test_protocol_version_and_operation_identity_are_exact(self) -> None:
        case = copy.deepcopy(self.fixture["cases"][0])
        case["request"]["protocol_version"] = "2.0.0"
        with self.assertRaisesRegex(ProtocolError, "unsupported protocol version"):
            parse_request(case["request"])

        case = copy.deepcopy(self.fixture["cases"][0])
        case["response"]["operation_id"] = "00000000-0000-4000-8000-999999999999"
        with self.assertRaisesRegex(ProtocolError, "operation_id mismatch"):
            validate_response_for_request(case["request"], case["response"])

    def test_failure_dispositions_require_typed_failure_detail(self) -> None:
        request = self.fixture["cases"][0]["request"]
        with self.assertRaisesRegex(ProtocolError, "requires failure detail"):
            build_response(
                request,
                disposition="failed",
                capability_tier="core",
            )

        response = build_response(
            request,
            disposition="failed",
            capability_tier="core",
            failure={
                "code": "PYTHON_OPERATION_FAILED",
                "class": "RuntimeError",
                "message": "fixture failure",
                "retryable": False,
                "context": {},
            },
        )
        self.assertEqual(response["failure"]["code"], "PYTHON_OPERATION_FAILED")

    def test_successful_mutation_requires_hash_and_exact_counts(self) -> None:
        request = next(
            case["request"]
            for case in self.fixture["cases"]
            if case["request"]["operation"] == "apply_many_edits"
        )
        with self.assertRaisesRegex(ProtocolError, "requires output_sha256"):
            build_response(
                request,
                disposition="succeeded",
                capability_tier="pro",
                requested_count=1,
                applied_count=1,
            )
        with self.assertRaisesRegex(ProtocolError, "applied 0 of 1"):
            build_response(
                request,
                disposition="succeeded",
                capability_tier="pro",
                output_sha256="22" * 32,
                requested_count=1,
                applied_count=0,
            )


if __name__ == "__main__":
    unittest.main()
