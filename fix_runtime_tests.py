import re

with open("src/app/runtime.rs", "r") as f:
    content = f.read()

content = re.sub(r'\s+mindee_api_key: None,', '', content)
content = re.sub(r'\s+assert!\(!availability\.mindee\);', '', content)
content = re.sub(r'\s+// when neither Document AI nor Mindee is configured\.', '', content)
content = re.sub(r'\s+assert!\(availability\.unavailable_reason\("mindee"\)\.is_some\(\)\);', '', content)

with open("src/app/runtime.rs", "w") as f:
    f.write(content)
