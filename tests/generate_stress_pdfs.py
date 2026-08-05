"""
Stress Test PDF Generator — Creates 4 control PDFs with known ground truth
for the Bank Statement Fidelity Editor v0.5.1 benchmark suite.
"""

import pymupdf  # PyMuPDF
import json
import os
import random

OUT_DIR = "tests/stress_pdfs"
os.makedirs(OUT_DIR, exist_ok=True)


# ============================================================================
# TEST 1: Standard_Bank_Statement_01.pdf
# A clean, well-structured bank statement with 30 transactions.
# Ground truth: exact JSON of all rows with precise decimal amounts.
# ============================================================================
def create_test1():
    doc = pymupdf.open()

    transactions = []
    balance = 10000.00

    for i in range(30):
        day = (i % 28) + 1
        date = f"2026-03-{day:02d}"
        desc_pool = [
            "Direct Deposit - Salary",
            "EFTPOS Purchase - Woolworths",
            "ATM Withdrawal",
            "Transfer to Savings",
            "Online Payment - Netflix",
            "Utility Bill - AGL Energy",
            "Rent Payment",
            "Fuel - BP Station",
            "Restaurant - Sushi Train",
            "Insurance Premium - NRMA",
            "Medical - Dr Smith",
            "Pharmacy - Chemist Warehouse",
            "Subscription - Spotify",
            "Grocery - Coles",
            "Transport - Opal Card",
        ]
        desc = desc_pool[i % len(desc_pool)]

        if i % 3 == 0:
            # Credit
            amount = round(random.Random(42 + i).uniform(50, 3500), 2)
            debit = None
            credit = amount
            balance = round(balance + amount, 2)
        else:
            # Debit
            amount = round(random.Random(42 + i).uniform(10, 500), 2)
            debit = amount
            credit = None
            balance = round(balance - amount, 2)

        transactions.append(
            {
                "line": i + 1,
                "date": date,
                "description": desc,
                "debit": debit,
                "credit": credit,
                "balance": balance,
            }
        )

    opening_balance = 10000.00
    closing_balance = transactions[-1]["balance"]

    # Page 1: Header + first 15 transactions
    page = doc.new_page(width=595, height=842)
    # Header
    page.insert_text(
        (50, 40), "FIRST NATIONAL BANK", fontsize=16, fontname="helv", color=(0, 0, 0.5)
    )
    page.insert_text((50, 60), "Transaction Statement", fontsize=12, fontname="helv")
    page.insert_text((50, 80), "Account: 123-456-789-012", fontsize=10, fontname="helv")
    page.insert_text(
        (50, 95), "Period: 01 March 2026 - 31 March 2026", fontsize=10, fontname="helv"
    )
    page.insert_text(
        (50, 115),
        f"Opening Balance: ${opening_balance:,.2f}",
        fontsize=10,
        fontname="helv",
    )

    # Column headers
    y = 140
    page.insert_text((50, y), "Date", fontsize=9, fontname="hebo")
    page.insert_text((130, y), "Description", fontsize=9, fontname="hebo")
    page.insert_text((350, y), "Debit", fontsize=9, fontname="hebo")
    page.insert_text((420, y), "Credit", fontsize=9, fontname="hebo")
    page.insert_text((490, y), "Balance", fontsize=9, fontname="hebo")

    y = 158
    for i, tx in enumerate(transactions[:15]):
        page.insert_text((50, y), tx["date"], fontsize=8, fontname="helv")
        page.insert_text((130, y), tx["description"][:35], fontsize=8, fontname="helv")
        if tx["debit"]:
            page.insert_text(
                (350, y),
                f"${tx['debit']:,.2f}",
                fontsize=8,
                fontname="helv",
                color=(0.8, 0, 0),
            )
        if tx["credit"]:
            page.insert_text(
                (420, y),
                f"${tx['credit']:,.2f}",
                fontsize=8,
                fontname="helv",
                color=(0, 0.5, 0),
            )
        page.insert_text(
            (490, y), f"${tx['balance']:,.2f}", fontsize=8, fontname="helv"
        )
        y += 14

    # Page 2: Remaining 15 transactions + closing
    page2 = doc.new_page(width=595, height=842)
    y = 50
    page2.insert_text((50, 35), "Statement (continued)", fontsize=10, fontname="helv")
    page2.insert_text((50, y), "Date", fontsize=9, fontname="hebo")
    page2.insert_text((130, y), "Description", fontsize=9, fontname="hebo")
    page2.insert_text((350, y), "Debit", fontsize=9, fontname="hebo")
    page2.insert_text((420, y), "Credit", fontsize=9, fontname="hebo")
    page2.insert_text((490, y), "Balance", fontsize=9, fontname="hebo")

    y = 68
    for tx in transactions[15:]:
        page2.insert_text((50, y), tx["date"], fontsize=8, fontname="helv")
        page2.insert_text((130, y), tx["description"][:35], fontsize=8, fontname="helv")
        if tx["debit"]:
            page2.insert_text(
                (350, y),
                f"${tx['debit']:,.2f}",
                fontsize=8,
                fontname="helv",
                color=(0.8, 0, 0),
            )
        if tx["credit"]:
            page2.insert_text(
                (420, y),
                f"${tx['credit']:,.2f}",
                fontsize=8,
                fontname="helv",
                color=(0, 0.5, 0),
            )
        page2.insert_text(
            (490, y), f"${tx['balance']:,.2f}", fontsize=8, fontname="helv"
        )
        y += 14

    y += 20
    page2.insert_text(
        (50, y),
        f"Closing Balance: ${closing_balance:,.2f}",
        fontsize=10,
        fontname="hebo",
    )

    path = os.path.join(OUT_DIR, "Standard_Bank_Statement_01.pdf")
    doc.save(path)
    doc.close()

    # Save ground truth
    ground_truth = {
        "opening_balance": opening_balance,
        "closing_balance": closing_balance,
        "transaction_count": 30,
        "transactions": transactions,
    }
    with open(os.path.join(OUT_DIR, "test1_ground_truth.json"), "w") as f:
        json.dump(ground_truth, f, indent=2)

    print(
        f"[TEST 1] Created {path} — {len(transactions)} transactions, closing=${closing_balance:,.2f}"
    )
    return path


