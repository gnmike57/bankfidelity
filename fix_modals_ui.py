import re

with open("src/app/modals.rs", "r") as f:
    content = f.read()

# Replace rows 1 through 4
old_start = '                        // ── 1. PDF Engine ──'
old_end = '                        // ── 5. Font Handling ──'

new_ui = """                        // ── Pipeline Architecture ──
                        ui.label("Extraction:");
                        ui.label("1. LlamaParse \\u{2192} 2. Offline Heuristic (93%)");
                        ui.end_row();

                        ui.label("Fidelity Edit:");
                        ui.label("1\\u{fe0f}\\u{20e3} PyMuPDF Pro (88%) \\u{2192} 2\\u{fe0f}\\u{20e3} Pdfium (76%) \\u{2192} 3\\u{fe0f}\\u{20e3} Typst Reconstruct (70%)");
                        ui.end_row();

                        ui.label("Math Balance:");
                        ui.label("1\\u{fe0f}\\u{20e3} Local Math Engine (100%)");
                        ui.end_row();

                        ui.label("Forensics:");
                        ui.label("1\\u{fe0f}\\u{20e3} PyMuPDF Pro (100%) \\u{2192} 2\\u{fe0f}\\u{20e3} Typst Reconstruct (90%)");
                        ui.end_row();

                        ui.label("Visual AI Validation:");
                        ui.label("new system");
                        ui.end_row();

                        // ── 5. Font Handling ──"""

idx_start = content.find(old_start)
idx_end = content.find(old_end)

if idx_start != -1 and idx_end != -1:
    content = content[:idx_start] + new_ui + content[idx_end + len(old_end):]
    with open("src/app/modals.rs", "w") as f:
        f.write(content)
else:
    print("Could not find blocks")
