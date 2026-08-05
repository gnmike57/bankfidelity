#!/usr/bin/env python3
"""Supervised JSON-lines worker for the permanent PyMuPDF production pipeline."""

from __future__ import annotations

import contextlib
import gc
import hashlib
import importlib
import importlib.metadata
import json
import os
import platform
import sys
import time
import traceback
import uuid
from pathlib import Path
from typing import Any, Mapping

from verify_runtime_manifest import verify as verify_runtime_manifest

from bridge_protocol import (
    OPERATIONS,
    PROTOCOL_VERSION,
    ProtocolError,
    build_response,
    canonical_json,
    parse_request,
)


def _sha256_file(path: str | os.PathLike[str]) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _rss_bytes() -> int | None:
    try:
        import resource

        rss = int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
        return rss if sys.platform == "darwin" else rss * 1024
    except (ImportError, OSError, ValueError):
        return None


def _open_handles() -> int | None:
    try:
        fd_dir = Path("/proc/self/fd")
        return len(tuple(fd_dir.iterdir())) if fd_dir.is_dir() else None
    except OSError:
        return None


def _gc_collections() -> int:
    try:
        return sum(int(item.get("collections", 0)) for item in gc.get_stats())
    except (AttributeError, TypeError, ValueError):
        return 0


MUTATING_OPERATIONS = frozenset(
    {"replace_text_in_rect", "apply_many_edits", "clone_pages", "remove_pages"}
)


class MutationTransaction:
    """Publish one worker-produced PDF atomically or leave prior bytes untouched."""

    def __init__(self, final_path: Path) -> None:
        self.final_path = final_path
        self.final_path.parent.mkdir(parents=True, exist_ok=True)
        self.stage_path = self.final_path.with_name(
            f".{self.final_path.stem}.{uuid.uuid4().hex}.worker-stage"
            f"{self.final_path.suffix or '.pdf'}"
        )

    @classmethod
    def prepare(
        cls, request: Mapping[str, Any]
    ) -> tuple[dict[str, Any], "MutationTransaction | None"]:
        if request["operation"] not in MUTATING_OPERATIONS:
            return dict(request), None
        payload = dict(request["payload"])
        output_path = payload.get("output_path")
        if not isinstance(output_path, str) or not output_path:
            raise ProtocolError("OUTPUT_PATH_REQUIRED", "mutation requires output_path")
        transaction = cls(Path(output_path))
        payload["output_path"] = str(transaction.stage_path)
        staged_request = dict(request)
        staged_request["payload"] = payload
        return staged_request, transaction

    def commit(self, operation: str, requested: int, applied: int) -> dict[str, Any]:
        if not self.stage_path.is_file():
            raise ProtocolError(
                "OUTPUT_ARTIFACT_MISSING",
                f"{operation} did not create the staged output artifact",
            )
        # Windows requires a write-capable handle for FlushFileBuffers/fsync.
        with self.stage_path.open("r+b") as stream:
            stream.flush()
            os.fsync(stream.fileno())
        size_bytes = self.stage_path.stat().st_size
        if size_bytes <= 0:
            raise ProtocolError("OUTPUT_ARTIFACT_EMPTY", "staged output is empty")
        sha256 = _sha256_file(self.stage_path)
        os.replace(self.stage_path, self.final_path)
        directory_fd = None
        if os.name != "nt":
            try:
                directory_fd = os.open(self.final_path.parent, os.O_RDONLY)
            except OSError:
                directory_fd = None
        if directory_fd is not None:
            try:
                os.fsync(directory_fd)
            except OSError:
                # Some Unix filesystems do not support fsync on directories.
                pass
            finally:
                os.close(directory_fd)
        return {
            "path": str(self.final_path),
            "sha256": sha256,
            "size_bytes": size_bytes,
            "operation": operation,
            "requested_count": requested,
            "applied_count": applied,
            "committed": True,
        }

    def cleanup(self) -> None:
        try:
            self.stage_path.unlink(missing_ok=True)
        except OSError:
            pass


