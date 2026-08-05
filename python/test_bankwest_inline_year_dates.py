import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("bankwest_date_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


class BankwestInlineYearDateTests(unittest.TestCase):
    def test_two_sequential_dates_match_semantic_prefix_with_inline_year(self):
        fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        transactions = BRIDGE.get_all_transactions(str(fixture))
        first, second = transactions[0], transactions[1]
        self.assertEqual(first["raw_text"], "01 SEP CREDIT INTEREST $0.21 $301.65")
        with pymupdf.open(fixture) as document:
            for transaction in (first, second):
                matches = BRIDGE._find_exact_target_spans(
                    document[0],
                    pymupdf.Rect(transaction["field_bboxes"]["date"]),
                    transaction["date"],
                )
                self.assertEqual(len(matches), 1, matches)
                self.assertTrue(matches[0]["text"].endswith("23"))
            description_matches = BRIDGE._find_exact_target_spans(
                document[0],
                pymupdf.Rect(first["field_bboxes"]["description"]),
                "CREDIT INTEREST",
            )
            self.assertEqual(len(description_matches), 1, description_matches)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "edited.pdf"
            report = BRIDGE.apply_many_edits(
                str(fixture),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": first["field_bboxes"]["date"],
                        "old_text": first["date"],
                        "new_text": "31 JUL",
                    },
                    {
                        "page": 0,
                        "rect": second["field_bboxes"]["date"],
                        "old_text": second["date"],
                        "new_text": "01 AUG",
                    },
                ],
            )
            self.assertTrue(report["success"], report)
            self.assertEqual((report["matched"], report["placed"]), (2, 2))
            with pymupdf.open(output) as document:
                text = document[0].get_text()
                self.assertIn("31 JUL", text)
                self.assertIn("01 AUG", text)


if __name__ == "__main__":
    unittest.main()
