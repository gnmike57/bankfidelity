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
SPEC = importlib.util.spec_from_file_location("type3_bridge", MODULE_PATH)
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


def span_for(page, text: str, target: pymupdf.Rect):
    matches = []
    for block in page.get_text("dict").get("blocks", []):
        for line in block.get("lines", []):
            for span in line.get("spans", []):
                if normalized(span.get("text", "")) != normalized(text):
                    continue
                rect = pymupdf.Rect(span.get("bbox", (0, 0, 0, 0)))
                if not (rect & target).is_empty:
                    matches.append(span)
    if len(matches) != 1:
        raise AssertionError(f"expected one {text!r} span in {list(target)}, found {len(matches)}")
    return matches[0]


class Type3SourceResourceTests(unittest.TestCase):
    old_text = "$0.00"
    new_text = "$100.00"
    target = pymupdf.Rect(509.3, 133.9, 529.7, 144.5)

    def setUp(self):
        fixture = ROOT / "AU Bank Statements" / "fallback.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        self.tempdir = tempfile.TemporaryDirectory()
        self.directory = Path(self.tempdir.name)
        self.segment = self.directory / "fallback_pages_1_3.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            try:
                segment.insert_pdf(source, from_page=0, to_page=min(2, source.page_count - 1))
                segment.save(self.segment, garbage=4, deflate=True)
            finally:
                segment.close()
        with pymupdf.open(self.segment) as document:
            self.source_page_count = document.page_count
            self.source_page_rects = [list(page.rect) for page in document]
            self.source_fonts = [tuple(font) for font in document[0].get_fonts(full=True)]
            self.source_span = span_for(document[0], self.old_text, self.target)
            self.source_right = float(self.source_span["bbox"][2])
        self.source_hash = sha256(self.segment)

    def tearDown(self):
        self.tempdir.cleanup()

    def edit(self, output: Path, new_text: str | None = None):
        return BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": 0,
                    "rect": list(self.target),
                    "old_text": self.old_text,
                    "new_text": self.new_text if new_text is None else new_text,
                }
            ],
        )

    def test_real_type3_edit_is_extractable_right_aligned_and_structurally_preserved(self):
        output = self.directory / "edited.pdf"
        report = self.edit(output)

        self.assertTrue(report["success"], report)
        self.assertEqual(report["requested"], 1)
        self.assertEqual(report["matched"], 1)
        self.assertEqual(report["placed"], 1)
        self.assertEqual(report["failed"], 0)
        self.assertEqual(report["method_per_edit"], ["type3-source-resource"])
        self.assertEqual(report["review_flags"], [])
        self.assertTrue(report["output_published"])
        self.assertEqual(report["source_sha256"], self.source_hash)
        self.assertEqual(report["output_sha256"], sha256(output))
        self.assertEqual(sha256(self.segment), self.source_hash)

        with pymupdf.open(output) as document:
            self.assertEqual(document.page_count, self.source_page_count)
            self.assertEqual([list(page.rect) for page in document], self.source_page_rects)
            edited_span = span_for(document[0], self.new_text, pymupdf.Rect(500, 130, 535, 148))
            self.assertAlmostEqual(float(edited_span["bbox"][2]), self.source_right, delta=1.0)
            clip_text = document[0].get_text("text", clip=pymupdf.Rect(500, 128, 536, 149))
            self.assertIn(normalized(self.new_text), normalized(clip_text))
            self.assertNotIn(normalized(self.old_text), normalized(clip_text))
            output_fonts = [tuple(font) for font in document[0].get_fonts(full=True)]
            source_type3 = {(font[3], font[4]) for font in self.source_fonts if font[2] == "Type3"}
            output_type3 = {(font[3], font[4]) for font in output_fonts if font[2] == "Type3"}
            self.assertTrue(source_type3.issubset(output_type3))

        with pymupdf.open(self.segment) as source, pymupdf.open(output) as edited:
            scale = 240 / 72
            before = source[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            after = edited[0].get_pixmap(matrix=pymupdf.Matrix(scale, scale), alpha=False)
            before_array = np.frombuffer(before.samples, dtype=np.uint8).reshape(before.height, before.width, before.n)[:, :, :3]
            after_array = np.frombuffer(after.samples, dtype=np.uint8).reshape(after.height, after.width, after.n)[:, :, :3]
            changed = np.any(np.abs(before_array.astype(np.int16) - after_array.astype(np.int16)) > 4, axis=2)
            expanded = pymupdf.Rect(self.target)
            expanded.x0 -= 18
            expanded.y0 -= 12
            expanded.x1 += 12
            expanded.y1 += 12
            target = [int(round(value * scale)) for value in expanded]
            mask = np.zeros_like(changed)
            mask[target[1]:target[3], target[0]:target[2]] = True
            self.assertEqual(int(np.logical_and(changed, ~mask).sum()), 0)

    def test_real_type3_edit_is_repeatable(self):
        outputs = [self.directory / "edited_a.pdf", self.directory / "edited_b.pdf"]
        reports = [self.edit(output) for output in outputs]
        for report in reports:
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["type3-source-resource"])
        with pymupdf.open(outputs[0]) as first, pymupdf.open(outputs[1]) as second:
            self.assertEqual(first.page_count, second.page_count)
            self.assertEqual(first[0].get_text("text"), second[0].get_text("text"))
            first_pix = first[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            second_pix = second[0].get_pixmap(matrix=pymupdf.Matrix(2, 2), alpha=False)
            self.assertEqual(first_pix.samples, second_pix.samples)

    def test_unmapped_type3_character_fails_without_output(self):
        output = self.directory / "must_not_exist.pdf"
        with self.assertRaises(ValueError) as raised:
            self.edit(output, "$100.00Ω")
        payload = json.loads(str(raised.exception))
        self.assertEqual(payload["error"], "FONT_COVERAGE_INSUFFICIENT")
        self.assertIn("Ω", payload["missing_chars"])
        self.assertFalse(output.exists())
        self.assertEqual(sha256(self.segment), self.source_hash)

    def test_nab_negative_balance_uses_near_metric_type3_donor(self):
        transactions = BRIDGE.get_all_transactions(str(self.segment))
        first = transactions[0]
        output = self.directory / "negative_balance.pdf"
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": first["page"],
                    "rect": first["field_bboxes"]["running_balance"],
                    "old_text": str(first["running_balance"]),
                    "new_text": "-200",
                }
            ],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["type3-source-resource"])
        with pymupdf.open(output) as document:
            self.assertIn("-200", document[first["page"]].get_text())
        self.assertEqual(sha256(self.segment), self.source_hash)

    def test_nab_description_without_type3_donor_uses_reported_standard14(self):
        transactions = BRIDGE.get_all_transactions(str(self.segment))
        first = transactions[0]
        output = self.directory / "standard14_description.pdf"
        replacement = "eBay O*10-12434-35623 Sydney AU AUS"
        report = BRIDGE.apply_many_edits(
            str(self.segment),
            str(output),
            [
                {
                    "page": first["page"],
                    "rect": first["field_bboxes"]["description"],
                    "old_text": "AA1M7692502345501T Jobseeker Pymt",
                    "new_text": replacement,
                }
            ],
        )
        self.assertTrue(report["success"], report)
        self.assertEqual(report["method_per_edit"], ["verified-standard14"])
        with pymupdf.open(output) as document:
            self.assertIn(replacement, document[first["page"]].get_text())
        self.assertEqual(sha256(self.segment), self.source_hash)

    def test_nab_dotted_leader_description_matches_semantic_prefix_exactly(self):
        fixture = ROOT / "AU Bank Statements" / "fallback.pdf"
        source_transaction = next(
            item
            for item in BRIDGE.get_all_transactions(str(fixture))
            if "Anthony McIver" in str(item.get("raw_text") or "")
        )
        anthony_segment = self.directory / "anthony_segment.pdf"
        with pymupdf.open(fixture) as source:
            segment = pymupdf.open()
            segment.insert_pdf(
                source,
                from_page=source_transaction["page"],
                to_page=source_transaction["page"],
            )
            segment.save(anthony_segment, garbage=3, deflate=True)
            segment.close()
        transaction = next(
            item
            for item in BRIDGE.get_all_transactions(str(anthony_segment))
            if "Anthony McIver" in str(item.get("raw_text") or "")
        )
        target = pymupdf.Rect(transaction["field_bboxes"]["description"])
        semantic_old = "Anthony McIver Some assistance"
        segment_hash = sha256(anthony_segment)
        with pymupdf.open(anthony_segment) as document:
            matches = BRIDGE._find_exact_target_spans(
                document[transaction["page"]], target, semantic_old
            )
            self.assertEqual(len(matches), 1, matches)
            self.assertTrue(
                BRIDGE._normalized_text_identity(matches[0]["text"]).startswith(
                    BRIDGE._normalized_text_identity(semantic_old)
                )
            )
        output = self.directory / "dotted_leader_description.pdf"
        replacement = "Settlement Assistance"
        report = BRIDGE.apply_many_edits(
            str(anthony_segment),
            str(output),
            [
                {
                    "page": transaction["page"],
                    "rect": list(target),
                    "old_text": semantic_old,
                    "new_text": replacement,
                }
            ],
        )
        self.assertTrue(report["success"], report)
        with pymupdf.open(output) as document:
            self.assertIn(replacement, document[transaction["page"]].get_text())
        self.assertEqual(sha256(anthony_segment), segment_hash)

    def test_background_classifier_uses_white_edges_not_black_text_center(self):
        path = self.directory / "background.pdf"
        document = pymupdf.open()
        page = document.new_page(width=300, height=200)
        page.insert_text(pymupdf.Point(100, 100), "$100.00", fontname="helv", fontsize=18)
        document.save(path)
        document.close()
        with pymupdf.open(path) as rendered:
            page = rendered[0]
            search = page.search_for("$100.00")
            self.assertEqual(len(search), 1)
            classification, color = BRIDGE.classify_background(page, search[0])
        self.assertEqual(classification, "solid")
        self.assertTrue(all(channel >= 0.95 for channel in color), color)


if __name__ == "__main__":
    unittest.main()
