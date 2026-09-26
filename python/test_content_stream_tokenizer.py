#!/usr/bin/env python3
import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path

import pymupdf

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = Path(__file__).with_name("pymupdf_pro_integration.py")
SPEC = importlib.util.spec_from_file_location("pymupdf_pro_integration", MODULE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)
BRIDGE._ensure_pro_unlocked = lambda *_args, **_kwargs: None


class ContentStreamTokenizerTests(unittest.TestCase):
    def test_adversarial_escaped_paren_inside_string_literal(self):
        """Adversarial: a \\) escape inside a string literal must not yield a false match."""
        # Stream has /F1 12 Tf (Prefix\\)Target) Tj followed by (Target) Tj
        stream = b"/F1 12 Tf (Prefix\\)Target) Tj /F1 12 Tf (Target) Tj"
        tokens = list(BRIDGE.scan_content_stream_text_strings(stream))

        self.assertEqual(len(tokens), 2)
        # First string is (Prefix\)Target) with literal inner bytes b"Prefix\\)Target"
        self.assertEqual(tokens[0]["inner_bytes"], b"Prefix\\)Target")
        self.assertEqual(tokens[0]["operator"], b"Tj")
        self.assertEqual(tokens[0]["font_alias"], "F1")

        # Second string is (Target)
        self.assertEqual(tokens[1]["inner_bytes"], b"Target")
        self.assertEqual(tokens[1]["operator"], b"Tj")

        # A search for b"Target" with tokenizer matching exact inner_bytes must only find the second one!
        exact_matches = [
            t for t in tokens if t["type"] == "literal" and t["inner_bytes"] == b"Target"
        ]
        self.assertEqual(len(exact_matches), 1)
        self.assertEqual(exact_matches[0]["inner_offset"], stream.rfind(b"Target"))

    def test_adversarial_target_bytes_inside_inline_image(self):
        """Adversarial: target bytes inside an inline image must not yield a false match."""
        # Embed arbitrary payload matching b"(TARGET)Tj" inside an inline image
        stream = (
            b"/F1 12 Tf\n"
            b"BI\n"
            b"/W 16 /H 16 /CS /DeviceRGB /BPC 8\n"
            b"ID\n"
            b"\x00\x01\x02(TARGET)Tj\x03\x04\x05\n"
            b"EI\n"
            b"(ACTUAL) Tj"
        )
        tokens = list(BRIDGE.scan_content_stream_text_strings(stream))

        # Only the string outside the inline image should be yielded
        self.assertEqual(len(tokens), 1)
        self.assertEqual(tokens[0]["inner_bytes"], b"ACTUAL")
        self.assertEqual(tokens[0]["operator"], b"Tj")

        # Searching for TARGET must yield zero matches!
        target_matches = [t for t in tokens if t["inner_bytes"] == b"TARGET"]
        self.assertEqual(len(target_matches), 0)

    def test_byte_identity_fontfile2_and_fontfile3_preserved(self):
        """Byte-identity proof: before/after hash of /FontFile2 and /FontFile3 on an edited PDF."""
        fixture = ROOT / "AU Bank Statements" / "commbank_smartaccess_example.pdf"
        if not fixture.exists():
            self.skipTest(f"fixture missing: {fixture}")

        def extract_font_hashes(pdf_path: Path):
            hashes = {}
            with pymupdf.open(pdf_path) as doc:
                for xref in range(1, doc.xref_length()):
                    for key in ("FontFile2", "FontFile3"):
                        val = doc.xref_get_key(xref, key)
                        if val[0] == "xref":
                            target_xref = int(val[1].split()[0])
                            stream = doc.xref_stream(target_xref)
                            if stream:
                                base_font = doc.xref_get_key(xref, "BaseFont")[1]
                                hashes[(key, base_font)] = hashlib.sha256(stream).hexdigest()
            return hashes

        before_hashes = extract_font_hashes(fixture)
        self.assertTrue(len(before_hashes) > 0, "Fixture must contain /FontFile2 or /FontFile3")

        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "edited.pdf"
            # Apply an edit using one-byte in-place stream
            target_rect = [481.5, 145.3, 546.8, 156.9]
            report = BRIDGE.apply_many_edits(
                str(fixture),
                str(output),
                [
                    {
                        "page": 0,
                        "rect": target_rect,
                        "old_text": "$35,308.14 CR",
                        "new_text": "$35,408.14 CR",
                    }
                ],
            )
            self.assertTrue(report["success"], report)
            after_hashes = extract_font_hashes(output)

            # Assert every font stream is 100% byte-identical
            self.assertEqual(before_hashes, after_hashes)
            for (key, xref), h in before_hashes.items():
                self.assertEqual(
                    h,
                    after_hashes.get((key, xref)),
                    f"Font stream {key} (xref {xref}) byte-identity violated!",
                )


if __name__ == "__main__":
    unittest.main()