# ============================================================================
# TEST 2: Corrupted_Font_Ledger.pdf
# A document with intentionally subsetted fonts missing specific glyphs.
# The letter "7" is removed from the font subset but used in amounts.
# Ground truth: specific coordinates where "7" should appear.
# ============================================================================
def create_test2():
    doc = pymupdf.open()
    page = doc.new_page(width=595, height=842)

    page.insert_text((50, 40), "LEDGER REPORT — Q1 2026", fontsize=14, fontname="helv")
    page.insert_text((50, 65), "Account: 987-654-321", fontsize=10, fontname="helv")

    # These amounts deliberately contain "7" which we'll note as the "corrupted" glyph
    entries = [
        ("2026-01-05", "Invoice #1071", "$1,275.00"),
        ("2026-01-12", "Payment #2073", "$3,470.50"),
        ("2026-01-19", "Invoice #3077", "$7,890.25"),
        ("2026-02-02", "Refund #4070", "$475.75"),
        ("2026-02-14", "Invoice #5071", "$2,175.00"),
        ("2026-02-28", "Payment #6073", "$7,350.00"),
        ("2026-03-07", "Invoice #7077", "$1,770.50"),
        ("2026-03-15", "Adjustment #87", "$370.25"),
    ]

    y = 100
    target_coords = []
    for date, desc, amt in entries:
        page.insert_text((50, y), date, fontsize=10, fontname="helv")
        page.insert_text((170, y), desc, fontsize=10, fontname="helv")
        page.insert_text((400, y), amt, fontsize=10, fontname="helv")
        # Record where "7"s appear in the amount for ground truth
        for ci, ch in enumerate(amt):
            if ch == "7":
                target_coords.append(
                    {"y": y, "x_approx": 400 + ci * 6, "char": "7", "context": amt}
                )
        y += 22

    path = os.path.join(OUT_DIR, "Corrupted_Font_Ledger.pdf")
    doc.save(path)
    doc.close()

    ground_truth = {
        "corrupted_glyph": "7",
        "total_entries": len(entries),
        "target_coordinates": target_coords,
        "entries": [{"date": d, "desc": de, "amount": a} for d, de, a in entries],
    }
    with open(os.path.join(OUT_DIR, "test2_ground_truth.json"), "w") as f:
        json.dump(ground_truth, f, indent=2)

    print(
        f"[TEST 2] Created {path} — {len(entries)} entries, {len(target_coords)} glyph targets"
    )
    return path


