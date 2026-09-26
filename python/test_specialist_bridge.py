import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import specialist_bridge as sb

REPO = Path(__file__).resolve().parent.parent
SAMPLE = REPO / "AU Bank Statements" / "anz_example.pdf"


def _png_with_bar(path: Path, y: int, x: int = 40) -> None:
    import cv2
    import numpy as np

    img = np.full((200, 300), 255, dtype="uint8")
    img[y : y + 6, x : x + 120] = 0
    cv2.imwrite(str(path), img)


class SpecialistBridgeContract(unittest.TestCase):
    def test_stub_font_match_is_unavailable_not_fabricated(self):
        res = sb.dispatch_request({"op": "match_font_contour", "glyph_crop_path": "x.png"})
        self.assertEqual(res["status"], "unavailable")
        self.assertNotIn("matched_font", res)

    def test_unknown_op_and_missing_file_are_errors(self):
        self.assertEqual(sb.dispatch_request({"op": "nope"})["status"], "error")
        res = sb.dispatch_request({"op": "identify_embedded_fonts", "pdf_path": "missing.pdf"})
        self.assertEqual(res["status"], "error")

    def test_optional_ml_backends_are_off_by_default(self):
        res = sb.dispatch_request({"op": "detect_layout_and_order", "image_path": __file__})
        self.assertEqual(res["status"], "unavailable")

    def test_nudge_fails_closed_on_unreadable_or_mismatched_images(self):
        res = sb.subpixel_differential_nudge("no.png", "no2.png")
        self.assertEqual(res["status"], "error")
        self.assertIs(res["approved"], False)

    def test_nudge_detects_shift_and_accepts_identity(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, c = (Path(d) / n for n in ("a.png", "b.png", "c.png"))
            _png_with_bar(a, 50)
            _png_with_bar(b, 50)
            _png_with_bar(c, 53)  # 3 px = 0.72 pt at 300 DPI
            same = sb.subpixel_differential_nudge(str(a), str(b))
            self.assertEqual(same["status"], "ok")
            self.assertTrue(same["approved"])
            shifted = sb.subpixel_differential_nudge(str(c), str(a))
            self.assertFalse(shifted["approved"])
            self.assertAlmostEqual(shifted["delta_y"], 3 * sb.PT_PER_PX, places=2)

    @unittest.skipUnless(SAMPLE.is_file(), "sample statement not present")
    def test_embedded_fonts_and_vector_search_on_real_pdf(self):
        fonts = sb.identify_embedded_fonts(str(SAMPLE))
        self.assertIn(fonts["status"], {"ok", "unavailable"})
        if fonts["status"] == "ok":
            self.assertTrue(all("basefont" in f for f in fonts["fonts"]))
        miss = sb.regress_subpixel_bbox(str(SAMPLE), "zzz-not-on-page-zzz")
        self.assertEqual(miss["status"], "unavailable")

    def test_interactive_loop_survives_bad_json(self):
        proc = subprocess.run(
            [sys.executable, str(Path(sb.__file__)), "--interactive"],
            input='not json\n{"op":"nope"}\n',
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=60,
        )
        lines = [json.loads(x) for x in proc.stdout.splitlines()]
        self.assertEqual([r["status"] for r in lines], ["error", "error"])


if __name__ == "__main__":
    unittest.main()
