import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("chunk_resource_bridge", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)


class ChunkPdfResourcePreservationTests(unittest.TestCase):
    def test_repeated_macquarie_clones_keep_text_across_three_page_chunks(self):
        fixture = ROOT / "AU Bank Statements" / "macquarie_example.pdf"
        if not fixture.exists():
            self.skipTest(f"real fixture missing: {fixture}")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cloned = root / "cloned.pdf"
            chunks_dir = root / "chunks"
            clone_report = BRIDGE.clone_pages(str(fixture), str(cloned), [0, 0, 0])
            self.assertTrue(clone_report["success"], clone_report)
            self.assertEqual(clone_report["cloned"], 3)
            chunks = BRIDGE.chunk_pdf_for_docai(str(cloned), str(chunks_dir), 3)
            self.assertEqual(
                [(item["page_offset"], item["page_count"]) for item in chunks],
                [(0, 3), (3, 2)],
            )
            with pymupdf.open(chunks[0]["path"]) as first:
                self.assertEqual(first.page_count, 3)
                for page in first:
                    self.assertIn("09/02/2026", page.get_text())
                date_rect = list(first[0].search_for("09/02/2026")[0])
            with pymupdf.open(chunks[1]["path"]) as second:
                self.assertEqual(second.page_count, 2)
                self.assertIn("09/02/2026", second[0].get_text())
            edited = root / "edited_first_chunk.pdf"
            report = BRIDGE.apply_many_edits(
                chunks[0]["path"],
                str(edited),
                [
                    {
                        "page": 0,
                        "rect": date_rect,
                        "old_text": "09/02/2026",
                        "new_text": "10/02/2026",
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            with pymupdf.open(edited) as document:
                self.assertIn("10/02/2026", document[0].get_text())
                self.assertIn("09/02/2026", document[1].get_text())
                self.assertIn("09/02/2026", document[2].get_text())


if __name__ == "__main__":
    unittest.main()
