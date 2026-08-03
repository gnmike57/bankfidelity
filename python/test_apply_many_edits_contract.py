#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import pymupdf

MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("pymupdf_pro_integration", MODULE_PATH)
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


def create_text_pdf(path: Path, text: str = "KEEP") -> list[float]:
    document = pymupdf.open()
    page = document.new_page(width=612, height=792)
    page.insert_text(pymupdf.Point(72, 100), text, fontname="helv", fontsize=12)
    document.save(path)
    document.close()

    document = pymupdf.open(path)
    spans = [
        span
        for block in document[0].get_text("dict").get("blocks", [])
        for line in block.get("lines", [])
        for span in line.get("spans", [])
        if text in span.get("text", "")
    ]
    document.close()
    if len(spans) != 1:
        raise AssertionError(f"expected one {text!r} span, found {len(spans)}")
    return [float(value) for value in spans[0]["bbox"]]


def create_many_text_pdf(path: Path, count: int) -> list[tuple[str, list[float]]]:
    document = pymupdf.open()
    page = document.new_page(width=612, height=792)
    expected = []
    for index in range(count):
        text = f"OLD_{index:02d}"
        expected.append(text)
        page.insert_text(
            pymupdf.Point(72, 55 + index * 28),
            text,
            fontname="helv",
            fontsize=11,
        )
    document.save(path)
    document.close()

    document = pymupdf.open(path)
    by_text = {
        span.get("text", ""): [float(value) for value in span["bbox"]]
        for block in document[0].get_text("dict").get("blocks", [])
        for line in block.get("lines", [])
        for span in line.get("spans", [])
    }
    document.close()
    missing = [text for text in expected if text not in by_text]
    if missing:
        raise AssertionError(f"missing generated spans: {missing}")
    return [(text, by_text[text]) for text in expected]


