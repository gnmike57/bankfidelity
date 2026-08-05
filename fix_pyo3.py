import re

with open('src/ai/pyo3_bridge.rs', 'r') as f:
    content = f.read()

old_code = """                    // Use with_gil to create a new GILPool and prevent memory leaks
                    pyo3::Python::with_gil(f)"""

new_code = """                    match pyo3::Python::try_attach(|py| {
                        // Create an explicit GILPool to prevent memory leaks from runaway Python objects
                        // in this OS thread.
                        let _pool = unsafe { pyo3::marker::GILPool::new(py) };
                        f(py)
                    }) {
                        Some(res) => res,
                        None => Err("Failed to attach Python GIL".to_string()),
                    }"""

if old_code in content:
    content = content.replace(old_code, new_code)
    with open('src/ai/pyo3_bridge.rs', 'w') as f:
        f.write(content)
    print("Patched with_gil back to try_attach + GILPool")
else:
    print("Old code not found")
