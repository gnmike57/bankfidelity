import re

with open("src/app/runtime.rs", "r") as f:
    content = f.read()

content = content.replace("DocumentParserMode::PyMuPdfBuiltin", "DocumentParserMode::OfflineHeuristic")
content = content.replace("DocumentParserMode::MindeeFinDoc", "DocumentParserMode::LlamaParse")

with open("src/app/runtime.rs", "w") as f:
    f.write(content)