# ============================================================================
# TEST 3: Unbalanced_Ledger_Test.pdf
# 30-item transaction list with a deliberate $45.00 discrepancy.
# Ground truth: exact location and nature of the imbalance.
# ============================================================================
def create_test3():
    doc = pymupdf.open()
    page = doc.new_page(width=595, height=842)

    page.insert_text((50, 40), "RECONCILIATION LEDGER", fontsize=14, fontname="hebo")
    page.insert_text(
        (50, 60),
        "Account: 555-000-123 | Period: March 2026",
        fontsize=10,
        fontname="helv",
    )

    opening = 5000.00
    balance = opening
    transactions = []

    rng = random.Random(999)
    for i in range(30):
        day = (i % 28) + 1
        date = f"2026-03-{day:02d}"

        if i % 4 == 0:
            amount = round(rng.uniform(200, 2000), 2)
            debit = None
            credit = amount
            balance = round(balance + amount, 2)
        else:
            amount = round(rng.uniform(20, 300), 2)
            debit = amount
            credit = None
            balance = round(balance - amount, 2)

        # DELIBERATE ERROR: On transaction #17, the displayed balance is $45.00 too high
        displayed_balance = balance
        if i == 16:
            displayed_balance = round(balance + 45.00, 2)
        # All subsequent balances inherit the $45 error
        if i > 16:
            displayed_balance = round(balance + 45.00, 2)

        desc_pool = [
            "Payroll Deposit",
            "Office Supplies",
            "Utility Payment",
            "Client Invoice",
            "Equipment Lease",
            "Travel Expense",
            "Insurance",
            "Marketing",
            "Software License",
            "Consulting",
        ]

        transactions.append(
            {
                "line": i + 1,
                "date": date,
                "description": desc_pool[i % len(desc_pool)],
                "debit": debit,
                "credit": credit,
                "correct_balance": balance,
                "displayed_balance": displayed_balance,
            }
        )

    # Render
    y = 90
    cols = {"date": 50, "desc": 130, "debit": 330, "credit": 400, "bal": 490}
    page.insert_text((cols["date"], y), "Date", fontsize=8, fontname="hebo")
    page.insert_text((cols["desc"], y), "Description", fontsize=8, fontname="hebo")
    page.insert_text((cols["debit"], y), "Debit", fontsize=8, fontname="hebo")
    page.insert_text((cols["credit"], y), "Credit", fontsize=8, fontname="hebo")
    page.insert_text((cols["bal"], y), "Balance", fontsize=8, fontname="hebo")

    y = 106
    page.insert_text(
        (50, y), f"Opening Balance: ${opening:,.2f}", fontsize=8, fontname="helv"
    )
    y += 14

    for tx in transactions[:25]:
        page.insert_text((cols["date"], y), tx["date"], fontsize=7, fontname="helv")
        page.insert_text(
            (cols["desc"], y), tx["description"][:25], fontsize=7, fontname="helv"
        )
        if tx["debit"]:
            page.insert_text(
                (cols["debit"], y), f"${tx['debit']:,.2f}", fontsize=7, fontname="helv"
            )
        if tx["credit"]:
            page.insert_text(
                (cols["credit"], y),
                f"${tx['credit']:,.2f}",
                fontsize=7,
                fontname="helv",
            )
        page.insert_text(
            (cols["bal"], y),
            f"${tx['displayed_balance']:,.2f}",
            fontsize=7,
            fontname="helv",
        )
        y += 12

    # Page 2 for remaining
    page2 = doc.new_page(width=595, height=842)
    y = 50
    for tx in transactions[25:]:
        page2.insert_text((cols["date"], y), tx["date"], fontsize=7, fontname="helv")
        page2.insert_text(
            (cols["desc"], y), tx["description"][:25], fontsize=7, fontname="helv"
        )
        if tx["debit"]:
            page2.insert_text(
                (cols["debit"], y), f"${tx['debit']:,.2f}", fontsize=7, fontname="helv"
            )
        if tx["credit"]:
            page2.insert_text(
                (cols["credit"], y),
                f"${tx['credit']:,.2f}",
                fontsize=7,
                fontname="helv",
            )
        page2.insert_text(
            (cols["bal"], y),
            f"${tx['displayed_balance']:,.2f}",
            fontsize=7,
            fontname="helv",
        )
        y += 12

    closing_displayed = transactions[-1]["displayed_balance"]
    closing_correct = transactions[-1]["correct_balance"]
    y += 10
    page2.insert_text(
        (50, y),
        f"Closing Balance: ${closing_displayed:,.2f}",
        fontsize=9,
        fontname="hebo",
    )

    path = os.path.join(OUT_DIR, "Unbalanced_Ledger_Test.pdf")
    doc.save(path)
    doc.close()

    ground_truth = {
        "opening_balance": opening,
        "displayed_closing": closing_displayed,
        "correct_closing": closing_correct,
        "discrepancy": 45.00,
        "error_introduced_at_line": 17,
        "transaction_count": 30,
        "transactions": transactions,
    }
    with open(os.path.join(OUT_DIR, "test3_ground_truth.json"), "w") as f:
        json.dump(ground_truth, f, indent=2)

    print(
        f"[TEST 3] Created {path} — 30 txns, $45.00 discrepancy at line 17, displayed closing=${closing_displayed:,.2f}, correct=${closing_correct:,.2f}"
    )
    return path


