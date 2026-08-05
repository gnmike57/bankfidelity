import re

with open("src/app/runtime.rs", "r") as f:
    content = f.read()

content = content.replace("DocumentParserMode::PyMuPdfBuiltin", "DocumentParserMode::OfflineHeuristic")

with open("src/app/runtime.rs", "w") as f:
    f.write(content)
