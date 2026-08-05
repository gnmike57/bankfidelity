import hashlib
import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path

import pymupdf

MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("single_edit_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def create_text_pdf(path: Path, text: str = "ORIGINAL") -> list[float]:
    document = pymupdf.open()
    page = document.new_page(width=612, height=792)
    page.insert_text((72, 72), text, fontname="helv", fontsize=12)
    document.save(path)
    document.close()
    with pymupdf.open(path) as reopened:
        return list(reopened[0].search_for(text)[0])


class ReplaceTextInRectContractTests(unittest.TestCase):
    def test_exact_single_edit_uses_complete_batch_evidence(self):
        with tempfile.TemporaryDirectory(prefix="single-edit-exact-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "output.pdf"
            bbox = create_text_pdf(source)

            report = BRIDGE.replace_text_in_rect(
                str(source),
                str(output),
                0,
                bbox,
                "ORIGINAL",
                "REPLACED",
            )

            self.assertTrue(report["success"], json.dumps(report, indent=2))
            self.assertEqual(
                (report["requested"], report["matched"], report["placed"], report["failed"]),
                (1, 1, 1, 0),
            )
            self.assertTrue(report["output_published"])
            self.assertEqual(report["output_sha256"], sha256(output))
            with pymupdf.open(output) as document:
                observed = document[0].get_text()
            self.assertIn("REPLACED", observed)
            self.assertNotIn("ORIGINAL", observed)

    def test_identity_mismatch_preserves_existing_output(self):
        with tempfile.TemporaryDirectory(prefix="single-edit-mismatch-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing.pdf"
            bbox = create_text_pdf(source)
            shutil.copy2(source, output)
            output_before = sha256(output)

            report = BRIDGE.replace_text_in_rect(
                str(source),
                str(output),
                0,
                bbox,
                "WRONG",
                "REPLACED",
            )

            self.assertFalse(report["success"])
            self.assertEqual(report["method_per_edit"], ["identity-no-match"])
            self.assertFalse(report["output_published"])
            self.assertEqual(sha256(output), output_before)

    def test_empty_identity_is_rejected_without_output(self):
        with tempfile.TemporaryDirectory(prefix="single-edit-empty-identity-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "output.pdf"
            bbox = create_text_pdf(source)

            with self.assertRaises(ValueError) as captured:
                BRIDGE.replace_text_in_rect(
                    str(source),
                    str(output),
                    0,
                    bbox,
                    "",
                    "REPLACED",
                )
            payload = json.loads(str(captured.exception))
            self.assertEqual(payload["error"], "MISSING_STABLE_IDENTITY")
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
