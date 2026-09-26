import pymupdf
doc = pymupdf.open("AU Bank Statements/commbank_smartaccess_example.pdf")
words = doc[0].get_text("words")
print(words[:20])
