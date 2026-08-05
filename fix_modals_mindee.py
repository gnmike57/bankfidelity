import re

with open("src/app/modals.rs", "r") as f:
    content = f.read()

content = re.sub(r'\s+ui\.small\(format!\("Mindee \{\}", mark\(self\.api_availability\.mindee\)\)\);', '', content)

with open("src/app/modals.rs", "w") as f:
    f.write(content)
