"""Inspect AU bank statement PDFs to learn header signatures and column ranges.

Reads every PDF in `AU Bank Statements/`, extracts text + word-level
bounding boxes from page 1, and prints a YAML-ready summary for each
that we can drop into `bank_templates/`.

Heuristic, not magic: the column ranges are estimated from the x-positions
of common header tokens like "Date", "Description", "Debit", "Credit",
"Balance". A human still has to skim and confirm.
"""
import os
import re
import sys
from pathlib import Path

try:
    import pymupdf
except ImportError:
    print("pip install pymupdf  (PyMuPDF)", file=sys.stderr)
    sys.exit(2)


KEYWORDS = {
    "date":        re.compile(r"^(date|transaction\s*date|posting\s*date)$",   re.I),
    "description": re.compile(r"^(description|transaction|details|particulars|narrative)$", re.I),
    "debit":       re.compile(r"^(debit|withdrawal|withdrawals?|out|paid out|money out)$",  re.I),
    "credit":      re.compile(r"^(credit|deposit|deposits?|in|paid in|money in)$",          re.I),
    "amount":      re.compile(r"^(amount|value)$",                              re.I),
    "balance":     re.compile(r"^(balance|running\s*balance|closing\s*balance)$", re.I),
}


def slug(s: str) -> str:
    s = s.lower().strip().replace("&", "and")
    s = re.sub(r"[^a-z0-9]+", "_", s)
    return s.strip("_")


def header_words(pdf_path: Path):
    """Return the candidate header words on the first page."""
    doc = pymupdf.open(pdf_path.as_posix())
    page = doc[0]
    words = page.get_text("words")  # [x0,y0,x1,y1,word,block,line,word_no]
    doc.close()
    return words


def find_columns(words):
    """For each known column label, find x-range from header words."""
    cols = {}
    for w in words:
        x0, y0, x1, y1, text = w[0], w[1], w[2], w[3], w[4]
        token = text.strip()
        for col_name, pat in KEYWORDS.items():
            if pat.match(token):
                cols.setdefault(col_name, []).append((x0, x1, y0))
    # collapse: keep the highest-up (smallest y0) match per column,
    # since headers typically sit near the top of the page.
    out = {}
    for name, hits in cols.items():
        hits.sort(key=lambda h: h[2])
        x0, x1, _ = hits[0]
        # round to 1pt to make YAML clean
        out[name] = [round(x0, 1), round(x1, 1)]
    return out


def header_signatures(words, max_keep: int = 4):
    """Pick distinctive *bank-characteristic* tokens from the top quarter of page 1.

    A signature is good when it identifies the bank/product and would
    appear on every statement that bank issues. Filter out:
      - Numbers and currency strings (statement-specific)
      - Likely person names (capitalised, not in a stop-list of bank words)
      - Dates and date-month names
      - Filler tokens like "Page", "Statement", "of"
    """
    if not words:
        return []
    page_height = max((w[3] for w in words), default=842.0)
    top = [w for w in words if w[3] < page_height * 0.30]

    # Tokens that are common across bank statements but don't pin down which bank.
    GENERIC = {
        "page", "statement", "account", "summary", "details", "period",
        "from", "your", "the", "of", "for", "and", "this", "that",
        "balance", "opening", "closing", "total",
        "january", "february", "march", "april", "may", "june",
        "july", "august", "september", "october", "november", "december",
        "miss", "mr", "mrs", "ms", "dr",
    }

    # Bank-characteristic tokens (we positively want to see these).
    BANK_HINTS = re.compile(
        r"^(anz|westpac|nab|cba|commbank|commonwealth|ing|macquarie|bendigo|"
        r"suncorp|bankwest|me\b|ubank|judo|amp|orange|everyday|plus|"
        r"choice|business|one|basic|saver|essentials?)$",
        re.I,
    )

    candidates = []
    for w in top:
        token = w[4].strip()
        if len(token) < 3:
            continue
        # Skip obvious statement-specific data
        if any(c.isdigit() for c in token):
            continue
        if token.startswith("$") or token.endswith("%"):
            continue
        if not token.replace("-", "").replace("'", "").isalpha():
            continue
        if token.lower() in GENERIC:
            continue
        candidates.append((token, BANK_HINTS.match(token) is not None))

    # Prefer bank-hint tokens, then keep insertion order, dedupe.
    candidates.sort(key=lambda c: (0 if c[1] else 1,))
    seen = set()
    sigs = []
    for token, _ in candidates:
        if token not in seen:
            seen.add(token)
            sigs.append(token)
        if len(sigs) >= max_keep:
            break
    return sigs