class ApplyManyEditsContractTests(unittest.TestCase):
    def test_no_overlap_is_non_destructive_and_not_success(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-no-overlap-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing-output.pdf"
            create_text_pdf(source)
            shutil.copy2(source, output)
            source_before = sha256(source)
            output_before = sha256(output)

            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": [300.0, 300.0, 360.0, 330.0],
                        "old_text": "KEEP",
                        "new_text": "REPLACEMENT",
                    }
                ],
            )

            self.assertFalse(report["success"])
            self.assertEqual(report["schema_version"], 1)
            self.assertEqual(
                (report["requested"], report["matched"], report["placed"], report["failed"]),
                (1, 0, 0, 1),
            )
            self.assertFalse(report["output_published"])
            self.assertIsNone(report["output_sha256"])
            self.assertEqual(report["source_sha256"], source_before)
            self.assertEqual(report["method_per_edit"], ["identity-no-match"])
            self.assertEqual(len(report["edits"]), 1)
            self.assertFalse(report["edits"][0]["matched"])
            self.assertFalse(report["edits"][0]["placed"])
            self.assertEqual(sha256(source), source_before)
            self.assertEqual(sha256(output), output_before)

            document = pymupdf.open(output)
            observed = "".join(page.get_text() for page in document)
            document.close()
            self.assertIn("KEEP", observed)
            self.assertNotIn("REPLACEMENT", observed)

    def test_exact_success_has_complete_counts_hashes_and_evidence(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-success-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "output.pdf"
            bbox = create_text_pdf(source, "OLD")

            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [{"page": 0, "rect": bbox, "old_text": "OLD", "new_text": "NEW"}],
            )

            self.assertTrue(report["success"], json.dumps(report, indent=2))
            self.assertEqual(
                (report["requested"], report["matched"], report["placed"], report["failed"]),
                (1, 1, 1, 0),
            )
            self.assertTrue(report["output_published"])
            self.assertEqual(report["source_sha256"], sha256(source))
            self.assertEqual(report["output_sha256"], sha256(output))
            self.assertEqual(len(report["method_per_edit"]), 1)
            self.assertEqual(len(report["edits"]), 1)
            self.assertTrue(report["edits"][0]["matched"])
            self.assertTrue(report["edits"][0]["placed"])

            document = pymupdf.open(output)
            observed = "".join(page.get_text() for page in document)
            document.close()
            self.assertIn("NEW", observed)
            self.assertNotIn("OLD", observed)

    def test_missing_stable_identity_is_rejected_before_publication(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-missing-identity-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing-output.pdf"
            bbox = create_text_pdf(source, "OLD")
            shutil.copy2(source, output)
            output_before = sha256(output)

            with self.assertRaises(ValueError) as captured:
                BRIDGE.apply_many_edits(
                    str(source),
                    str(output),
                    [{"page": 0, "rect": bbox, "new_text": "NEW"}],
                )
            payload = json.loads(str(captured.exception))
            self.assertEqual(payload["error"], "INVALID_EDIT_SCHEMA")
            self.assertEqual(payload["missing"], ["old_text"])
            self.assertEqual(sha256(output), output_before)

    def test_old_text_mismatch_is_non_destructive(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-identity-mismatch-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing-output.pdf"
            bbox = create_text_pdf(source, "OLD")
            shutil.copy2(source, output)
            output_before = sha256(output)

            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [{"page": 0, "rect": bbox, "old_text": "WRONG", "new_text": "NEW"}],
            )
            self.assertFalse(report["success"])
            self.assertEqual(report["method_per_edit"], ["identity-no-match"])
            self.assertFalse(report["output_published"])
            self.assertEqual(sha256(output), output_before)

    def test_field_bounded_currency_identity_matches_exact_numeric_value(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-money-identity-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "output.pdf"
            bbox = create_text_pdf(source, "$1,234.50 CR")

            report = BRIDGE.apply_many_edits(
                str(source),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": bbox,
                        "old_text": "1234.50",
                        "new_text": "1334.50",
                    }
                ],
            )

            self.assertTrue(report["success"], json.dumps(report, indent=2))
            self.assertEqual(
                (report["requested"], report["matched"], report["placed"], report["failed"]),
                (1, 1, 1, 0),
            )
            document = pymupdf.open(output)
            observed = "".join(page.get_text() for page in document)
            document.close()
            self.assertIn("1334.50", observed)
            self.assertNotIn("$1,234.50 CR", observed)

    def test_ambiguous_identity_is_non_destructive(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-ambiguous-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing-output.pdf"
            bbox = create_text_pdf(source, "OLD")
            shutil.copy2(source, output)
            output_before = sha256(output)

            with mock.patch.object(
                BRIDGE,
                "_find_exact_target_spans",
                return_value=[{"text": "OLD"}, {"text": "OLD"}],
            ):
                report = BRIDGE.apply_many_edits(
                    str(source),
                    str(output),
                    [{"page": 0, "rect": bbox, "old_text": "OLD", "new_text": "NEW"}],
                )
            self.assertFalse(report["success"])
            self.assertEqual(report["method_per_edit"], ["ambiguous-target"])
            self.assertFalse(report["output_published"])
            self.assertEqual(sha256(output), output_before)

    def test_twenty_edit_transaction_is_exact_and_repeatable(self):
        with tempfile.TemporaryDirectory(prefix="apply-report-twenty-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output_one = root / "output-one.pdf"
            output_two = root / "output-two.pdf"
            spans = create_many_text_pdf(source, 20)
            edits = [
                {
                    "page": 0,
                    "rect": bbox,
                    "old_text": old_text,
                    "new_text": f"NEW_{index:02d}",
                }
                for index, (old_text, bbox) in enumerate(spans)
            ]

            first = BRIDGE.apply_many_edits(str(source), str(output_one), edits)
            second = BRIDGE.apply_many_edits(str(source), str(output_two), edits)

            for report, output in ((first, output_one), (second, output_two)):
                self.assertTrue(report["success"], json.dumps(report, indent=2))
                self.assertEqual(
                    (
                        report["requested"],
                        report["matched"],
                        report["placed"],
                        report["failed"],
                    ),
                    (20, 20, 20, 0),
                )
                self.assertTrue(report["output_published"])
                self.assertEqual(report["output_sha256"], sha256(output))
                self.assertEqual(len(report["edits"]), 20)
                self.assertEqual(len(report["method_per_edit"]), 20)
                self.assertTrue(all(edit["matched"] for edit in report["edits"]))
                self.assertTrue(all(edit["placed"] for edit in report["edits"]))

            observed_outputs = []
            for output in (output_one, output_two):
                document = pymupdf.open(output)
                observed_outputs.append("\n".join(page.get_text() for page in document))
                document.close()
            self.assertEqual(observed_outputs[0], observed_outputs[1])
            for index in range(20):
                self.assertIn(f"NEW_{index:02d}", observed_outputs[0])
                self.assertNotIn(f"OLD_{index:02d}", observed_outputs[0])

    def test_unavailable_embedded_font_never_substitutes_or_publishes(self):
        with tempfile.TemporaryDirectory(prefix="font-embedding-block-") as temp:
            root = Path(temp)
            source = root / "source.pdf"
            output = root / "existing.pdf"
            bbox = create_text_pdf(source, "ORIGINAL")
            shutil.copy2(source, output)
            prior_hash = sha256(output)
            edits = [{
                "page": 0,
                "rect": bbox,
                "old_text": "ORIGINAL",
                "new_text": "REPLACED",
            }]

            with (
                mock.patch.object(BRIDGE, "_is_standard_14", return_value=False),
                mock.patch.object(BRIDGE, "_embedded_font_xref_for_span", return_value=7),
                mock.patch.object(BRIDGE, "_font_covers_text", return_value=(True, [])),
                mock.patch.object(BRIDGE, "_resolve_embedded_font", return_value=None),
            ):
                with self.assertRaises(ValueError) as captured:
                    BRIDGE.apply_many_edits(str(source), str(output), edits)

            payload = json.loads(str(captured.exception))
            self.assertEqual(payload["error"], "FONT_EMBEDDING_UNAVAILABLE")
            self.assertEqual(payload["edit_index"], 0)
            self.assertEqual(sha256(output), prior_hash)


if __name__ == "__main__":
    unittest.main(verbosity=2)
