#!/usr/bin/env python3
"""End-to-end Stage F: extend a fixed-advance subset from a donor and verify
the injected glyph adopts the host's digit advance and remains renderable."""
import os, sys, tempfile
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))
import font_replicator as FR
from fontTools.ttLib import TTFont
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

def make_box_font(path, chars, advance=600, upm=1000, donor=False):
    fb = FontBuilder(upm, isTTF=True)
    order = [".notdef"] + [c if c.isalnum() else f"uni{ord(c):04X}" for c in chars]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({ord(c): order[i+1] for i, c in enumerate(chars)})
    pen = TTGlyphPen(None)
    # donor uses a WIDER box (900) to prove we re-normalize to host 600
    w = 850 if donor else 300
    pen.moveTo((50, 0)); pen.lineTo((50, 700)); pen.lineTo((50+w, 700)); pen.lineTo((50+w, 0)); pen.closePath()
    box = pen.glyph()
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    metrics = {".notdef": (advance, 0)}
    for i, c in enumerate(chars):
        glyphs[order[i+1]] = box
        metrics[order[i+1]] = (advance, 0)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "T", "styleName": "Regular"})
    fb.setupOS2(); fb.setupPost()
    fb.font.save(path)

d = tempfile.mkdtemp()
host = os.path.join(d, "host.ttf")
donor = os.path.join(d, "donor.ttf")
out = os.path.join(d, "extended.ttf")
# Host has digits 0-3 at advance 600; missing '7'. Donor has '7' at advance 950.
make_box_font(host, "0123", advance=600)
make_box_font(donor, "0123456789", advance=950, donor=True)

extended, still = FR._try_subset_extension(host, donor, out, ["7"])
print("extended:", extended, "still_missing:", still)
assert extended == ["7"], extended
assert os.path.isfile(out)

ext = TTFont(out)
cmap = ext.getBestCmap()
assert ord("7") in cmap, "7 not in extended cmap"
gname = cmap[ord("7")]
adv = ext["hmtx"].metrics[gname][0]
print(f"injected '7' advance = {adv} (host digit advance = 600)")
assert adv == 600, f"expected normalized advance 600, got {adv}"
print("\nSTAGE F E2E PASSED: donor glyph normalized to host metrics")
