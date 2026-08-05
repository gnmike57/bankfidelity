import pymupdf
doc = pymupdf.open("AU Bank Statements/bankwest_example.pdf")
print(doc[0].get_text("text")[1000:2000])
