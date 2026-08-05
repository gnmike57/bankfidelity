import re
import os

paths = ["src/app/gui.rs", "src/app/modals.rs"]
buttons_to_jobs = []

for path in paths:
    with open(path, "r") as f:
        content = f.read()
    
    # We are looking for lines with ui.button("...").clicked() { ... job_tx.send(Job::XYZ) }
    # Since they might span multiple lines, let's just do a simpler search:
    # Find all button labels
    button_matches = re.finditer(r'\.(button|menu_button)\s*\(\s*"([^"]+)"\s*\)', content)
    for match in button_matches:
        buttons_to_jobs.append(match.group(2))

# Deduplicate
buttons_to_jobs = sorted(list(set(buttons_to_jobs)))
for b in buttons_to_jobs:
    print(b)
