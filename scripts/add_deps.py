"""Add Rust crate dependencies to Cargo.toml."""

with open('Cargo.toml', 'r', encoding='utf-8') as f:
    content = f.read()

content = content.replace('[dependencies]', '''[dependencies]
pyo3 = { version = "0.20.0", features = ["auto-initialize"] }
pdfium-render = "0.8.21"
mupdf = "0.7.0"
''')

with open('Cargo.toml', 'w', encoding='utf-8') as f:
    f.write(content)
print("Updated Cargo.toml")

