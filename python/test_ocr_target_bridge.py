import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("ocr_target_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class OcrTargetBridgeTests(unittest.TestCase):
    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "anz_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "anz_page_1.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            try:
                segment.insert_pdf(source, from_page=0, to_page=0)
                segment.save(self.segment, garbage=4, deflate=True)
            finally:
                segment.close()
        self.source_hash = sha256(self.segment)

    def tearDown(self):
        self.tempdir.cleanup()

    def test_ocr_identity_selects_native_font_span_atomically(self):
        transactions = BRIDGE.get_all_transactions(str(self.segment))
        self.assertGreaterEqual(len(transactions), 1)
        first = transactions[0]
        target = first["field_bboxes"]["date"]
        output = self.directory / "edited.pdf"
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": 0,
                    "rect": target,
                    "old_text": first["date"],
                    "new_text": "01 Sep",
                }
            ],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual((report["requested"], report["matched"], report["placed"]), (1, 1, 1))
        self.assertEqual(report["method_per_edit"], ["verified-standard14"])
        self.assertEqual(report["review_flags"], [])
        self.assertTrue(output.exists())
        self.assertEqual(sha256(self.segment), self.source_hash)
        with pymupdf.open(output) as edited:
            clip = pymupdf.Rect(target)
            clip.x0 -= 3
            clip.y0 -= 3
            clip.x1 += 12
            clip.y1 += 3
            observed = " ".join(edited[0].get_text("text", clip=clip).split())
            self.assertIn("01 Sep", observed)


if __name__ == "__main__":
    unittest.main()
