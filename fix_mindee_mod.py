import re

with open("src/ai/mod.rs", "r") as f:
    content = f.read()

content = content.replace("pub mod mindee;\n", "")

with open("src/ai/mod.rs", "w") as f:
    f.write(content)
