#!/usr/bin/env python3
"""Unit-test Stage F helpers: metric normalization + donor scoring."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "python"))
import font_replicator as FR

ok = True

# _char_class
assert FR._char_class("5") == "digit"
assert FR._char_class("A") == "upper"
assert FR._char_class("a") == "lower"
assert FR._char_class("$") == "other"
print("char_class: ok")

# _infer_weight_width
w, wd = FR._infer_weight_width("HelveticaNeue-BoldCondensed")
assert w == 700, w
assert wd == 3, wd
w, wd = FR._infer_weight_width("Aeonik2.0-Light")
assert w == 300, w
w, wd = FR._infer_weight_width("Times-Roman")
assert w == 400, w
print("infer_weight_width: ok")

# Build a tiny synthetic font with fixed digit advance, verify
# _host_class_advances finds it, and _shift_glyph_x translates outlines.
from fontTools.ttLib import TTFont
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

def make_font(digit_adv=600, upm=1000):
    fb = FontBuilder(upm, isTTF=True)
    glyph_order = [".notdef"] + [str(d) for d in range(10)]
    fb.setupGlyphOrder(glyph_order)
    cmap = {ord(str(d)): str(d) for d in range(10)}
    fb.setupCharacterMap(cmap)
    pen = TTGlyphPen(None)
    pen.moveTo((100, 0)); pen.lineTo((100, 700)); pen.lineTo((400, 700)); pen.lineTo((400, 0)); pen.closePath()
    box = pen.glyph()
    empty = TTGlyphPen(None).glyph()
    glyphs = {".notdef": empty}
    for d in range(10):
        glyphs[str(d)] = box
    fb.setupGlyf(glyphs)
    metrics = {".notdef": (digit_adv, 0)}
    for d in range(10):
        metrics[str(d)] = (digit_adv, 0)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "TestFont", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    return fb.font

f = make_font(digit_adv=600)
cmap = f.getBestCmap()
adv = FR._host_class_advances(f, cmap, f["hmtx"])
assert adv.get("digit") == 600, adv
print("host_class_advances: ok", adv)

# shift a glyph
g = f["glyf"]["5"]
g.expand(f["glyf"])
xs_before = [c[0] for c in g.coordinates]
FR._shift_glyph_x(g, 50)
xs_after = [c[0] for c in g.coordinates]
assert all(b + 50 == a for b, a in zip(xs_before, xs_after)), (xs_before, xs_after)
print("shift_glyph_x: ok")

print("\nALL STAGE F HELPER TESTS PASSED")
