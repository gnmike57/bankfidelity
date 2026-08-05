import os
import re

PATTERNS = {
    "LlamaParse": re.compile(r"llx-[a-zA-Z0-9_]{25,}"),
    "Gemini": re.compile(r"AIza[a-zA-Z0-9_\-]{25,}"),
    "Groq": re.compile(r"gsk_[a-zA-Z0-9]{20,}"),
    "OpenRouter": re.compile(r"sk-or-v1-[a-zA-Z0-9]{25,}"),
    "PyMuPDF_Pro": re.compile(r"hFKt[a-zA-Z0-9]{20,}"),
    "Mindee": re.compile(r"md_[a-zA-Z0-9_]{20,}")
}

def scan_file(filepath):
    try:
        with open(filepath, 'r', encoding='utf-8', errors='ignore') as f:
            content = f.read()
            for name, pattern in PATTERNS.items():
                for match in pattern.findall(content):
                    print(f"[{name}] found in {filepath}: {match}")
    except Exception as e:
        pass

for root, dirs, files in os.walk("."):
    if ".git" in root or "node_modules" in root or "target" in root or ".venv" in root:
        continue
    for file in files:
        if file.endswith((".py", ".rs", ".md", ".json", ".txt", ".env", ".toml", ".yml", ".yaml", "example", ".ps1")):
            scan_file(os.path.join(root, file))
