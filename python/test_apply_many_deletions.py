import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("deletion_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


class ApplyManyDeletionTests(unittest.TestCase):
    def test_exact_text_deletion_is_atomic_and_extractable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.pdf"
            output = root / "output.pdf"
            document = pymupdf.open()
            page = document.new_page()
            page.insert_text((72, 96), "DELETE ME", fontname="helv", fontsize=10)
            document.save(source)
            document.close()
            with pymupdf.open(source) as opened:
                target = list(opened[0].search_for("DELETE ME")[0])
            source_hash = sha256(source)

            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": target,
                        "old_text": "DELETE ME",
                        "new_text": "",
                    }
                ],
            )

            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["exact-redaction-delete"])
            self.assertEqual((report["requested"], report["matched"], report["placed"]), (1, 1, 1))
            self.assertEqual(sha256(source), source_hash)
            self.assertTrue(output.exists())
            with pymupdf.open(output) as edited:
                self.assertNotIn("DELETE ME", edited[0].get_text())

    def test_exact_money_identity_matches_signed_presentation_by_magnitude(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.pdf"
            output = root / "output.pdf"
            document = pymupdf.open()
            page = document.new_page()
            page.insert_text((72, 96), "-1,000.00", fontname="helv", fontsize=10)
            document.save(source)
            document.close()
            with pymupdf.open(source) as opened:
                target = list(opened[0].search_for("-1,000.00")[0])
            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": target,
                        "old_text": "1000",
                        "new_text": "93",
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            with pymupdf.open(output) as edited:
                self.assertIn("93", edited[0].get_text())


if __name__ == "__main__":
    unittest.main()
