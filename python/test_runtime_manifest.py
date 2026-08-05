"""Tests for the deterministic packaged Python runtime manifest."""

from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import verify_runtime_manifest as runtime_manifest


class RuntimeManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.runtime_dir = Path(self.directory.name) / "python"
        self.runtime_dir.mkdir()
        source_dir = Path(runtime_manifest.__file__).resolve().parent
        manifest = json.loads(
            (source_dir / "runtime-manifest.json").read_text(encoding="utf-8")
        )
        for relative_name in manifest["source_files"]:
            shutil.copy2(source_dir / relative_name, self.runtime_dir / relative_name)
        shutil.copy2(
            source_dir / "runtime-manifest.json",
            self.runtime_dir / "runtime-manifest.json",
        )
        self.python_dir_patch = mock.patch.object(
            runtime_manifest, "PYTHON_DIR", self.runtime_dir
        )
        self.manifest_path_patch = mock.patch.object(
            runtime_manifest,
            "MANIFEST_PATH",
            self.runtime_dir / "runtime-manifest.json",
        )
        self.python_dir_patch.start()
        self.manifest_path_patch.start()

    def tearDown(self) -> None:
        self.manifest_path_patch.stop()
        self.python_dir_patch.stop()
        self.directory.cleanup()

    def test_exact_base_runtime_is_accepted(self) -> None:
        report = runtime_manifest.verify("base")
        self.assertEqual(report["protocol_version"], "1.0.0")
        self.assertEqual(report["packages"]["PyMuPDF"], "1.28.0")
        self.assertIn("worker.py", report["source_files"])

    def test_source_tampering_is_rejected(self) -> None:
        with (self.runtime_dir / "worker.py").open("a", encoding="utf-8") as stream:
            stream.write("\n# tampered\n")
        with self.assertRaisesRegex(
            runtime_manifest.RuntimeManifestError,
            "runtime source hash mismatch: worker.py",
        ):
            runtime_manifest.verify("base")

    def test_package_version_drift_is_rejected(self) -> None:
        with mock.patch.object(
            runtime_manifest.importlib.metadata, "version", return_value="0.0.0"
        ):
            with self.assertRaisesRegex(
                runtime_manifest.RuntimeManifestError,
                "PyMuPDF 0.0.0 does not match 1.28.0",
            ):
                runtime_manifest.verify("base")


if __name__ == "__main__":
    unittest.main()
