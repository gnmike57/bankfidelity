import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import numpy as np
import pymupdf

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("bankwest_profile_bridge", MODULE_PATH)
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


def semantic_fonts(page) -> set[tuple]:
    return {
        (font[1], font[2], font[3], font[4], font[5])
        for font in page.get_fonts(full=True)
    }


def matching_traces(page, expected: list[int], target: pymupdf.Rect):
    expanded = pymupdf.Rect(target)
    expanded.x0 -= 4
    expanded.y0 -= 4
    expanded.x1 += 4
    expanded.y1 += 4
    matches = []
    for trace in page.get_texttrace():
        glyph_ids = [int(item[1]) for item in trace.get("chars", [])]
        rect = pymupdf.Rect(trace.get("bbox", (0, 0, 0, 0)))
        if glyph_ids == expected and not (rect & expanded).is_empty:
            matches.append(trace)
    return matches


class BankwestProfiledType0Tests(unittest.TestCase):
    old_text = "$301.44"
    new_text = "$401.44"
    target = pymupdf.Rect(516.4, 315.7, 546.5, 328.2)
    expected_glyph_ids = [46, 18, 13, 17, 47, 18, 18]
    expected_profile_sha256 = "4d96294fbff43c3b4ec43d62857423ccd9ad3086efd0fe5294927beae6a3621e"

    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "bankwest_pages_1_3.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            try:
                segment.insert_pdf(source, from_page=0, to_page=min(2, source.page_count - 1))
                segment.save(self.segment, garbage=4, deflate=True)
            finally:
                segment.close()
        self.source_hash = sha256(self.segment)
        with pymupdf.open(self.segment) as document:
            self.page_count = document.page_count
            self.page_rects = [list(page.rect) for page in document]
            self.source_fonts = semantic_fonts(document[0])
            traces = matching_traces(document[0], [46, 12, 13, 17, 47, 18, 18], self.target)
            self.assertEqual(len(traces), 1, traces)
            self.source_right = float(traces[0]["bbox"][2])

    def test_bankwest_tight_bold_amount_background_is_white(self):
        with pymupdf.open(self.segment) as document:
            kind, color = BRIDGE.classify_background(document[0], self.target)
        self.assertIn(kind, {"solid", "striped"})
        self.assertGreater(min(color), 0.9, (kind, color))

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

    def test_bankwest_uses_profiled_resource_and_is_extractable(self):
        output = self.directory / "edited.pdf"
        report = self.edit(output)
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["profiled-type0-inplace-stream"])
        self.assertEqual((report["requested"], report["matched"], report["placed"], report["failed"]), (1, 1, 1, 0))
        self.assertEqual(report["review_flags"], [])
        self.assertEqual(report["edits"][0]["font_profile_sha256"], self.expected_profile_sha256)
        self.assertEqual(sha256(self.segment), self.source_hash)
        with pymupdf.open(output) as document:
            self.assertIn(self.new_text, document[0].get_text())
            traces = matching_traces(document[0], self.expected_glyph_ids, self.target)
            self.assertEqual(len(traces), 1, traces)
            self.assertAlmostEqual(float(traces[0]["bbox"][2]), self.source_right, delta=1.0)
            self.assertEqual(semantic_fonts(document[0]), self.source_fonts)
            self.assertEqual(document.page_count, self.page_count)
            self.assertEqual([list(page.rect) for page in document], self.page_rects)

        with pymupdf.open(self.segment) as source, pymupdf.open(output) as edited:
            before = source[0].get_pixmap(matrix=pymupdf.Matrix(240 / 72, 240 / 72), alpha=False)
            after = edited[0].get_pixmap(matrix=pymupdf.Matrix(240 / 72, 240 / 72), alpha=False)
            before_array = np.frombuffer(before.samples, dtype=np.uint8).reshape(before.height, before.width, before.n)[:, :, :3]
            after_array = np.frombuffer(after.samples, dtype=np.uint8).reshape(after.height, after.width, after.n)[:, :, :3]
            changed = np.any(np.abs(before_array.astype(np.int16) - after_array.astype(np.int16)) > 4, axis=2)
            expanded = pymupdf.Rect(self.target)
            expanded.x0 -= 12
            expanded.y0 -= 12
            expanded.x1 += 12
            expanded.y1 += 12
            scale = 240 / 72
            target = [int(round(value * scale)) for value in expanded]
            mask = np.zeros_like(changed)
            mask[target[1]:target[3], target[0]:target[2]] = True
            self.assertEqual(int(np.logical_and(changed, ~mask).sum()), 0)

    def test_bankwest_profiled_edit_is_repeatable(self):
        outputs = [self.directory / "a.pdf", self.directory / "b.pdf"]
        reports = [self.edit(output) for output in outputs]
        self.assertTrue(all(report["success"] for report in reports), reports)
        with pymupdf.open(outputs[0]) as first, pymupdf.open(outputs[1]) as second:
            first_pix = first[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            second_pix = second[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            self.assertEqual(first_pix.samples, second_pix.samples)

    def test_unproven_bankwest_digit_uses_reported_standard14(self):
        output = self.directory / "standard14_digit.pdf"
        report = self.edit(output, "$601.44")
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["verified-standard14"])
        self.assertTrue(output.exists())
        with pymupdf.open(output) as document:
            self.assertIn("$601.44", document[0].get_text())
        self.assertEqual(sha256(self.segment), self.source_hash)


if __name__ == "__main__":
    unittest.main()
