"""Split AU PDFs > 15 pages into 15-page chunks for Document AI training."""
import sys
from pathlib import Path
import pymupdf

SRC = Path("AU Bank Statements")
OUT = Path("output/training_split")
LIMIT = 15

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for pdf in sorted(SRC.glob("*.pdf")):
        try:
            doc = pymupdf.open(pdf.as_posix())
        except Exception as e:
            print(f"  skip {pdf.name}: {e}")
            continue
        total = len(doc)
        if total <= LIMIT:
            print(f"  {pdf.name}: {total} pages — OK as-is, copying")
            (OUT / pdf.name).write_bytes(pdf.read_bytes())
            doc.close()
            continue
        # split into chunks of LIMIT pages
        chunks = (total + LIMIT - 1) // LIMIT
        for i in range(chunks):
            start = i * LIMIT
            end = min(start + LIMIT, total) - 1  # inclusive
            stem = pdf.stem
            chunk_path = OUT / f"{stem} (part {i + 1} of {chunks}).pdf"
            new_doc = pymupdf.open()
            new_doc.insert_pdf(doc, from_page=start, to_page=end)
            new_doc.save(chunk_path.as_posix())
            new_doc.close()
            print(f"  {pdf.name}: pages {start+1}-{end+1} -> {chunk_path.name}")
        doc.close()

if __name__ == "__main__":
    main()
