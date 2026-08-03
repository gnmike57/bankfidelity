import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("westpac_description_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


class WestpacPrecedingDescriptionTests(unittest.TestCase):
    def test_description_above_date_row_is_attached_without_count_drift(self):
        fixture = ROOT / "AU Bank Statements" / "westpac_choicebasic_example.pdf"
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
        description = target["field_bboxes"]["description"]
        date = target["field_bboxes"]["date"]
        self.assertIsNotNone(description)
        self.assertAlmostEqual(description[1], date[1], places=2)
        self.assertAlmostEqual(target["credit"], 25.0, places=2)
        self.assertAlmostEqual(target["running_balance"], 576.87, places=2)


if __name__ == "__main__":
    unittest.main()
