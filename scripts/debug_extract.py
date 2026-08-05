import os, sys, tempfile, shutil, json
sys.path.insert(0, 'python')

# Build subsetted PDF same as the integration test does.
import pymupdf
import pymupdf.pro
from fontTools.ttLib import TTFont
from fontTools.subset import Subsetter
import pymupdf_pro_integration as m
pymupdf.pro.unlock(m.PYMUPDF_PRO_KEY)

tmp = tempfile.mkdtemp(prefix='ext_')
donor = r'C:\Windows\Fonts\arial.ttf'
sub = TTFont(donor)
ss = Subsetter()
ss.populate(text='0123e')
ss.subset(sub)
subset_path = os.path.join(tmp, 'subset.ttf')
sub.save(subset_path)

doc = pymupdf.open()
page = doc.new_page(width=400, height=100)
page.insert_font(fontname='testsubset', fontfile=subset_path)
page.insert_text(pymupdf.Point(20, 50), '0123e', fontname='testsubset', fontsize=24)
pdf_path = os.path.join(tmp, 'doc.pdf')
doc.save(pdf_path)
doc.close()

# Now mimic the cascade extraction path.
doc = pymupdf.open(pdf_path)
for p in doc:
    for f in p.get_fonts(full=True):
        print('font:', f)
        xref = f[0]
        info = doc.extract_font(xref)
        if isinstance(info, (tuple, list)):
            print('  type:', type(info))
            print('  len:', len(info))
            for i, item in enumerate(info):
                if isinstance(item, (bytes, bytearray)):
                    print(f'  item {i}: bytes ({len(item)})')
                else:
                    print(f'  item {i}: {item!r}')
        else:
            print('  dict:', info)
doc.close()
