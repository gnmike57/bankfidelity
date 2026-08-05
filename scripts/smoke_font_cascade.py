"""Smoke-test the Stage 11 font cascade.

Picks the largest embedded font in the AU statement, asks for a few
fictional 'missing' characters, and prints what each tier produced.
"""
import json
import os
import sys
import tempfile

sys.path.insert(0, 'python')
import pymupdf_pro_integration as m

OUT = tempfile.mkdtemp(prefix='font_cascade_')

# Use the real AU bank statement.
pdf = 'AU Bank Statements/ANZ Plus Statement March 2026.pdf'
font = 'Aeonik2.0-Regular'
# Mix: a precomposed accent (é = e + combining acute → Tier 1 candidate
# if the base e and combining acute are both already in the subset),
# and an uncommon Cyrillic letter (ж → Tier 2/3 candidate).
missing = ['é', 'ж', '✓']

result = m.replicate_font_for_missing_chars(pdf, font, ','.join(missing), OUT)
print(json.dumps(result, indent=2, default=str))
print()
print(f'Output directory: {OUT}')
print('Files produced:')
for f in os.listdir(OUT):
    full = os.path.join(OUT, f)
    print(f'  {f}  ({os.path.getsize(full)} bytes)')
