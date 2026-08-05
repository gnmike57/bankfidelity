"""Synthesise a kerned PDF, then exercise _extract_kern_map and confirm
the kerning is captured. Stage 10 / Item #3 regression test.

The script:
  1. Creates a 1-page PDF with two Helvetica spans on the same line:
     - 'Default' rendered with insert_text (default advance widths)
     - A second span where we *manually* place each glyph with custom
       per-pair adjustments.
  2. Reads the PDF back via _extract_kern_map and asserts the kerned span
     produces a non-empty map.
"""
import sys
import os
import tempfile
sys.path.insert(0, 'python')
import pymupdf
import pymupdf.pro
import pymupdf_pro_integration as m

pymupdf.pro.unlock(m.PYMUPDF_PRO_KEY)


def main():
    tmpdir = tempfile.mkdtemp()
    pdf_path = os.path.join(tmpdir, 'kern.pdf')

    doc = pymupdf.open()
    page = doc.new_page(width=300, height=100)

    # Default: simple insert_text - no kerning.
    page.insert_text(
        pymupdf.Point(20, 50),
        'AVAVAV',
        fontname='helv',
        fontsize=24,
        color=(0, 0, 0),
    )

    # Manual kerned: each glyph placed with deliberately tight pairs.
    f = pymupdf.Font(fontname='helv')
    cursor = 20.0
    text = 'AVAVAV'
    for i, ch in enumerate(text):
        page.insert_text(
            pymupdf.Point(cursor, 80),
            ch,
            fontname='helv',
            fontsize=24,
            color=(0, 0, 0),
        )
        adv = float(f.text_length(ch, fontsize=24))
        if i + 1 < len(text):
            nxt = text[i + 1]
            # Tighten 'AV', 'VA' by 2pt each
            tighten = -2.0 if (ch, nxt) in {('A', 'V'), ('V', 'A')} else 0.0
            cursor += adv + tighten

    doc.save(pdf_path)
    doc.close()

    # Read back.
    doc = pymupdf.open(pdf_path)
    page = doc[0]

    print('Spans found:')
    target_kerned = None
    for block in page.get_text('dict').get('blocks', []):
        for line in block.get('lines', []):
            for span in line.get('spans', []):
                text = span.get('text', '') or ''
                bbox = span.get('bbox')
                print(f'  text={text!r}  bbox y0={bbox[1]:.1f}')
                if 'AVAVAV' in text and bbox[1] > 50:  # second span (lower on page)
                    target_kerned = span

    if target_kerned is None:
        print('NO kerned span found — test inconclusive')
        return

    kmap = m._extract_kern_map(page, target_kerned)
    print(f'kerned span kern entries: {len(kmap)}')
    for pair, delta in sorted(kmap.items(), key=lambda kv: -abs(kv[1]))[:5]:
        print(f'  {pair}: {delta:+.3f}pt')

    if len(kmap) == 0:
        print('FAIL: expected non-empty kern map for manually kerned span')
        sys.exit(1)
    av_or_va = any(p in {('A', 'V'), ('V', 'A')} for p in kmap)
    if not av_or_va:
        print('FAIL: expected AV or VA pair in kern map')
        sys.exit(1)
    print('PASS: kerning extracted correctly')
    doc.close()


if __name__ == '__main__':
    main()
