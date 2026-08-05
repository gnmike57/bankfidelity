import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("simple_resource_bridge", MODULE_PATH)
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


def normalized(value: str) -> str:
    return "".join(str(value).split())


def semantic_fonts(page) -> set[tuple]:
    return {
        (font[1], font[2], font[3], font[4], font[5])
        for font in page.get_fonts(full=True)
    }


def target_span(page, text: str, target: pymupdf.Rect):
    matches = []
    for block in page.get_text("dict").get("blocks", []):
        for line in block.get("lines", []):
            for span in line.get("spans", []):
                if normalized(span.get("text", "")) != normalized(text):
                    continue
                if not (pymupdf.Rect(span.get("bbox", (0, 0, 0, 0))) & target).is_empty:
                    matches.append(span)
    if len(matches) != 1:
        raise AssertionError(f"expected one {text!r} span, found {len(matches)}")
    return matches[0]


class SimpleSourceResourceTests(unittest.TestCase):
    old_text = "$0.00"
    new_text = "$100.00"
    target = pymupdf.Rect(38.4, 414.8, 63.7, 427.6)

    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "ing_orangeeveryday_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "ing_pages_1_3.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            try:
                segment.insert_pdf(source, from_page=0, to_page=min(2, source.page_count - 1))
                segment.save(self.segment, garbage=4, deflate=True)
            finally:
                segment.close()
        with pymupdf.open(self.segment) as document:
            self.page_count = document.page_count
            self.page_rects = [list(page.rect) for page in document]
            self.source_fonts = semantic_fonts(document[0])
            span = target_span(document[0], self.old_text, self.target)
            self.source_right = float(span["bbox"][2])
        self.source_hash = sha256(self.segment)

    def tearDown(self):
        self.tempdir.cleanup()

    def edit(self, output: Path, new_text: str | None = None):
        return BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [{
                "page": 0,
                "rect": list(self.target),
                "old_text": self.old_text,
                "new_text": self.new_text if new_text is None else new_text,
            }],
        )

    def test_ing_edit_reuses_simple_resource_and_is_extractable(self):
        output = self.directory / "edited.pdf"
        report = self.edit(output)
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["simple-source-resource"])
        self.assertEqual((report["requested"], report["matched"], report["placed"], report["failed"]), (1, 1, 1, 0))
        self.assertEqual(report["review_flags"], [])
        self.assertTrue(report["output_published"])
        self.assertEqual(report["source_sha256"], self.source_hash)
        self.assertEqual(report["output_sha256"], sha256(output))
        self.assertEqual(sha256(self.segment), self.source_hash)

        with pymupdf.open(output) as document:
            self.assertEqual(document.page_count, self.page_count)
            self.assertEqual([list(page.rect) for page in document], self.page_rects)
            span = target_span(document[0], self.new_text, pymupdf.Rect(24, 408, 70, 433))
            self.assertAlmostEqual(float(span["bbox"][2]), self.source_right, delta=1.0)
            clip_text = document[0].get_text("text", clip=pymupdf.Rect(24, 408, 70, 433))
            self.assertIn(normalized(self.new_text), normalized(clip_text))
            self.assertNotIn(normalized(self.old_text), normalized(clip_text))
            self.assertEqual(semantic_fonts(document[0]), self.source_fonts)
            self.assertFalse(any(str(font[4]).startswith("embf_") for font in document[0].get_fonts(full=True)))

    def test_ing_edit_is_repeatable(self):
        outputs = [self.directory / "a.pdf", self.directory / "b.pdf"]
        reports = [self.edit(output) for output in outputs]
        for report in reports:
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["simple-source-resource"])
        with pymupdf.open(outputs[0]) as first, pymupdf.open(outputs[1]) as second:
            self.assertEqual(first[0].get_text("text"), second[0].get_text("text"))
            first_pix = first[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            second_pix = second[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            self.assertEqual(first_pix.samples, second_pix.samples)

    def test_unmapped_winansi_character_fails_atomically(self):
        output = self.directory / "must_not_exist.pdf"
        with self.assertRaises(ValueError) as raised:
            self.edit(output, "$100.00Ω")
        payload = json.loads(str(raised.exception))
        self.assertEqual(payload["error"], "FONT_COVERAGE_INSUFFICIENT")
        self.assertIn("Ω", payload["missing_chars"])
        self.assertFalse(output.exists())
        self.assertEqual(sha256(self.segment), self.source_hash)


if __name__ == "__main__":
    unittest.main()