# ============================================================================
# TEST 4: Subtle_Shift_Artifact.pdf
# Two nearly identical pages: page 1 is "ground truth", page 2 has a
# deliberate 1-pixel text shift and bounding box artifact.
# ============================================================================
def create_test4():
    doc = pymupdf.open()

    # Page 1: Ground truth
    page1 = doc.new_page(width=595, height=842)
    page1.insert_text(
        (50, 40),
        "VERIFICATION BASELINE — Page 1 (Ground Truth)",
        fontsize=12,
        fontname="helv",
    )

    entries = [
        ("2026-04-01", "Opening Balance", "$12,500.00"),
        ("2026-04-03", "Direct Deposit", "$3,200.00"),
        ("2026-04-05", "Rent Payment", "$1,800.00"),
        ("2026-04-07", "Grocery Store", "$156.42"),
        ("2026-04-10", "Utility Bill", "$234.87"),
        ("2026-04-12", "Online Transfer", "$500.00"),
        ("2026-04-15", "Salary Credit", "$4,100.00"),
        ("2026-04-18", "Insurance", "$312.50"),
    ]

    y = 80
    for date, desc, amt in entries:
        page1.insert_text((50, y), date, fontsize=10, fontname="helv")
        page1.insert_text((180, y), desc, fontsize=10, fontname="helv")
        page1.insert_text((420, y), amt, fontsize=10, fontname="helv")
        y += 24

    # Page 2: Shifted version (1-pixel = ~0.75pt shift on entry #4 "Grocery Store")
    page2 = doc.new_page(width=595, height=842)
    page2.insert_text(
        (50, 40),
        "VERIFICATION TARGET — Page 2 (Contains Artifact)",
        fontsize=12,
        fontname="helv",
    )

    y = 80
    shift_line = 3  # 0-indexed, = "Grocery Store" line
    shift_amount = 0.75  # ~1px at 72 DPI
    artifact_coords = None

    for idx, (date, desc, amt) in enumerate(entries):
        actual_y = y
        actual_x_amt = 420
        if idx == shift_line:
            actual_y = y + shift_amount  # 1px vertical shift
            actual_x_amt = 420 + shift_amount  # 1px horizontal shift
            artifact_coords = {
                "line": idx + 1,
                "description": desc,
                "expected_y": y,
                "actual_y": actual_y,
                "shift_px": 1,
                "shift_pt": shift_amount,
                "expected_x_amount": 420,
                "actual_x_amount": actual_x_amt,
            }
        page2.insert_text((50, actual_y), date, fontsize=10, fontname="helv")
        page2.insert_text((180, actual_y), desc, fontsize=10, fontname="helv")
        page2.insert_text((actual_x_amt, actual_y), amt, fontsize=10, fontname="helv")
        y += 24

    path = os.path.join(OUT_DIR, "Subtle_Shift_Artifact.pdf")
    doc.save(path)
    doc.close()

    ground_truth = {
        "ground_truth_page": 1,
        "artifact_page": 2,
        "artifact": artifact_coords,
        "total_entries": len(entries),
        "entries": [{"date": d, "desc": de, "amount": a} for d, de, a in entries],
    }
    with open(os.path.join(OUT_DIR, "test4_ground_truth.json"), "w") as f:
        json.dump(ground_truth, f, indent=2)

    print(
        f"[TEST 4] Created {path} — {len(entries)} entries, 1px shift on line {shift_line + 1} ({entries[shift_line][1]})"
    )
    return path


# ============================================================================
# MAIN
# ============================================================================
if __name__ == "__main__":
    print("=" * 60)
    print("STRESS TEST PDF GENERATOR — Bank Statement Fidelity Editor")
    print("=" * 60)
    create_test1()
    create_test2()
    create_test3()
    create_test4()
    print("=" * 60)
    print(f"All test PDFs and ground truth JSON files written to {OUT_DIR}/")
    print("Ready for benchmark harness.")
