import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import numpy as np
import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("one_byte_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def signature(page):
    return {
        "fonts": sorted([[font[1], font[2], font[3], font[4], font[5]] for font in page.get_fonts(full=True)], key=str),
        "images": sorted([[image[2], image[3], image[4], image[5], image[8]] for image in page.get_images(full=True)], key=str),
        "drawings": len(page.get_drawings()),
        "annotations": sum(1 for _ in (page.annots() or [])),
        "links": len(page.get_links()),
    }


class OneByteInplaceStreamTests(unittest.TestCase):
    old_text = "$35,308.14 CR"
    new_text = "$35,408.14 CR"
    target = pymupdf.Rect(481.5, 145.3, 546.8, 156.9)

    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "commbank_smartaccess_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "commbank_pages_1_3.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            try:
                segment.insert_pdf(source, from_page=0, to_page=min(2, source.page_count - 1))
                segment.save(self.segment, garbage=4, deflate=True)
            finally:
                segment.close()
        with pymupdf.open(self.segment) as document:
            self.source_signature = signature(document[0])
            self.page_count = document.page_count
        self.source_hash = sha256(self.segment)

    def tearDown(self):
        self.tempdir.cleanup()

    def test_commbank_same_length_edit_is_stream_local(self):
        output = self.directory / "edited.pdf"
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [{"page": 0, "rect": list(self.target), "old_text": self.old_text, "new_text": self.new_text}],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["one-byte-inplace-stream"])
        self.assertEqual(report["review_flags"], [])
        self.assertEqual(sha256(self.segment), self.source_hash)
        with pymupdf.open(self.segment) as source, pymupdf.open(output) as edited:
            self.assertEqual(edited.page_count, self.page_count)
            self.assertEqual(signature(edited[0]), self.source_signature)
            self.assertIn(self.new_text, edited[0].get_text())
            scale = 240 / 72
            before = source[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            after = edited[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            before_array = np.frombuffer(before.samples, dtype=np.uint8).reshape(before.height, before.width, before.n)[:, :, :3]
            after_array = np.frombuffer(after.samples, dtype=np.uint8).reshape(after.height, after.width, after.n)[:, :, :3]
            changed = np.any(np.abs(before_array.astype(np.int16) - after_array.astype(np.int16)) > 4, axis=2)
            expanded = pymupdf.Rect(self.target)
            expanded.x0 -= 20
            expanded.y0 -= 14
            expanded.x1 += 14
            expanded.y1 += 14
            target = [int(round(value * scale)) for value in expanded]
            mask = np.zeros_like(changed)
            mask[target[1]:target[3], target[0]:target[2]] = True
            self.assertEqual(int(np.logical_and(changed, ~mask).sum()), 0)

    def test_commbank_duplicate_date_is_selected_by_exact_geometry(self):
        output = self.directory / "edited_duplicate_date.pdf"
        date_target = pymupdf.Rect(
            54.1441650390625,
            474.5362548828125,
            83.08541107177734,
            485.506591796875,
        )
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": 0,
                    "rect": list(date_target),
                    "old_text": "19 Dec",
                    "new_text": "01 Sep",
                }
            ],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["one-byte-inplace-stream"])
        self.assertEqual(report["review_flags"], [])
        self.assertEqual(sha256(self.segment), self.source_hash)
        with pymupdf.open(self.segment) as source, pymupdf.open(output) as edited:
            self.assertEqual(edited[0].get_text().count("19 Dec"), 2)
            self.assertEqual(edited[0].get_text().count("01 Sep"), 1)
            scale = 240 / 72
            before = source[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            after = edited[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            before_array = np.frombuffer(before.samples, dtype=np.uint8).reshape(
                before.height, before.width, before.n
            )[:, :, :3]
            after_array = np.frombuffer(after.samples, dtype=np.uint8).reshape(
                after.height, after.width, after.n
            )[:, :, :3]
            changed = np.any(
                np.abs(before_array.astype(np.int16) - after_array.astype(np.int16)) > 4,
                axis=2,
            )
            expanded = pymupdf.Rect(date_target)
            expanded.x0 -= 4
            expanded.y0 -= 4
            expanded.x1 += 4
            expanded.y1 += 4
            target = [int(round(value * scale)) for value in expanded]
            mask = np.zeros_like(changed)
            mask[target[1]:target[3], target[0]:target[2]] = True
            self.assertEqual(int(np.logical_and(changed, ~mask).sum()), 0)

    def test_commbank_variable_description_reuses_macroman_source_resource(self):
        output = self.directory / "edited_description.pdf"
        description_target = pymupdf.Rect(
            88.4311294555664,
            475.3078918457031,
            148.70291137695312,
            486.2782287597656,
        )
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": 0,
                    "rect": list(description_target),
                    "old_text": "Settlement Fee",
                    "new_text": "CREDIT INTEREST",
                }
            ],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["simple-source-resource"])
        self.assertEqual(report["review_flags"], [])
        self.assertEqual(sha256(self.segment), self.source_hash)
        with pymupdf.open(output) as edited:
            clip = pymupdf.Rect(description_target)
            clip.x0 -= 3
            clip.y0 -= 3
            clip.x1 += 16
            clip.y1 += 6
            observed = "".join(edited[0].get_text("text", clip=clip).split())
            full_text = "".join(edited[0].get_text("text").split())
            self.assertIn("CREDITINTEREST", observed)
            self.assertIn("CREDITINTEREST", full_text)
            self.assertNotIn("SettlementFee", observed)
            self.assertFalse(
                any(
                    str(font[4]).startswith("embf_")
                    for font in edited[0].get_fonts(full=True)
                )
            )


if __name__ == "__main__":
    unittest.main()