def detect_date_format(words):
    """Look for any token that matches common date patterns."""
    text = " ".join(w[4] for w in words[:200])
    if re.search(r"\b\d{1,2}/\d{1,2}/\d{2,4}\b", text):
        return "%d/%m/%Y"  # DD/MM/YYYY (AU default)
    if re.search(r"\b\d{1,2}-\d{1,2}-\d{2,4}\b", text):
        return "%d-%m-%Y"
    if re.search(r"\b\d{1,2}\s(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)", text):
        return "%d %b %Y"
    return "%d/%m/%Y"


def render_yaml(template_id: str, sigs, cols, date_format: str):
    lines = [
        f"# Auto-generated from AU Bank Statements/. Review the column x-ranges before relying on them.",
        f"id: {template_id}",
        f"header_signatures:",
    ]
    for s in sigs:
        # yaml-safe quoting
        lines.append(f"  - \"{s}\"")
    lines.append(f'date_format: "{date_format}"')
    lines.append(r'amount_regex: "^-?\\$?[0-9,]+\\.\\d{2}$"')
    lines.append("column_x_ranges:")
    for name in ("date", "description", "debit", "credit", "amount", "balance"):
        if name in cols:
            x0, x1 = cols[name]
            # widen slightly so transactions a couple of points off still hit
            lines.append(f"  {name}: [{x0:.1f}, {x1 + 60:.1f}]")
    return "\n".join(lines) + "\n"


def main():
    src = Path("AU Bank Statements")
    out = Path("bank_templates")
    out.mkdir(parents=True, exist_ok=True)

    if not src.exists():
        print(f"missing {src}", file=sys.stderr)
        sys.exit(1)

    pdfs = sorted(src.glob("*.pdf"))
    print(f"found {len(pdfs)} PDFs in {src}")

    for pdf in pdfs:
        try:
            words = header_words(pdf)
        except Exception as e:
            print(f"  skip {pdf.name}: {e}")
            continue

        sigs = header_signatures(words)
        cols = find_columns(words)
        date_format = detect_date_format(words)

        if not sigs:
            print(f"  skip {pdf.name}: no header words found")
            continue

        # derive template id from a distinctive word in the filename
        stem = pdf.stem.lower()
        if "anz" in stem:
            tid = "anz_plus_au"
        elif "westpac" in stem and "business" in stem:
            tid = "westpac_business_one_au"
        elif "westpac" in stem:
            tid = "westpac_choice_basic_au"
        elif "ing" in stem:
            tid = "ing_orange_au"
        elif "ia_" in stem or stem.startswith("ia"):
            tid = "ia_bank_au"
        elif stem.startswith("80") or "accountstatement" in stem:
            tid = "au_bank_statement_807466413"
        else:
            tid = slug(stem)[:40] or "au_unknown"

        yaml_out = render_yaml(tid, sigs, cols, date_format)
        target = out / f"{tid}.yaml"
        target.write_text(yaml_out, encoding="utf-8")
        print(f"  wrote {target}")
        print(f"    signatures: {sigs}")
        print(f"    columns:    {sorted(cols.keys())}")
        print(f"    date fmt:   {date_format}")


if __name__ == "__main__":
    main()
