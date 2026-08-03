import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("bankwest_standard14", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


class BankwestStandard14FallbackTests(unittest.TestCase):
    def test_exact_bankwest_date_uses_verified_standard14_when_subset_is_insufficient(self):
        fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            segment = root / "bankwest_page.pdf"
            output = root / "output.pdf"
            with pymupdf.open(fixture) as source:
                document = pymupdf.open()
                document.insert_pdf(source, from_page=0, to_page=0)
                document.save(segment, garbage=4, deflate=True)
                document.close()
            source_hash = hashlib.sha256(segment.read_bytes()).hexdigest()
            transaction = BRIDGE.get_all_transactions(str(segment))[0]
            report = BRIDGE.apply_many_edits(
                str(segment),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": transaction["field_bboxes"]["date"],
                        "old_text": transaction["date"],
                        "new_text": "31 JUL 23",
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["verified-standard14"])
            self.assertEqual(hashlib.sha256(segment.read_bytes()).hexdigest(), source_hash)
            with pymupdf.open(output) as edited:
                self.assertIn("31 JUL 23", edited[0].get_text())

    def test_long_bankwest_description_is_condensed_and_remains_extractable(self):
        fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output.pdf"
            transactions = BRIDGE.get_all_transactions(str(fixture))
            transaction = transactions[1]
            replacement = "VISA DEBIT PURCHASE CARD 7718 UBER* TRIP"
            report = BRIDGE.apply_many_edits(
                str(fixture),
                str(output),
                [
                    {
                        "page": transaction["page"],
                        "rect": transaction["field_bboxes"]["description"],
                        "old_text": "IB TRANSFER 781752755 TO 306-821-1290303",
                        "new_text": replacement,
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["verified-standard14"])
            with pymupdf.open(output) as edited:
                observed = " ".join(edited[transaction["page"]].get_text().split())
                self.assertIn(replacement, observed)

    def test_inset_bankwest_description_uses_origin_to_cell_fit(self):
        fixture = ROOT / "AU Bank Statements" / "bankwest_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output.pdf"
            transaction = BRIDGE.get_all_transactions(str(fixture))[1]
            replacement = "PAYMENT FROM MD SOHANUL ISLAM SOHAN"
            report = BRIDGE.apply_many_edits(
                str(fixture),
                str(output),
                [
                    {
                        "page": transaction["page"],
                        "rect": transaction["field_bboxes"]["description"],
                        "old_text": "IB TRANSFER 781752755 TO 306-821-1290303",
                        "new_text": replacement,
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            with pymupdf.open(output) as edited:
                self.assertIn(replacement, " ".join(edited[0].get_text().split()))


if __name__ == "__main__":
    unittest.main()
