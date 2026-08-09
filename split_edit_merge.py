import os
import time
import subprocess
import pymupdf

in_pdf = r"C:\Users\zbook\Desktop\ing.pdf"
dates_pdf = r"C:\Users\zbook\Desktop\ing_dates.pdf"
p12_pdf = r"C:\Users\zbook\Desktop\ing_p12.pdf"
p12_edited_pdf = r"C:\Users\zbook\Desktop\ing_p12_edited.pdf"
final_pdf = r"C:\Users\zbook\Desktop\ing_final.pdf"
verify_dir = r"C:\Users\zbook\Desktop\verify_output"
bf_exe = r"C:\bankfidelity\bankfidelity\target\debug\dual-core-pdf-pipeline.exe"
temp_dir = r"C:\Users\zbook\Desktop\temp_pages"

os.makedirs(temp_dir, exist_ok=True)

print("Splitting document into single pages for adjust-dates...")
doc = pymupdf.open(in_pdf)
total_pages = len(doc)

for i in range(total_pages):
    page_pdf = os.path.join(temp_dir, f"page_{i}.pdf")
    temp_doc = pymupdf.open()
    temp_doc.insert_pdf(doc, from_page=i, to_page=i)
    temp_doc.save(page_pdf)
    temp_doc.close()
doc.close()

print("Running BankFidelity adjust-dates on each page...")
merged_dates_doc = pymupdf.open()
for i in range(total_pages):
    page_pdf = os.path.join(temp_dir, f"page_{i}.pdf")
    adjusted_pdf = os.path.join(temp_dir, f"page_{i}_dates.pdf")
    print(f"Adjusting dates for page {i}...")
    subprocess.run([
        bf_exe, "adjust-dates", "-i", page_pdf, "-o", adjusted_pdf, "--mode", "shift-forward-31-month"
    ], check=True)
    adj_doc = pymupdf.open(adjusted_pdf)
    merged_dates_doc.insert_pdf(adj_doc)
    adj_doc.close()

merged_dates_doc.save(dates_pdf)
merged_dates_doc.close()

print("Splitting Page 12...")
doc = pymupdf.open(dates_pdf)
doc_p12 = pymupdf.open()
doc_p12.insert_pdf(doc, from_page=11, to_page=11)
doc_p12.save(p12_pdf)
doc_p12.close()

print("Running BankFidelity text on Page 12...")
subprocess.run([
    bf_exe, "text", "-i", p12_pdf, "-o", p12_edited_pdf,
    "--old", "Mr Peter Henry Hendel", "--new", "GEORGE GUTHRIE",
    "--bbox", "103.4,667.8,196.2,679.2", "-p", "0"
], check=True)

print("Merging Page 12 back into full document...")
edited_page = pymupdf.open(p12_edited_pdf)
doc.delete_page(11)
doc.insert_pdf(edited_page, from_page=0, to_page=0, start_at=11)
doc.save(final_pdf)
doc.close()
edited_page.close()

print("Running BankFidelity verify on final document...")
subprocess.run([
    bf_exe, "verify", "--original", in_pdf, "--edited", final_pdf, "--output-dir", verify_dir
], check=True)

print("E2E Verification Complete! Check exit code.")
