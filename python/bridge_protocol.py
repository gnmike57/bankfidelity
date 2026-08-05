"""Versioned Rust/Python operation protocol for the permanent PyMuPDF pipeline.

This module is intentionally standard-library-only so it can run before PyMuPDF is
imported and can diagnose an incompatible or damaged bundled Python runtime.
"""

from __future__ import annotations

import json
import re
import uuid
from typing import Any, Mapping

PROTOCOL_VERSION = "1.0.0"
OPERATIONS = (
    "ping",
    "get_text_blocks",
    "replace_text_in_rect",
    "find_text_block_at_click",
    "get_all_transactions",
    "analyze_document_layout",
    "complete_font_with_adaption",
    "deep_font_replication",
    "apply_many_edits",
    "chunk_pdf_for_docai",
    "analyze_fonts",
    "replicate_font_for_missing_chars",
    "clone_pages",
    "remove_pages",
    "render_page_to_png",
)
MUTATING_OPERATIONS = {
    "replace_text_in_rect",
    "apply_many_edits",
    "clone_pages",
    "remove_pages",
}
DISPOSITIONS = {
    "succeeded",
    "no_op",
    "partial",
    "failed",
    "cancelled",
    "timed_out",
}
FAILURE_DISPOSITIONS = {"failed", "cancelled", "timed_out"}
CAPABILITY_TIERS = {"core", "pro"}
_SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")

_REQUEST_KEYS = {
    "protocol_version",
    "operation_id",
    "operation",
    "submitted_at_unix_ms",
    "deadline_unix_ms",
    "input_sha256",
    "payload",
}
_RESPONSE_KEYS = {
    "protocol_version",
    "operation_id",
    "operation",
    "disposition",
    "input_sha256",
    "output_sha256",
    "requested_count",
    "applied_count",
    "capability_tier",
    "warnings",
    "metrics",
    "payload",
    "failure",
}
_WARNING_KEYS = {"code", "message"}
_FAILURE_KEYS = {"code", "class", "message", "retryable", "context"}
_METRIC_KEYS = {
    "duration_ms",
    "rss_before_bytes",
    "rss_after_bytes",
    "open_handles_before",
    "open_handles_after",
    "gc_collections",
}


