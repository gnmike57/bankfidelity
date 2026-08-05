import re

with open('src/ai/document_ai.rs', 'r') as f:
    content = f.read()

# Fix the bbox type error
content = content.replace("let row_bbox = row_bbox.unwrap_or_default();\n", "")

# Fix the duplicate functions at the end of the file
# We will just find where the first `fn extract_string_property` (the one I inserted) is.
first_idx = content.find('fn extract_string_property')

# Find the second `fn extract_string_property`
second_idx = content.find('fn extract_string_property', first_idx + 1)
if second_idx != -1:
    print("Found old functions starting at", second_idx)
    content = content[:second_idx]

# Wait, `bbox_from_bounding_poly` is redefined.
# The original helper functions must be fully removed. Let's find all the old ones.
# The end of the file should just be the end of `fn property_bbox`.
# Let's find the last `fn property_bbox` and keep only up to its end brace.
idx = content.rfind('fn property_bbox')
if idx != -1:
    # find its closing brace
    braces = 0
    in_block = False
    end_idx = -1
    for i in range(idx, len(content)):
        if content[i] == '{':
            braces += 1
            in_block = True
        elif content[i] == '}':
            braces -= 1
        
        if in_block and braces == 0:
            end_idx = i + 1
            break
    
    if end_idx != -1:
        # Check if there is another property_bbox before this
        prev_idx = content.find('fn property_bbox')
        if prev_idx != idx:
            # We have duplicates! The first one is the new one, the second is the old one.
            # We want to remove everything from the second `fn extract_string_property` (or where the old ones start)
            # The old functions start with `fn extract_string_property(entity: &serde_json::Value`
            old_start = content.find('fn extract_string_property(entity: &serde_json::Value')
            if old_start != -1:
                content = content[:old_start]

with open('src/ai/document_ai.rs', 'w') as f:
    f.write(content)

