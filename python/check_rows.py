import pymupdf
import pymupdf_pro_integration as ppi

doc = pymupdf.open("AU Bank Statements/bankwest_example.pdf")
words = doc[0].get_text("words")
rows = ppi._transaction_rows(words)
for r in rows:
    date, cnt, bbox = ppi._transaction_date_prefix(r)
    if date:
        print("MATCHED DATE:", date, "in row:", [" ".join(str(w[4]) for w in r)])
    else:
        print("No date match in row:", [" ".join(str(w[4]) for w in r)])
