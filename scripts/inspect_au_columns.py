"""For each AU PDF, find the actual x-positions of:
   - first amount-like token per row (usually right-aligned columns)
to validate the hand-coded column ranges in the templates.
"""
import re
import sys
from pathlib import Path

import pymupdf

amount_pat = re.compile(r"^-?\$?[\d,]+\.\d{2}$")
date_pat = re.compile(r"^\d{1,2}[/\-]\d{1,2}([/\-]\d{2,4})?$|^\d{1,2}\s(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)$")


def main():
    src = Path("AU Bank Statements")
    for pdf in sorted(src.glob("*.pdf")):
        print(f"\n=== {pdf.name} ===")
        try:
            doc = pymupdf.open(pdf.as_posix())
        except Exception as e:
            print(f"  open failed: {e}")
            continue

        page = doc[1] if len(doc) > 1 else doc[0]  # page 2 usually has transaction tables
        words = page.get_text("words")

        # Group by y (rows)
        rows = {}
        for w in words:
            y = round(w[1] / 4) * 4  # bucket to 4pt bands
            rows.setdefault(y, []).append(w)

        # Sample first 4 rows that look like transactions (start with a date,
        # contain at least one amount).
        samples = 0
        for y in sorted(rows.keys()):
            row = sorted(rows[y], key=lambda w: w[0])
            row_text = " ".join(w[4] for w in row)
            has_date = any(date_pat.match(w[4]) for w in row)
            amounts = [w for w in row if amount_pat.match(w[4])]
            if has_date and amounts:
                date_x = next(w[0] for w in row if date_pat.match(w[4]))
                amt_xs = [w[2] for w in amounts]  # right edge for right-aligned
                print(f"  row y={y:>4}  date_x0={date_x:5.1f}  amount_x1s={[round(x,1) for x in amt_xs]}")
                print(f"    raw: {row_text[:100]}")
                samples += 1
                if samples >= 4:
                    break
        if samples == 0:
            print("  (no transaction rows detected on page 2)")

        doc.close()


if __name__ == "__main__":
    main()