class ProtocolError(ValueError):
    """A deterministic version, shape, identity, or invariant violation."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def canonical_json(value: Mapping[str, Any]) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def parse_request(raw: str | bytes | Mapping[str, Any]) -> dict[str, Any]:
    request = _load_object(raw, "request")
    _require_exact_keys(request, _REQUEST_KEYS, "request")
    _require_protocol(request["protocol_version"])
    _require_operation_id(request["operation_id"])
    _require_choice(request["operation"], OPERATIONS, "operation")
    submitted = _require_uint(request["submitted_at_unix_ms"], "submitted_at_unix_ms")
    deadline = _require_uint(request["deadline_unix_ms"], "deadline_unix_ms")
    if deadline < submitted:
        raise ProtocolError("INVALID_DEADLINE", "deadline precedes submission time")
    _require_optional_sha256(request["input_sha256"], "input_sha256")
    if not isinstance(request["payload"], dict):
        raise ProtocolError("INVALID_PAYLOAD", "request payload must be an object")
    return request


def parse_response(raw: str | bytes | Mapping[str, Any]) -> dict[str, Any]:
    response = _load_object(raw, "response")
    _require_exact_keys(response, _RESPONSE_KEYS, "response")
    _require_protocol(response["protocol_version"])
    _require_operation_id(response["operation_id"])
    operation = _require_choice(response["operation"], OPERATIONS, "operation")
    disposition = _require_choice(response["disposition"], DISPOSITIONS, "disposition")
    _require_optional_sha256(response["input_sha256"], "input_sha256")
    _require_optional_sha256(response["output_sha256"], "output_sha256")
    _require_choice(response["capability_tier"], CAPABILITY_TIERS, "capability_tier")

    requested = _require_optional_uint(response["requested_count"], "requested_count")
    applied = _require_optional_uint(response["applied_count"], "applied_count")
    if (requested is None) != (applied is None):
        raise ProtocolError(
            "INCOMPLETE_COUNT_EVIDENCE",
            "requested_count and applied_count must appear together",
        )
    if requested is not None and applied is not None and applied > requested:
        raise ProtocolError(
            "INVALID_APPLIED_COUNT",
            f"applied_count {applied} exceeds requested_count {requested}",
        )

    warnings = response["warnings"]
    if not isinstance(warnings, list):
        raise ProtocolError("INVALID_WARNINGS", "warnings must be an array")
    for index, warning in enumerate(warnings):
        if not isinstance(warning, dict):
            raise ProtocolError("INVALID_WARNING", f"warning {index} must be an object")
        _require_exact_keys(warning, _WARNING_KEYS, f"warning {index}")
        _require_nonempty_string(warning["code"], f"warning {index}.code")
        _require_nonempty_string(warning["message"], f"warning {index}.message")

    metrics = response["metrics"]
    if not isinstance(metrics, dict):
        raise ProtocolError("INVALID_METRICS", "metrics must be an object")
    _require_exact_keys(metrics, _METRIC_KEYS, "metrics")
    _require_uint(metrics["duration_ms"], "metrics.duration_ms")
    _require_optional_uint(metrics["rss_before_bytes"], "metrics.rss_before_bytes")
    _require_optional_uint(metrics["rss_after_bytes"], "metrics.rss_after_bytes")
    _require_optional_uint(metrics["open_handles_before"], "metrics.open_handles_before")
    _require_optional_uint(metrics["open_handles_after"], "metrics.open_handles_after")
    _require_uint(metrics["gc_collections"], "metrics.gc_collections")

    if not isinstance(response["payload"], dict):
        raise ProtocolError("INVALID_PAYLOAD", "response payload must be an object")

    failure = response["failure"]
    if disposition in FAILURE_DISPOSITIONS:
        if not isinstance(failure, dict):
            raise ProtocolError("MISSING_FAILURE", "failure disposition requires failure detail")
        _require_exact_keys(failure, _FAILURE_KEYS, "failure")
        _require_nonempty_string(failure["code"], "failure.code")
        _require_nonempty_string(failure["class"], "failure.class")
        _require_nonempty_string(failure["message"], "failure.message")
        if not isinstance(failure["retryable"], bool):
            raise ProtocolError("INVALID_FAILURE", "failure.retryable must be a boolean")
        if not isinstance(failure["context"], dict):
            raise ProtocolError("INVALID_FAILURE", "failure.context must be an object")
    elif failure is not None:
        raise ProtocolError(
            "UNEXPECTED_FAILURE", "non-failure disposition cannot contain failure detail"
        )

    if disposition == "succeeded" and operation in MUTATING_OPERATIONS:
        if response["output_sha256"] is None:
            raise ProtocolError(
                "MISSING_OUTPUT_HASH", "successful mutation requires output_sha256"
            )
        if requested is not None and requested != applied:
            raise ProtocolError(
                "SUCCESS_COUNT_MISMATCH",
                f"successful operation applied {applied} of {requested} requested changes",
            )
    return response


def validate_response_for_request(
    request_raw: str | bytes | Mapping[str, Any],
    response_raw: str | bytes | Mapping[str, Any],
) -> dict[str, Any]:
    request = parse_request(request_raw)
    response = parse_response(response_raw)
    if response["operation_id"] != request["operation_id"]:
        raise ProtocolError("OPERATION_ID_MISMATCH", "response operation_id mismatch")
    if response["operation"] != request["operation"]:
        raise ProtocolError("OPERATION_MISMATCH", "response operation mismatch")
    if response["input_sha256"] != request["input_sha256"]:
        raise ProtocolError("INPUT_HASH_MISMATCH", "response input_sha256 mismatch")
    return response


def build_response(
    request_raw: str | bytes | Mapping[str, Any],
    *,
    disposition: str,
    capability_tier: str,
    output_sha256: str | None = None,
    requested_count: int | None = None,
    applied_count: int | None = None,
    warnings: list[dict[str, str]] | None = None,
    metrics: Mapping[str, Any] | None = None,
    payload: Mapping[str, Any] | None = None,
    failure: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    request = parse_request(request_raw)
    response = {
        "protocol_version": PROTOCOL_VERSION,
        "operation_id": request["operation_id"],
        "operation": request["operation"],
        "disposition": disposition,
        "input_sha256": request["input_sha256"],
        "output_sha256": output_sha256,
        "requested_count": requested_count,
        "applied_count": applied_count,
        "capability_tier": capability_tier,
        "warnings": list(warnings or []),
        "metrics": dict(
            metrics
            or {
                "duration_ms": 0,
                "rss_before_bytes": None,
                "rss_after_bytes": None,
                "open_handles_before": None,
                "open_handles_after": None,
                "gc_collections": 0,
            }
        ),
        "payload": dict(payload or {}),
        "failure": dict(failure) if failure is not None else None,
    }
    return validate_response_for_request(request, response)


def _load_object(raw: str | bytes | Mapping[str, Any], name: str) -> dict[str, Any]:
    if isinstance(raw, Mapping):
        value = dict(raw)
    else:
        try:
            value = json.loads(raw)
        except (TypeError, UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ProtocolError("MALFORMED_JSON", f"malformed {name}: {error}") from error
    if not isinstance(value, dict):
        raise ProtocolError("INVALID_ENVELOPE", f"{name} must be an object")
    return value


def _require_exact_keys(value: Mapping[str, Any], expected: set[str], name: str) -> None:
    actual = set(value)
    missing = expected - actual
    unknown = actual - expected
    if missing or unknown:
        raise ProtocolError(
            "INVALID_FIELDS",
            f"{name} fields mismatch; missing={sorted(missing)} unknown={sorted(unknown)}",
        )


def _require_protocol(value: Any) -> None:
    if value != PROTOCOL_VERSION:
        raise ProtocolError("UNSUPPORTED_VERSION", f"unsupported protocol version: {value!r}")


def _require_operation_id(value: Any) -> None:
    if not isinstance(value, str):
        raise ProtocolError("INVALID_OPERATION_ID", "operation_id must be a UUID string")
    try:
        parsed = uuid.UUID(value)
    except (ValueError, AttributeError) as error:
        raise ProtocolError("INVALID_OPERATION_ID", "operation_id must be a UUID") from error
    if parsed.int == 0:
        raise ProtocolError("INVALID_OPERATION_ID", "operation_id must not be nil")


def _require_choice(value: Any, choices: Any, name: str) -> str:
    if not isinstance(value, str) or value not in choices:
        raise ProtocolError("INVALID_CHOICE", f"invalid {name}: {value!r}")
    return value


def _require_nonempty_string(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ProtocolError("INVALID_STRING", f"{name} must be a non-empty string")
    return value


def _require_uint(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ProtocolError("INVALID_UINT", f"{name} must be a non-negative integer")
    return value


def _require_optional_uint(value: Any, name: str) -> int | None:
    if value is None:
        return None
    return _require_uint(value, name)


def _require_optional_sha256(value: Any, name: str) -> None:
    if value is not None and (not isinstance(value, str) or _SHA256.fullmatch(value) is None):
        raise ProtocolError("INVALID_SHA256", f"{name} must be a hexadecimal SHA-256")
