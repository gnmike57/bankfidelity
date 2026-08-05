from __future__ import annotations

import hashlib
import importlib.metadata
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

import pymupdf

from bridge_protocol import OPERATIONS, PROTOCOL_VERSION, ProtocolError, parse_response
from worker import classify_error

ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "python" / "worker.py"


def pro_package_available() -> bool:
    try:
        importlib.metadata.version("PyMuPDFPro")
        return True
    except importlib.metadata.PackageNotFoundError:
        return False


def request(operation: str, payload: dict[str, object]) -> dict[str, object]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "operation_id": "00000000-0000-4000-8000-000000000999",
        "operation": operation,
        "submitted_at_unix_ms": 1_000,
        "deadline_unix_ms": int(time.time() * 1000) + 60_000,
        "input_sha256": None,
        "payload": payload,
    }


class WorkerProcess:
    def __init__(self, extra_env: dict[str, str] | None = None) -> None:
        env = os.environ.copy()
        env["PYTHONPATH"] = str(ROOT / "python")
        env["PYTHONUNBUFFERED"] = "1"
        env.update(extra_env or {})
        self.process = subprocess.Popen(
            [sys.executable, str(WORKER)],
            cwd=ROOT,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        assert self.process.stdout is not None
        self.handshake = json.loads(self.process.stdout.readline())

    def send(self, value: object) -> dict[str, object]:
        assert self.process.stdin is not None
        assert self.process.stdout is not None
        line = value if isinstance(value, str) else json.dumps(value)
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()
        return json.loads(self.process.stdout.readline())

    def close(self) -> tuple[int, str]:
        assert self.process.stdin is not None
        assert self.process.stdout is not None
        assert self.process.stderr is not None
        self.process.stdin.close()
        code = self.process.wait(timeout=10)
        stderr = self.process.stderr.read()
        self.process.stdout.close()
        self.process.stderr.close()
        return code, stderr

    def terminate(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=10)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None and not stream.closed:
                stream.close()


class WorkerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.worker = WorkerProcess()

    def tearDown(self) -> None:
        self.worker.terminate()

    def test_handshake_and_ping_are_versioned_and_complete(self) -> None:
        handshake = self.worker.handshake
        self.assertEqual(handshake["event"], "handshake")
        self.assertEqual(handshake["protocol_version"], PROTOCOL_VERSION)
        self.assertEqual(handshake["operations"], list(OPERATIONS))
        self.assertIsInstance(handshake["worker_pid"], int)
        self.assertIn("ready", handshake)
        self.assertIn("pymupdf_version", handshake)

        response = parse_response(self.worker.send(request("ping", {})))
        self.assertEqual(response["disposition"], "succeeded")
        self.assertEqual(response["payload"]["handshake"]["worker_pid"], handshake["worker_pid"])

    def test_malformed_request_is_rejected_without_killing_worker(self) -> None:
        error = self.worker.send("{not-json")
        self.assertEqual(error["event"], "protocol_error")
        self.assertEqual(error["code"], "MALFORMED_JSON")
        response = parse_response(self.worker.send(request("ping", {})))
        self.assertEqual(response["disposition"], "succeeded")

    def test_operation_failure_is_typed_and_correlated(self) -> None:
        operation = request(
            "render_page_to_png",
            {
                "pdf_path": "fixtures/does-not-exist.pdf",
                "page_num": 0,
                "dpi": 144.0,
            },
        )
        response = parse_response(self.worker.send(operation))
        self.assertEqual(response["operation_id"], operation["operation_id"])
        self.assertEqual(response["operation"], "render_page_to_png")
        self.assertEqual(response["disposition"], "failed")
        self.assertIsNotNone(response["failure"])
        self.assertEqual(response["failure"]["code"], "INPUT_NOT_FOUND")
        self.assertEqual(response["failure"]["class"], "FileNotFoundError")
        self.assertNotIn("traceback", response["failure"]["context"])

    def test_core_text_extraction_succeeds_without_optional_pro(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "core-text.pdf"
            document = pymupdf.open()
            try:
                page = document.new_page()
                page.insert_text((72, 72), "CORE TEXT")
                document.save(source_path)
            finally:
                document.close()
            operation = request(
                "get_text_blocks",
                {"pdf_path": str(source_path), "page_num": 0},
            )
            response = parse_response(self.worker.send(operation))
            self.assertEqual(response["disposition"], "succeeded", response)
            spans = response["payload"]["result"]
            self.assertEqual([span["text"] for span in spans], ["CORE TEXT"])

    def test_pro_page_limit_cannot_be_bypassed_and_preserves_source(self) -> None:
        self.worker.terminate()
        self.worker = WorkerProcess({"IGNORE_PRO_LIMIT": "100"})
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "four-pages.pdf"
            output_path = Path(directory) / "should-not-exist.pdf"
            document = pymupdf.open()
            try:
                for page_index in range(4):
                    page = document.new_page()
                    page.insert_text((72, 72), f"PAGE {page_index + 1}")
                document.save(source_path)
            finally:
                document.close()
            before_hash = hashlib.sha256(source_path.read_bytes()).hexdigest()
            operation = request(
                "apply_many_edits",
                {
                    "pdf_path": str(source_path),
                    "output_path": str(output_path),
                    "edits": [
                        {
                            "page": 0,
                            "rect": [60.0, 50.0, 160.0, 85.0],
                            "old_text": "PAGE 1",
                            "new_text": "REPLACED",
                        }
                    ],
                    "font_path": None,
                },
            )
            response = parse_response(self.worker.send(operation))
            self.assertEqual(response["disposition"], "failed")
            self.assertEqual(response["failure"]["code"], "PRO_PAGE_LIMIT_EXCEEDED")
            self.assertEqual(hashlib.sha256(source_path.read_bytes()).hexdigest(), before_hash)
            self.assertFalse(output_path.exists())

    def test_input_hash_mismatch_preserves_existing_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "source.pdf"
            output_path = Path(directory) / "existing.pdf"
            document = pymupdf.open()
            try:
                page = document.new_page()
                page.insert_text((72, 72), "ORIGINAL")
                document.save(source_path)
            finally:
                document.close()
            sentinel = b"existing-output-must-survive"
            output_path.write_bytes(sentinel)
            operation = request(
                "clone_pages",
                {
                    "pdf_path": str(source_path),
                    "output_path": str(output_path),
                    "page_indices": [0],
                },
            )
            operation["input_sha256"] = "0" * 64
            response = parse_response(self.worker.send(operation))
            self.assertEqual(response["disposition"], "failed")
            self.assertEqual(response["failure"]["code"], "INPUT_HASH_MISMATCH")
            self.assertEqual(output_path.read_bytes(), sentinel)
            self.assertEqual(list(output_path.parent.glob("*.worker-stage.pdf")), [])

    @unittest.skipUnless(
        pro_package_available(),
        "PyMuPDF Pro package is not installed",
    )
    def test_pro_batch_maps_placed_count_and_publishes_exactly(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "source.pdf"
            output_path = Path(directory) / "edited.pdf"
            document = pymupdf.open()
            try:
                for text in ("PAGE ONE", "PAGE TWO"):
                    page = document.new_page()
                    page.insert_text((72, 72), text)
                document.save(source_path)
            finally:
                document.close()
            with pymupdf.open(source_path) as source:
                edits = [
                    {
                        "page": page_number,
                        "rect": list(source[page_number].search_for(text)[0]),
                        "old_text": text,
                        "new_text": replacement,
                    }
                    for page_number, (text, replacement) in enumerate(
                        (("PAGE ONE", "FIRST EDIT"), ("PAGE TWO", "SECOND EDIT"))
                    )
                ]
            operation = request(
                "apply_many_edits",
                {
                    "pdf_path": str(source_path),
                    "output_path": str(output_path),
                    "edits": edits,
                    "font_path": None,
                },
            )
            operation["input_sha256"] = hashlib.sha256(source_path.read_bytes()).hexdigest()
            response = parse_response(self.worker.send(operation))
            self.assertEqual(response["disposition"], "succeeded", response)
            self.assertEqual(response["requested_count"], 2)
            self.assertEqual(response["applied_count"], 2)
            self.assertEqual(response["payload"]["result"]["placed"], 2)
            self.assertTrue(response["payload"]["artifact"]["committed"])
            self.assertTrue(output_path.is_file())

    def test_successful_mutation_publishes_exact_artifact_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "source.pdf"
            output_path = Path(directory) / "cloned.pdf"
            document = pymupdf.open()
            try:
                page = document.new_page()
                page.insert_text((72, 72), "PAGE ONE")
                document.save(source_path)
            finally:
                document.close()
            operation = request(
                "clone_pages",
                {
                    "pdf_path": str(source_path),
                    "output_path": str(output_path),
                    "page_indices": [0],
                },
            )
            operation["input_sha256"] = hashlib.sha256(source_path.read_bytes()).hexdigest()
            response = parse_response(self.worker.send(operation))
            self.assertEqual(response["disposition"], "succeeded", response)
            self.assertEqual(response["requested_count"], 1)
            self.assertEqual(response["applied_count"], 1)
            artifact = response["payload"]["artifact"]
            self.assertTrue(artifact["committed"])
            self.assertEqual(artifact["path"], str(output_path))
            self.assertEqual(artifact["sha256"], hashlib.sha256(output_path.read_bytes()).hexdigest())
            self.assertEqual(artifact["sha256"], response["output_sha256"])
            self.assertEqual(artifact["size_bytes"], output_path.stat().st_size)
            self.assertEqual(list(output_path.parent.glob("*.worker-stage.pdf")), [])

    def test_exception_taxonomy_is_stable_and_retry_aware(self) -> None:
        cases = [
            (FileNotFoundError("missing"), "INPUT_NOT_FOUND", False),
            (PermissionError("denied"), "PERMISSION_DENIED", False),
            (TimeoutError("late"), "PYTHON_TIMEOUT", True),
            (ConnectionError("offline"), "PYTHON_CONNECTION_ERROR", True),
            (MemoryError("exhausted"), "PYTHON_MEMORY_EXHAUSTED", False),
            (ValueError("bad"), "PYTHON_INVALID_VALUE", False),
            (RuntimeError("PDF_NOT_EDITABLE: fixture"), "PDF_NOT_EDITABLE", False),
            (ProtocolError("BAD_PROTOCOL", "bad"), "BAD_PROTOCOL", False),
        ]
        for error, expected_code, retryable in cases:
            with self.subTest(expected_code=expected_code):
                failure = classify_error(error, "ping")
                self.assertEqual(failure["code"], expected_code)
                self.assertEqual(failure["retryable"], retryable)
                self.assertEqual(failure["context"], {"operation": "ping"})

    def test_eof_shuts_worker_down_cleanly(self) -> None:
        code, _stderr = self.worker.close()
        self.assertEqual(code, 0)


if __name__ == "__main__":
    unittest.main()
