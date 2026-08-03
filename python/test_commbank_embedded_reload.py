import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("commbank_reload_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class CommBankEmbeddedReloadTests(unittest.TestCase):
    def test_embedded_text_is_reloaded_and_negative_balance_preserves_minus(self):
        fixture = ROOT / "AU Bank Statements" / "commbank_smartaccess_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            segment = root / "segment.pdf"
            output = root / "output.pdf"
            with pymupdf.open(fixture) as source:
                target = pymupdf.open()
                target.insert_pdf(source, from_page=0, to_page=2)
                target.save(segment, garbage=3, deflate=True)
                target.close()
            source_hash = sha256(segment)
            report = BRIDGE.apply_many_edits(
                str(segment),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": [88.50537872314453, 664.3323974609375, 264.2167663574219, 675.302734375],
                        "old_text": "Fast Transfer From KIONA BULLINGHAM to",
                        "new_text": "VISA DEBIT PURCHASE CARD 7718 AFTERPAY",
                    },
                    {
                        "page": 0,
                        "rect": [348.0191345214844, 511.7144775390625, 376.4490051269531, 522.684814453125],
                        "old_text": "500",
                        "new_text": "120",
                    },
                    {
                        "page": 0,
                        "rect": [486.1709899902344, 588.0234375, 527.5235595703125, 598.9937744140625],
                        "old_text": "42455.31",
                        "new_text": "-1021.45",
                    },
                ],
            )
            self.assertTrue(report["success"], report)
            self.assertEqual(
                report["method_per_edit"][:2],
                ["embedded", "simple-source-resource"],
            )
            self.assertEqual(report["method_per_edit"][2], "simple-source-resource")
            with pymupdf.open(output) as document:
                text = document[0].get_text()
                normalized = BRIDGE._normalized_pdf_text(text)
                self.assertIn(
                    BRIDGE._normalized_pdf_text("VISA DEBIT PURCHASE CARD 7718 AFTERPAY"),
                    normalized,
                )
                self.assertIn(BRIDGE._normalized_pdf_text("120"), normalized)
                self.assertIn(BRIDGE._normalized_pdf_text("-1021.45"), normalized)
            self.assertEqual(sha256(segment), source_hash)

    def test_same_length_macroman_amount_uses_source_resource(self):
        fixture = ROOT / "AU Bank Statements" / "commbank_smartaccess_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        rect = [342.0320129394531, 514.2825927734375, 375.6309509277344, 525.2529296875]
        with pymupdf.open(fixture) as document:
            pages = [
                index
                for index, page in enumerate(document)
                if len(BRIDGE._find_exact_target_spans(page, pymupdf.Rect(rect), "500")) == 1
            ]
        self.assertEqual(pages, [0, 2], pages)
        with tempfile.TemporaryDirectory() as directory:
            source_hash = sha256(fixture)
            for page_number in pages:
                output = Path(directory) / f"amount-{page_number}.pdf"
                report = BRIDGE.apply_many_edits(
                    str(fixture),
                    str(output),
                    [{
                        "page": page_number,
                        "rect": rect,
                        "old_text": "500",
                        "new_text": "300",
                    }],
                )
                self.assertTrue(report["success"], report)
                self.assertEqual(report["method_per_edit"], ["simple-source-resource"])
                with pymupdf.open(output) as document:
                    self.assertIn("300", document[page_number].get_text())
            self.assertEqual(sha256(fixture), source_hash)

    def test_repeated_glyph_description_uses_verified_standard14(self):
        fixture = ROOT / "AU Bank Statements" / "commbank_smartaccess_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        rect = [88.11507415771484, 201.6929473876953, 228.12635803222656, 212.6632843017578]
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "pizza.pdf"
            source_hash = sha256(fixture)
            report = BRIDGE.apply_many_edits(
                str(fixture),
                str(output),
                [{
                    "page": 2,
                    "rect": rect,
                    "old_text": "WITHDRAWAL AT Handybank Atm",
                    "new_text": "Debit Card Purchase Bay Beach Pizza & Pas",
                }],
            )
            self.assertTrue(report["success"], report)
            self.assertEqual(report["method_per_edit"], ["verified-standard14"])
            with pymupdf.open(output) as document:
                normalized = BRIDGE._normalized_pdf_text(document[2].get_text())
                self.assertIn(
                    BRIDGE._normalized_pdf_text(
                        "Debit Card Purchase Bay Beach Pizza & Pas"
                    ),
                    normalized,
                )
            self.assertEqual(sha256(fixture), source_hash)


if __name__ == "__main__":
    unittest.main()
