import pymupdf
doc = pymupdf.open("AU Bank Statements/commbank_smartaccess_example.pdf")
rect = pymupdf.Rect(86.9, 431.8, 156.17, 442.8)
print("TEXT IN RECT:", repr(doc[0].get_text("text", clip=rect)))
