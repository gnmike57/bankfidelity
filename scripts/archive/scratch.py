import re

with open('src/ai/llamaparse.rs', 'r') as f:
    content = f.read()

idx = content.find('if transactions.is_empty() {')
print(content[idx:idx+800])
