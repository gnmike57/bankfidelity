"""State-machine apostrophe fixer with diagnostic output."""
import io

P = 'python/pymupdf_pro_integration.py'
src = io.open(P, 'r', encoding='utf-8').read()

print(f'Read {len(src)} chars')
print(f'ASCII apostrophes in source: {src.count(chr(0x27))}')

OUT = []
i = 0
n = len(src)
state = 'CODE'
replacements = 0

while i < n:
    c = src[i]
    if state == 'CODE':
        if src[i:i+3] == '"""':
            OUT.append('"""')
            i += 3
            state = 'STRING_TRIPLE_DQUOTE'
            continue
        if src[i:i+3] == "'''":
            OUT.append("'''")
            i += 3
            state = 'STRING_TRIPLE_SQUOTE'
            continue
        if c == '"':
            OUT.append('"')
            i += 1
            state = 'STRING_DQUOTE'
            continue
        if c == "'":
            OUT.append("'")
            i += 1
            state = 'STRING_SQUOTE'
            continue
        if c == '#':
            OUT.append('#')
            i += 1
            state = 'COMMENT'
            continue
        OUT.append(c)
        i += 1
    elif state == 'STRING_TRIPLE_DQUOTE':
        if src[i:i+3] == '"""':
            OUT.append('"""')
            i += 3
            state = 'CODE'
            continue
        if c == "'":
            OUT.append('\u2019')
            i += 1
            replacements += 1
            continue
        OUT.append(c)
        i += 1
    elif state == 'STRING_TRIPLE_SQUOTE':
        if src[i:i+3] == "'''":
            OUT.append("'''")
            i += 3
            state = 'CODE'
            continue
        OUT.append(c)
        i += 1
    elif state == 'STRING_DQUOTE':
        if c == '\\' and i + 1 < n:
            OUT.append(src[i:i+2])
            i += 2
            continue
        if c == '"':
            OUT.append('"')
            i += 1
            state = 'CODE'
            continue
        if c == '\n':
            OUT.append(c)
            i += 1
            state = 'CODE'
            continue
        OUT.append(c)
        i += 1
    elif state == 'STRING_SQUOTE':
        if c == '\\' and i + 1 < n:
            OUT.append(src[i:i+2])
            i += 2
            continue
        if c == "'":
            OUT.append("'")
            i += 1
            state = 'CODE'
            continue
        if c == '\n':
            OUT.append(c)
            i += 1
            state = 'CODE'
            continue
        OUT.append(c)
        i += 1
    elif state == 'COMMENT':
        if c == '\n':
            state = 'CODE'
        OUT.append(c)
        i += 1

new_src = ''.join(OUT)
print(f'Replacements made: {replacements}')
print(f'Output length: {len(new_src)}')
print(f'ASCII apostrophes in output: {new_src.count(chr(0x27))}')
print(f'Unicode 2019 in output: {new_src.count(chr(0x2019))}')

# Write with explicit binary mode to avoid newline conversion
with io.open(P, 'wb') as f:
    f.write(new_src.encode('utf-8'))
print('written')
