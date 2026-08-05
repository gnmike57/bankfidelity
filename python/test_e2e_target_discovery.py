import importlib.util
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "e2e_pipeline.py"
SPEC = importlib.util.spec_from_file_location("e2e_pipeline", SCRIPT)
E2E = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(E2E)


class E2ETargetDiscoveryTests(unittest.TestCase):
    def test_real_anz_profile_discovers_native_amount_identity(self):
        fixture = ROOT / "AU Bank Statements" / "anz_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        bbox, source_text, visible_text, page = E2E.first_amount_span(str(fixture))
        self.assertEqual(page, 0)
        self.assertEqual(visible_text, "$0.80")
        self.assertEqual(source_text, ":\x0f39\x0f")
        self.assertEqual(bbox, [303.3, 333.0, 329.7, 344.4])


if __name__ == "__main__":
    unittest.main()
