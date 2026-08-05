import re

with open("src/engine/model.rs", "r") as f:
    content = f.read()

content = re.sub(r'/// Extracted via the Mindee Financial Document API\.\s+Mindee\s+\{.*?\},', '', content, flags=re.DOTALL)
content = re.sub(r'pub mindee_wins: usize,', '', content)

with open("src/engine/model.rs", "w") as f:
    f.write(content)
