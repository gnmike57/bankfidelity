import pymupdf
doc = pymupdf.open(r"C:\Users\zbook\Desktop\ing.pdf")
for page_num in range(len(doc)):
    page = doc[page_num]
    rects = page.search_for("Mr Peter Henry Hendel")
    for rect in rects:
        # Bbox is expected in some format, BankFidelity text --help didn't specify format, 
        # usually x0,y0,x1,y1
        print(f"Page {page_num}: {rect.x0},{rect.y0},{rect.x1},{rect.y1}")
