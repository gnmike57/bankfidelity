import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

import numpy as np
import pymupdf

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("profiled_type0_bridge", MODULE_PATH)
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


def semantic_fonts(page) -> set[tuple]:
    return {
        (font[1], font[2], font[3], font[4], font[5])
        for font in page.get_fonts(full=True)
    }


def intersecting_spans(page, target: pymupdf.Rect):
    output = []
    for block in page.get_text("dict").get("blocks", []):
        for line in block.get("lines", []):
            for span in line.get("spans", []):
                rect = pymupdf.Rect(span.get("bbox", (0, 0, 0, 0)))
                if not (rect & target).is_empty:
                    output.append(span)
    return output


def matching_traces(page, expected: list[int], target: pymupdf.Rect):
    expanded = pymupdf.Rect(target)
    expanded.x0 -= 4
    expanded.y0 -= 4
    expanded.x1 += 4
    expanded.y1 += 4
    output = []
    for trace in page.get_texttrace():
        glyph_ids = [int(item[1]) for item in trace.get("chars", [])]
        trace_rect = pymupdf.Rect(trace.get("bbox", (0, 0, 0, 0)))
        if glyph_ids == expected and not (trace_rect & expanded).is_empty:
            output.append(trace)
    return output


class ProfiledType0ResourceTests(unittest.TestCase):
    raw_old_text = ":\x0f39\x0f"
    visible_old_text = "$0.80"
    new_text = "$100.80"
    target = pymupdf.Rect(303.3, 332.9, 329.7, 344.4)
    expected_glyph_ids = [58, 19, 15, 15, 51, 57, 15]
    expected_profile_sha256 = "9ed4d2ca2424d96d2c04cae7ef6d2577d18623672e131f66c22a7fadb3a588d9"

    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "anz_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "anz_pages_1_3.pdf"
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
            spans = [
                span for span in intersecting_spans(document[0], self.target)
                if span.get("font") == "Aeonik2.0-Regular"
            ]
            self.assertEqual(len(spans), 1, spans)
            self.assertEqual(spans[0].get("text"), self.raw_old_text)
            self.source_right = float(spans[0]["bbox"][2])
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
                "old_text": self.raw_old_text,
                "new_text": self.new_text if new_text is None else new_text,
            }],
        )

    def test_real_anz_edit_uses_verified_profile_and_exact_cids(self):
        output = self.directory / "edited.pdf"
        report = self.edit(output)
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["profiled-type0-source-resource"])
        self.assertEqual((report["requested"], report["matched"], report["placed"], report["failed"]), (1, 1, 1, 0))
        self.assertEqual(report["review_flags"], [])
        self.assertTrue(report["output_published"])
        self.assertEqual(report["source_sha256"], self.source_hash)
        self.assertEqual(report["output_sha256"], sha256(output))
        self.assertEqual(report["edits"][0]["font_profile_sha256"], self.expected_profile_sha256)
        self.assertEqual(sha256(self.segment), self.source_hash)

        with pymupdf.open(output) as document:
            self.assertEqual(document.page_count, self.page_count)
            self.assertEqual([list(page.rect) for page in document], self.page_rects)
            traces = matching_traces(document[0], self.expected_glyph_ids, self.target)
            self.assertEqual(len(traces), 1, traces)
            self.assertAlmostEqual(float(traces[0]["bbox"][2]), self.source_right, delta=1.0)
            self.assertEqual(semantic_fonts(document[0]), self.source_fonts)

        with pymupdf.open(self.segment) as source, pymupdf.open(output) as edited:
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

    def test_real_anz_profiled_edit_is_repeatable(self):
        outputs = [self.directory / "a.pdf", self.directory / "b.pdf"]
        reports = [self.edit(output) for output in outputs]
        for report in reports:
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["profiled-type0-source-resource"])
        with pymupdf.open(outputs[0]) as first, pymupdf.open(outputs[1]) as second:
            first_pix = first[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            second_pix = second[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            self.assertEqual(first_pix.samples, second_pix.samples)

    def test_character_outside_verified_profile_fails_atomically(self):
        output = self.directory / "must_not_exist.pdf"
        with self.assertRaises(ValueError) as raised:
            self.edit(output, "$100.80A")
        payload = json.loads(str(raised.exception))
        self.assertEqual(payload["error"], "FONT_COVERAGE_INSUFFICIENT")
        self.assertIn("A", payload["missing_chars"])
        self.assertFalse(output.exists())
        self.assertEqual(sha256(self.segment), self.source_hash)


if __name__ == "__main__":
    unittest.main()
