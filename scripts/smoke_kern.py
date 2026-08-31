"""Walk all spans and report kern map sizes."""
import sys
sys.path.insert(0, 'python')
import pymupdf
import pymupdf.pro
# pyrefly: ignore [missing-import]
import pymupdf_pro_integration as m

import os
pdf_path = sys.argv[1] if len(sys.argv) > 1 else 'AU Bank Statements/anz_example.pdf'
if not os.path.exists(pdf_path):
    pdf_path = 'examples/sample.pdf'

doc = pymupdf.open(pdf_path)

count = 0
kerned = 0
samples = []
for page in doc:
    for block in page.get_text('dict').get('blocks', []):
        for line in block.get('lines', []):
            for span in line.get('spans', []):
                text = span.get('text', '') or ''
                if len(text) < 2 or not any(c.isalnum() for c in text):
                    continue
                count += 1
                kmap = m._extract_kern_map(page, span)
                if kmap:
                    kerned += 1
                    samples.append((text, span.get('font'), kmap))

print(f'Total: {count}, kerned: {kerned}')
for text, font, kmap in samples[:5]:
    print(f'  {text!r:>30}  font={font}')
    for pair, d in sorted(kmap.items(), key=lambda kv: -abs(kv[1]))[:3]:
        print(f'    {pair}: {d:+.3f}')
doc.close()
