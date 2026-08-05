"""Build a TTF subset with 'e' and combining acute (U+0301) but NOT 'é',
then ask Tier 1 to synthesise 'é'. Verifies the composite glyph path.
"""
import os
import sys
import tempfile

sys.path.insert(0, 'python')
from fontTools.ttLib import TTFont
from fontTools.subset import Subsetter

import font_replicator

OUT = tempfile.mkdtemp(prefix='composite_test_')

candidates = [
    'C:/Windows/Fonts/arial.ttf',
    'C:/Windows/Fonts/calibri.ttf',
    'C:/Windows/Fonts/segoeui.ttf',
]
donor_path = next((c for c in candidates if os.path.isfile(c)), None)
print(f'Donor: {donor_path}')

# Subset to include 'e' AND combining acute (U+0301) but NOT 'é' (U+00E9).
sub = TTFont(donor_path)
ss = Subsetter()
ss.populate(unicodes=[ord('e'), 0x0301, ord('a'), ord('b'), ord('0')])
ss.subset(sub)
subset_path = os.path.join(OUT, 'subset_no_eacute.ttf')
sub.save(subset_path)
print(f'Subset saved: {os.path.getsize(subset_path)} bytes')

# Verify subset has no 'é'.
test = TTFont(subset_path)
cmap_before = test.getBestCmap()
print(f'subset cmap has e: {ord("e") in cmap_before}')
print(f'subset cmap has U+0301: {0x0301 in cmap_before}')
print(f'subset cmap has é: {ord("é") in cmap_before}')

# Run Tier 1.
out_path = os.path.join(OUT, 'composite.ttf')
synthesised, still = font_replicator._try_composite_synthesis(
    subset_path, out_path, ['é']
)
print(f'synthesised: {synthesised}')
print(f'still: {still}')

# Verify.
if os.path.isfile(out_path):
    final = TTFont(out_path)
    cmap_after = final.getBestCmap()
    if ord('é') in cmap_after:
        print(f'PASS: é now in cmap as {cmap_after[ord("é")]}')
    else:
        print('FAIL: é not in cmap after synthesis')
        sys.exit(1)
else:
    print('FAIL: composite file not produced')
    sys.exit(1)
