import pymupdf
doc = pymupdf.open("examples/multi_page_fonts.pdf")
page = doc[0]
blocks = page.get_text("dict")["blocks"]
for b in blocks:
    if "lines" in b:
        for l in b["lines"]:
            for s in l["spans"]:
                if "Helvetica" in s["text"]:
                    print(s["bbox"])
