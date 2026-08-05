import re

with open('src/ai/pyo3_bridge.rs', 'r') as f:
    content = f.read()

old_code = """    fn safe_python_with_gil<F, T>(f: F) -> Result<T, String>
    where
        F: FnOnce(Python<'_>) -> Result<T, String> + Send,
        T: Send,
    {
        let result = std::thread::scope(|s| {
            let handle = s.spawn(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    match pyo3::Python::try_attach(|py| {
                        // Create an explicit GILPool to prevent memory leaks from runaway Python objects
                        // in this OS thread.
                        let _pool = unsafe { pyo3::marker::GILPool::new(py) };
                        f(py)
                    }) {
                        Some(res) => res,
                        None => Err("Failed to attach Python GIL".to_string()),
                    }
                }))
            });
            handle.join()
        });

        match result {
            Ok(Ok(Ok(res))) => Ok(res),         // Inner Python execution succeeded
            Ok(Ok(Err(py_err))) => Err(py_err), // Inner Python execution failed gracefully
            Ok(Err(panic_err)) => {
                // Python execution panicked
                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic type".to_string()
                };
                Err(format!("Python panic: {}", msg))
            }
            Err(join_err) => Err(format!("Python thread join error: {:?}", join_err)),
        }
    }"""

new_code = """    fn safe_python_with_gil<F, T>(f: F) -> Result<T, String>
    where
        F: FnOnce(Python<'_>) -> Result<T, String> + Send,
        T: Send,
    {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            match pyo3::Python::try_attach(f) {
                Some(res) => res,
                None => Err("Failed to attach Python GIL".to_string()),
            }
        }));

        match result {
            Ok(Ok(res)) => Ok(res),         // Inner Python execution succeeded
            Ok(Err(py_err)) => Err(py_err), // Inner Python execution failed gracefully
            Err(panic_err) => {
                // Python execution panicked
                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic type".to_string()
                };
                Err(format!("Python panic: {}", msg))
            }
        }
    }"""

if old_code in content:
    content = content.replace(old_code, new_code)
    with open('src/ai/pyo3_bridge.rs', 'w') as f:
        f.write(content)
    print("Patched safe_python_with_gil to prevent thread leak")
else:
    print("Old code not found")
