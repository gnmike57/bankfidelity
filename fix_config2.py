import re

with open("src/app/config.rs", "r") as f:
    content = f.read()

# I previously removed PyMuPdfBuiltin, so let's add OfflineHeuristic after LlamaParse
content = content.replace("    LlamaParse,", "    LlamaParse,\n    /// Pure Rust heuristic parsing (regex + layout), highly accurate for standard banking formats.\n    OfflineHeuristic,")

content = content.replace("            Self::LlamaParse => \"LlamaParse\",", "            Self::LlamaParse => \"LlamaParse\",\n            Self::OfflineHeuristic => \"Offline Heuristic\",")

with open("src/app/config.rs", "w") as f:
    f.write(content)
