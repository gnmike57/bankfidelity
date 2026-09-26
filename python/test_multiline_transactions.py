"""Regression tests for multi-line transaction description logic.

Covers:
  - description words unioned across wrap lines at finalize time
  - below-date continuation attached to the current transaction
  - preceding (Westpac-style) description attached to the next date row
  - symmetric Y-gap (above rows never merge)
  - multi-span identity match for edit targeting
"""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf

MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("multiline_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def _word(x0, y0, x1, y1, text, block=0, line=0, word=0):
    return (float(x0), float(y0), float(x1), float(y1), str(text), block, line, word)


class FinalizeMultilineDescriptionTests(unittest.TestCase):
    def test_finalize_includes_continuation_row_text(self):
        date_row = [
            _word(10, 40, 60, 50, "15/01/2024", word=0),
            _word(70, 40, 140, 50, "Payment", word=1),
            _word(145, 40, 200, 50, "Merchant", word=2),
            _word(300, 40, 340, 50, "50.00", word=3),
            _word(360, 40, 410, 50, "1050.00", word=4),
        ]
        cont_row = [
            _word(70, 55, 100, 65, "Ref", word=0),
            _word(105, 55, 180, 65, "1394711", word=1),
        ]
        block = {
            "date": "15/01/2024",
            "date_word_count": 1,
            "date_bbox": [10.0, 40.0, 60.0, 50.0],
            "rows": [date_row, cont_row],
            "last_y": 65.0,
        }
        finalized = BRIDGE._finalize_transaction_block(0, block, "native")
        self.assertIsNotNone(finalized)
        self.assertIn("Payment", finalized["raw_text"])
        self.assertIn("Merchant", finalized["raw_text"])
        self.assertIn("Ref", finalized["raw_text"])
        self.assertIn("1394711", finalized["raw_text"])
        desc = finalized["field_bboxes"]["description"]
        self.assertLessEqual(desc[1], 40.0 + 1e-6)
        self.assertGreaterEqual(desc[3], 65.0 - 1e-6)

    def test_finalize_includes_preceding_description_words(self):
        pending = [
            _word(70, 20, 200, 30, "Withdrawal-Osko", word=0),
            _word(205, 20, 280, 30, "Payment", word=1),
        ]
        date_row = [
            _word(10, 40, 60, 50, "25/09/23", word=0),
            _word(300, 40, 340, 50, "25.00", word=1),
            _word(360, 40, 410, 50, "576.87", word=2),
        ]
        block = {
            "date": "25/09/23",
            "date_word_count": 1,
            "date_bbox": [10.0, 40.0, 60.0, 50.0],
            "rows": [date_row, pending],
            "description_words": pending,
            "last_y": 50.0,
        }
        finalized = BRIDGE._finalize_transaction_block(0, block, "native")
        self.assertIsNotNone(finalized)
        self.assertIn("Withdrawal-Osko", finalized["raw_text"])
        self.assertIn("Payment", finalized["raw_text"])


class ContinuationGapHelperTests(unittest.TestCase):
    def test_symmetric_gap_rejects_above_rows(self):
        self.assertFalse(BRIDGE._within_continuation_gap(50.0, 100.0, 34.0))
        self.assertTrue(BRIDGE._within_continuation_gap(110.0, 100.0, 34.0))
        self.assertFalse(BRIDGE._within_continuation_gap(150.0, 100.0, 34.0))


class SyntheticPdfMultilineTests(unittest.TestCase):
    def _write_statement(self, path: Path, lines: list[tuple[float, str]]):
        doc = pymupdf.open()
        page = doc.new_page(width=612, height=792)
        for y, text in lines:
            page.insert_text((72, y), text, fontsize=10, fontname="helv")
        doc.save(path)
        doc.close()

    def test_below_date_continuation_stays_on_current_tx(self):
        with tempfile.TemporaryDirectory() as directory:
            pdf = Path(directory) / "below.pdf"
            self._write_statement(
                pdf,
                [
                    (72, "Opening Balance $1000.00"),
                    (100, "15/01/2024 Payment to Merchant XYZ $50.00 $1050.00"),
                    (114, "Ref 1394711 Osko"),
                    (140, "16/01/2024 Coffee Shop $5.00 $1045.00"),
                ],
            )
            transactions = BRIDGE.get_all_transactions(str(pdf))
            self.assertGreaterEqual(len(transactions), 2, transactions)
            first = transactions[0]
            self.assertIn("Payment", first["raw_text"])
            self.assertIn("Merchant", first["raw_text"])
            self.assertIn("1394711", first["raw_text"], first["raw_text"])
            second = transactions[1]
            self.assertNotIn("1394711", second["raw_text"], second["raw_text"])

    def test_preceding_description_attaches_to_next_date(self):
        with tempfile.TemporaryDirectory() as directory:
            pdf = Path(directory) / "above.pdf"
            self._write_statement(
                pdf,
                [
                    (72, "Opening Balance $500.00"),
                    (100, "14/01/2024 Prior Purchase $10.00 $510.00"),
                    (120, "Withdrawal-Osko Payment 1394711 Paylive.me"),
                    (134, "25/09/23 $25.00 $535.00"),
                ],
            )
            transactions = BRIDGE.get_all_transactions(str(pdf))
            self.assertGreaterEqual(len(transactions), 2, transactions)
            # Last dated amount row should own the preceding Osko description.
            target = next(
                (
                    item
                    for item in transactions
                    if "25/09" in item["date"] or "25/09" in item["raw_text"]
                ),
                transactions[-1],
            )
            self.assertIn("Osko", target["raw_text"], target["raw_text"])
            self.assertAlmostEqual(float(target["running_balance"]), 535.0, places=2)

    def test_multiline_description_span_match(self):
        with tempfile.TemporaryDirectory() as directory:
            pdf = Path(directory) / "spans.pdf"
            doc = pymupdf.open()
            page = doc.new_page(width=612, height=792)
            # Two separate insert_text calls → two spans on different y.
            page.insert_text((72, 100), "Payment to Merchant", fontsize=11, fontname="helv")
            page.insert_text((72, 114), "Ref 1394711", fontsize=11, fontname="helv")
            doc.save(pdf)
            doc.close()
            with pymupdf.open(pdf) as document:
                page = document[0]
                # Tall rect covering both lines.
                rect = pymupdf.Rect(60, 88, 250, 130)
                matches = BRIDGE._find_exact_target_spans(
                    page, rect, "Payment to Merchant Ref 1394711"
                )
                self.assertEqual(len(matches), 1, matches)
                self.assertIn("Payment", matches[0]["text"])
                self.assertIn("1394711", matches[0]["text"])
                bbox = matches[0]["bbox"]
                self.assertGreater(float(bbox[3]) - float(bbox[1]), 10.0)


class RealFixtureWestpacRegression(unittest.TestCase):
    def test_westpac_preceding_description_still_attaches(self):
        root = Path(__file__).resolve().parents[1]
        fixture = root / "AU Bank Statements" / "westpac_choicebasic_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        transactions = BRIDGE.get_all_transactions(str(fixture))
        self.assertEqual(len(transactions), 351)
        target = next(
            item
            for item in transactions
            if item["page"] == 2 and item["line_on_page"] == 19
        )
        self.assertEqual(target["date"], "25/09/23")
        self.assertIn("Withdrawal-Osko Payment 1394711 Paylive.me", target["raw_text"])
        self.assertAlmostEqual(target["credit"], 25.0, places=2)
        self.assertAlmostEqual(target["running_balance"], 576.87, places=2)


if __name__ == "__main__":
    unittest.main()
