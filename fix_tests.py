import re

with open("tests/setting_combinations.rs", "r") as f:
    content = f.read()

content = content.replace("    DocumentParserMode::MindeeFinDoc,\n", "")

with open("tests/setting_combinations.rs", "w") as f:
    f.write(content)

with open("tests/dependency_e2e.rs", "r") as f:
    content = f.read()

content = content.replace('        "api.mindee.net:443", // Mindee\n', '')

with open("tests/dependency_e2e.rs", "w") as f:
    f.write(content)

with open("tests/fallback_integration_tests.rs", "r") as f:
    content = f.read()

content = re.sub(r'// We cannot easily mock the Mindee client without modifying the source to accept a base URL override\.\n', '', content)
content = re.sub(r'// when Document AI or Mindee returns an error, ensuring that `parse_statement_offline` gracefully takes over\.\n', '', content)
content = re.sub(r'    // Simulate Mindee returning an error\n    let _fake_mindee_error = anyhow::anyhow!\("Mindee API Error: 500 Internal Server Error"\);\n', '', content)

with open("tests/fallback_integration_tests.rs", "w") as f:
    f.write(content)
