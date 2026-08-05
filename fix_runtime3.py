import re

with open("src/app/runtime.rs", "r") as f:
    content = f.read()

# Replace MindeeFinDoc with LlamaParse
content = content.replace("DocumentParserMode::MindeeFinDoc", "DocumentParserMode::LlamaParse")

# Remove the Mindee parser arm entirely
content = re.sub(r'\s+DocumentParserMode::LlamaParse => \{\n\s+let _ = res_tx\.send\(JobResult::Progress \{ label: "Parsing with Mindee\.\.\."\.into\(\), fraction: 0\.3 \}\);\n\s+match crate::ai::mindee::MindeeClient::from_app_config\(&cfg\) \{.*?(?=\s+DocumentParserMode::LlamaParse => \{)', '', content, flags=re.DOTALL)

with open("src/app/runtime.rs", "w") as f:
    f.write(content)
