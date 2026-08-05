import pymupdf, sys, glob, os, re

paths = sys.argv[1:] if len(sys.argv) > 1 else (
    glob.glob("examples/*.pdf") + glob.glob("AU Bank Statements/*.pdf")
)
num_re = re.compile(r"\d[\d,]*\.\d{2}")
for p in paths:
    try:
        d = pymupdf.open(p)
    except Exception as e:
        print(f"{p}: OPEN FAILED {e}")
        continue
    npages = d.page_count
    # find first numeric (money-looking) span on page 0
    money = []
    pg = d[0]
    for b in pg.get_text("dict")["blocks"]:
        if "lines" not in b:
            continue
        for l in b["lines"]:
            for s in l["spans"]:
                if num_re.search(s["text"]):
                    money.append((round(s["bbox"][0],1),round(s["bbox"][1],1),round(s["bbox"][2],1),round(s["bbox"][3],1), s["text"], s["font"]))
    print(f"\n{p}  pages={npages}  money_spans_p0={len(money)}")
    for m in money[:4]:
        print("   ", m)
    d.close()
