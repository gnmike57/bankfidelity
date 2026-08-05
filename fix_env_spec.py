import re

with open("src/app/env_spec.rs", "r") as f:
    content = f.read()

content = re.sub(r'\s+EnvVarSpec \{\s+name: "MINDEE_API_KEY".*?\},', '', content, flags=re.DOTALL)
content = re.sub(r'\s+EnvVarSpec \{\s+name: "MINDEE_MODEL_ID".*?\},', '', content, flags=re.DOTALL)

with open("src/app/env_spec.rs", "w") as f:
    f.write(content)