class WorkerRuntime:
    def __init__(self) -> None:
        self.bridge: Any | None = None
        self.bridge_error_class: str | None = None
        self.runtime_manifest: dict[str, Any] | None = None
        try:
            self.runtime_manifest = verify_runtime_manifest("base")
            # stdout is the machine-readable JSON-lines transport. Optional Pro
            # packages can print license banners while importing, so route all
            # third-party diagnostics to stderr and keep the protocol pristine.
            with contextlib.redirect_stdout(sys.stderr):
                self.bridge = importlib.import_module("pymupdf_pro_integration")
        except BaseException as error:  # startup must report even loader-level failures
            self.bridge_error_class = type(error).__name__

    def handshake(self) -> dict[str, Any]:
        pymupdf_version = None
        pymupdf_pro_version = None
        pro_available = False
        pro_version_compatible = False
        pro_error_class = None
        if self.bridge is not None:
            version = getattr(self.bridge.pymupdf, "version", None)
            if isinstance(version, tuple):
                pymupdf_version = str(version[0]) if version else None
            elif version is not None:
                pymupdf_version = str(version)
            pro_available = bool(
                getattr(self.bridge, "_PYMUPDF_PRO_AVAILABLE", False)
            )
            pro_error = getattr(self.bridge, "_PYMUPDF_PRO_IMPORT_ERROR", None)
            if pro_error is not None:
                pro_error_class = type(pro_error).__name__
            try:
                pymupdf_pro_version = importlib.metadata.version("PyMuPDFPro")
            except importlib.metadata.PackageNotFoundError:
                pymupdf_pro_version = None
            pro_version_compatible = bool(
                pro_available
                and pymupdf_version
                and pymupdf_pro_version
                and pymupdf_version == pymupdf_pro_version
            )
            if pro_available and not pro_version_compatible:
                pro_available = False
                pro_error_class = "PyMuPDFProVersionMismatch"
        return {
            "event": "handshake",
            "protocol_version": PROTOCOL_VERSION,
            "worker_pid": os.getpid(),
            "python_version": platform.python_version(),
            "platform": sys.platform,
            "ready": self.bridge is not None,
            "bridge_error_class": self.bridge_error_class,
            "pymupdf_version": pymupdf_version,
            "pymupdf_pro_version": pymupdf_pro_version,
            "pro_version_compatible": pro_version_compatible,
            "pro_package_available": pro_available,
            "pro_import_error_class": pro_error_class,
            "operations": list(OPERATIONS),
        }

    def execute(self, request_raw: str | bytes | Mapping[str, Any]) -> dict[str, Any]:
        request = parse_request(request_raw)
        start_ns = time.monotonic_ns()
        rss_before = _rss_bytes()
        handles_before = _open_handles()
        gc_before = _gc_collections()
        transaction: MutationTransaction | None = None
        try:
            self._verify_input_hash(request)
            staged_request, transaction = MutationTransaction.prepare(request)
            outcome = self._dispatch(staged_request)
            if transaction is not None:
                if (
                    outcome["disposition"] == "succeeded"
                    and outcome.get("requested_count") == outcome.get("applied_count")
                ):
                    artifact = transaction.commit(
                        request["operation"],
                        int(outcome["requested_count"]),
                        int(outcome["applied_count"]),
                    )
                    outcome["output_sha256"] = artifact["sha256"]
                    outcome.setdefault("payload", {})["artifact"] = artifact
                else:
                    transaction.cleanup()
                    outcome["output_sha256"] = None
            response = build_response(
                request,
                disposition=outcome["disposition"],
                capability_tier=outcome["capability_tier"],
                output_sha256=outcome.get("output_sha256"),
                requested_count=outcome.get("requested_count"),
                applied_count=outcome.get("applied_count"),
                warnings=outcome.get("warnings"),
                metrics=self._metrics(start_ns, rss_before, handles_before, gc_before),
                payload=outcome.get("payload"),
                failure=outcome.get("failure"),
            )
        except BaseException as error:
            if transaction is not None:
                transaction.cleanup()
            response = build_response(
                request,
                disposition="failed",
                capability_tier=self._capability_tier(request["operation"]),
                metrics=self._metrics(start_ns, rss_before, handles_before, gc_before),
                failure=classify_error(error, request["operation"]),
            )
        return response

    @staticmethod
    def _verify_input_hash(request: Mapping[str, Any]) -> None:
        expected_hash = request.get("input_sha256")
        pdf_path = request["payload"].get("pdf_path")
        if expected_hash is None or not isinstance(pdf_path, str):
            return
        path = Path(pdf_path)
        if not path.is_file():
            raise FileNotFoundError(f"input PDF not found: {path.name}")
        actual_hash = _sha256_file(path)
        if actual_hash != expected_hash:
            raise ProtocolError(
                "INPUT_HASH_MISMATCH",
                f"input changed before {request['operation']} could run",
            )

    def _metrics(
        self,
        start_ns: int,
        rss_before: int | None,
        handles_before: int | None,
        gc_before: int,
    ) -> dict[str, Any]:
        return {
            "duration_ms": max(0, (time.monotonic_ns() - start_ns) // 1_000_000),
            "rss_before_bytes": rss_before,
            "rss_after_bytes": _rss_bytes(),
            "open_handles_before": handles_before,
            "open_handles_after": _open_handles(),
            "gc_collections": max(0, _gc_collections() - gc_before),
        }

    def _dispatch(self, request: Mapping[str, Any]) -> dict[str, Any]:
        operation = request["operation"]
        payload = request["payload"]
        if operation == "ping":
            return {
                "disposition": "succeeded",
                "capability_tier": "core",
                "payload": {"handshake": self.handshake()},
            }
        if self.bridge is None:
            raise RuntimeError(
                f"Python bridge unavailable ({self.bridge_error_class or 'unknown'})"
            )
        pdf_path = payload.get("pdf_path")
        if isinstance(pdf_path, str) and not Path(pdf_path).is_file():
            raise FileNotFoundError(f"input PDF not found: {Path(pdf_path).name}")

        method = {
            "get_text_blocks": lambda: self.bridge.get_text_blocks(
                payload["pdf_path"], payload["page_num"]
            ),
            "replace_text_in_rect": lambda: self.bridge.replace_text_in_rect(
                pdf_path=payload["pdf_path"],
                output_path=payload["output_path"],
                page_num=payload["page_num"],
                rect=payload["rect"],
                old_text=payload["old_text"],
                new_text=payload["new_text"],
                font_path=payload.get("font_path"),
            ),
            "find_text_block_at_click": lambda: self.bridge.find_text_block_at_click(
                payload["pdf_path"],
                payload["page_num"],
                payload["x"],
                payload["y"],
                72.0,
            ),
            "get_all_transactions": lambda: self.bridge.get_all_transactions(
                payload["pdf_path"]
            ),
            "analyze_document_layout": lambda: self.bridge.analyze_document_layout(
                payload["pdf_path"]
            ),
            "complete_font_with_adaption": lambda: self.bridge.complete_font_with_adaption_fallback(
                payload["pdf_path"], payload["font_name"]
            ),
            "deep_font_replication": lambda: self.bridge.deep_font_replication_api(
                payload["pdf_path"], payload["font_name"], payload["output_dir"]
            ),
            "apply_many_edits": lambda: self.bridge.apply_many_edits(
                payload["pdf_path"],
                payload["output_path"],
                payload["edits"],
                payload.get("font_path"),
            ),
            "chunk_pdf_for_docai": lambda: self.bridge.chunk_pdf_for_docai(
                payload["pdf_path"],
                payload["output_dir"],
                payload["max_pages_per_chunk"],
            ),
            "analyze_fonts": lambda: self.bridge.analyze_fonts(payload["pdf_path"]),
            "replicate_font_for_missing_chars": lambda: self.bridge.replicate_font_for_missing_chars(
                payload["pdf_path"],
                payload["font_name"],
                ",".join(payload["missing_chars"]),
                payload["output_dir"],
            ),
            "clone_pages": lambda: self.bridge.clone_pages(
                payload["pdf_path"], payload["output_path"], payload["page_indices"]
            ),
            "remove_pages": lambda: self.bridge.remove_pages(
                payload["pdf_path"], payload["output_path"], payload["page_indices"]
            ),
            "render_page_to_png": lambda: self.bridge.render_page_to_png(
                payload["pdf_path"], payload["page_num"], payload["dpi"]
            ),
        }.get(operation)
        if method is None:
            raise ProtocolError("UNSUPPORTED_OPERATION", f"unsupported operation: {operation}")
        # Bridge implementations and native extensions may print warnings or
        # license notices. stdout belongs exclusively to protocol envelopes.
        with contextlib.redirect_stdout(sys.stderr):
            result = method()
        return self._outcome(operation, payload, result)

    def _outcome(
        self, operation: str, payload: Mapping[str, Any], result: Any
    ) -> dict[str, Any]:
        outcome: dict[str, Any] = {
            "disposition": "succeeded",
            "capability_tier": self._capability_tier(operation),
            "payload": {"result": result},
        }
        if operation not in {
            "replace_text_in_rect",
            "apply_many_edits",
            "clone_pages",
            "remove_pages",
        }:
            return outcome

        output_path = payload["output_path"]
        requested = self._requested_count(operation, payload)
        applied = self._applied_count(operation, result)
        output_hash = _sha256_file(output_path) if Path(output_path).is_file() else None
        outcome.update(
            requested_count=requested,
            applied_count=applied,
            output_sha256=output_hash,
        )
        success_flag = not isinstance(result, dict) or bool(result.get("success", True))
        if success_flag and output_hash is not None and requested == applied:
            return outcome
        outcome["disposition"] = "partial" if applied > 0 else "failed"
        outcome["failure"] = {
            "code": "PYTHON_MUTATION_INCOMPLETE",
            "class": "MutationIncomplete",
            "message": f"applied {applied} of {requested} requested changes",
            "retryable": False,
            "context": {"requested_count": requested, "applied_count": applied},
        }
        if outcome["disposition"] == "partial":
            # Partial is not a failure disposition in protocol v1, so carry the
            # explanatory evidence as a warning rather than a failure object.
            failure = outcome.pop("failure")
            outcome["warnings"] = [
                {"code": failure["code"], "message": failure["message"]}
            ]
        return outcome

    @staticmethod
    def _requested_count(operation: str, payload: Mapping[str, Any]) -> int:
        if operation == "replace_text_in_rect":
            return 1
        if operation == "apply_many_edits":
            return len(payload["edits"])
        return len(payload["page_indices"])

    @staticmethod
    def _applied_count(operation: str, result: Any) -> int:
        if not isinstance(result, dict):
            return 1 if operation == "replace_text_in_rect" else 0
        if operation == "apply_many_edits":
            return int(result.get("placed", 0))
        if operation == "clone_pages":
            return int(result.get("cloned", 0))
        if operation == "remove_pages":
            return int(result.get("removed", 0))
        return 1 if result.get("success", True) else 0

    @staticmethod
    def _capability_tier(operation: str) -> str:
        return (
            "pro"
            if operation
            in {
                "replace_text_in_rect",
                "complete_font_with_adaption",
                "deep_font_replication",
                "apply_many_edits",
                "replicate_font_for_missing_chars",
            }
            else "core"
        )


def classify_error(error: BaseException, operation: str) -> dict[str, Any]:
    """Map every Python/backend exception to a stable protocol failure."""
    message = str(error) or type(error).__name__
    code = "PYTHON_OPERATION_FAILED"
    retryable = False
    for token in (
        "PRO_PAGE_LIMIT_EXCEEDED",
        "FONT_COVERAGE_INSUFFICIENT",
        "FONT_EMBEDDING_UNAVAILABLE",
        "PDF_NOT_EDITABLE",
    ):
        if token in message:
            code = token
            break
    else:
        if isinstance(error, ProtocolError):
            code = error.code
        elif isinstance(error, FileNotFoundError):
            code = "INPUT_NOT_FOUND"
        elif isinstance(error, PermissionError):
            code = "PERMISSION_DENIED"
        elif isinstance(error, TimeoutError):
            code = "PYTHON_TIMEOUT"
            retryable = True
        elif isinstance(error, InterruptedError):
            code = "PYTHON_INTERRUPTED"
            retryable = True
        elif isinstance(error, ConnectionError):
            code = "PYTHON_CONNECTION_ERROR"
            retryable = True
        elif isinstance(error, MemoryError):
            code = "PYTHON_MEMORY_EXHAUSTED"
        elif isinstance(error, OSError):
            code = "PYTHON_IO_ERROR"
        elif isinstance(error, ValueError):
            code = "PYTHON_INVALID_VALUE"
        elif isinstance(error, RuntimeError):
            code = "PYTHON_RUNTIME_ERROR"
        elif not isinstance(error, Exception):
            code = "PYTHON_WORKER_ABORTED"
    return {
        "code": code,
        "class": type(error).__name__,
        "message": message,
        "retryable": retryable,
        "context": {"operation": operation},
    }


def _isolate_protocol_output():
    """Reserve original stdout for JSON and quarantine all other output.

    Some native extensions write license banners directly to file descriptor 1,
    bypassing ``contextlib.redirect_stdout``. Duplicate the original descriptor
    for protocol envelopes, then redirect descriptor 1 to stderr before loading
    the bridge. This keeps the transport valid on Windows, macOS, and Linux.
    """
    try:
        protocol_fd = os.dup(sys.stdout.fileno())
        protocol_output = os.fdopen(
            protocol_fd,
            "w",
            encoding=sys.stdout.encoding or "utf-8",
            errors="backslashreplace",
            buffering=1,
        )
        os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
        return protocol_output
    except (AttributeError, OSError, ValueError):
        return sys.stdout


def _emit(value: Mapping[str, Any], output=None) -> None:
    stream = sys.stdout if output is None else output
    stream.write(canonical_json(value) + "\n")
    stream.flush()


def main() -> int:
    protocol_output = _isolate_protocol_output()
    try:
        runtime = WorkerRuntime()
        _emit(runtime.handshake(), protocol_output)
        for line in sys.stdin.buffer:
            if not line.strip():
                continue
            try:
                _emit(runtime.execute(line), protocol_output)
            except ProtocolError as error:
                _emit(
                    {
                        "event": "protocol_error",
                        "code": error.code,
                        "class": type(error).__name__,
                        "message": str(error),
                    },
                    protocol_output,
                )
            except BaseException as error:
                _emit(
                    {
                        "event": "worker_error",
                        "code": "WORKER_INTERNAL_ERROR",
                        "class": type(error).__name__,
                        "message": str(error) or type(error).__name__,
                    },
                    protocol_output,
                )
                traceback.print_exc(file=sys.stderr)
        return 0
    finally:
        if protocol_output is not sys.stdout:
            protocol_output.close()


if __name__ == "__main__":
    raise SystemExit(main())
