import re

with open('src/ai/document_ai.rs', 'r') as f:
    content = f.read()

# Find the old functions
old_start = content.find('fn extract_string_property(entity: &serde_json::Value')

if old_start != -1:
    content = content[:old_start]
    with open('src/ai/document_ai.rs', 'w') as f:
        f.write(content)
        print("Removed old functions")
else:
    print("Old functions not found!")

