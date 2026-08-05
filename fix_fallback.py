import re

with open("src/engine/interactive_fallback.rs", "r") as f:
    content = f.read()

content = re.sub(r'\s+InteractiveChoice \{\s+id: "mindee".*?\},', '', content, flags=re.DOTALL)

with open("src/engine/interactive_fallback.rs", "w") as f:
    f.write(content)
