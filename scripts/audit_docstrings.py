"""Audit triple-quote docstring positions in the integration module."""
import io

P = 'python/pymupdf_pro_integration.py'
with io.open(P, 'r', encoding='utf-8') as _f:
    src = _f.read()

# Find every position of triple-quote and report
positions = []
i = 0
while True:
    idx = src.find('"""', i)
    if idx < 0:
        break
    line_num = src[:idx].count('\n') + 1
    positions.append((line_num, idx))
    i = idx + 3

print(f'Total triple-quote tokens: {len(positions)}')
print(f'First 30:')
for ln, idx in positions[:30]:
    snippet = src[idx:idx+60].replace('\n', '\\n')
    print(f'  line {ln} idx {idx}: {snippet}')

print(f'\nLast 30:')
for ln, idx in positions[-30:]:
    snippet = src[idx:idx+60].replace('\n', '\\n')
    print(f'  line {ln} idx {idx}: {snippet}')

print(f'\nIs count even: {len(positions) % 2 == 0}')
