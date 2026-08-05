"""Synthesise a tiny TTF subset (just '0','1','2','3','e' chars), then
ask the cascade to add '4' from a donor (the same source TTF with all
glyphs intact). This exercises Tier 2 without needing PyMuPDF or a real
PDF — it directly invokes _try_subset_extension.
"""
import os
import sys
import tempfile

sys.path.insert(0, 'python')
from fontTools.ttLib import TTFont
from fontTools.subset import Subsetter

import font_replicator

OUT = tempfile.mkdtemp(prefix='subset_test_')

# Use a system font as donor.
candidates = [
    'C:/Windows/Fonts/arial.ttf',
    'C:/Windows/Fonts/Arial.ttf',
    'C:/Windows/Fonts/calibri.ttf',
    'C:/Windows/Fonts/segoeui.ttf',
]
donor_path = next((c for c in candidates if os.path.isfile(c)), None)
if not donor_path:
    print('No donor candidate found.')
    sys.exit(1)
print(f'Donor: {donor_path}')

# Build a heavily-subset version of the donor with only '0123e'.
sub = TTFont(donor_path)
ss = Subsetter()
ss.populate(text='0123e')
ss.subset(sub)
subset_path = os.path.join(OUT, 'subset.ttf')
sub.save(subset_path)
print(f'Subset font saved: {os.path.getsize(subset_path)} bytes')

# Now run Tier 2 directly: extend the subset to also cover '4', '5', 'A'.
extended_path = os.path.join(OUT, 'extended.ttf')
extended, still = font_replicator._try_subset_extension(
    subset_path, donor_path, extended_path, ['4', '5', 'A']
)
print(f'extended: {extended}')
print(f'still missing: {still}')
print(f'extended file size: {os.path.getsize(extended_path) if os.path.isfile(extended_path) else "(none)"}')

# Verify the new glyphs exist in the cmap.
if os.path.isfile(extended_path):
    final = TTFont(extended_path)
    cmap = final.getBestCmap()
    for ch in '45A':
        present = ord(ch) in cmap
        glyph_name = cmap.get(ord(ch))
        print(f'  {ch!r} in cmap: {present}  glyph_name={glyph_name}')
    if all(ord(c) in cmap for c in '45A'):
        print('PASS: all extended chars in cmap')
    else:
        print('FAIL')
        sys.exit(1)
else:
    print('FAIL: extended file not created')
    sys.exit(1)
