#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::fs;
use std::path::Path;

#[test]
fn test_no_pymupdf_in_split_merge() {
    let path = "src/engine/pdf_split_merge.rs";
    let content = fs::read_to_string(path).expect("Failed to read split_merge module");

    let restricted = ["pymupdf", "pyo3", "fitz", "pro.unlock", "Python"];

    for word in restricted {
        if content.to_lowercase().contains(&word.to_lowercase()) {
            panic!("Subsystem A (split_merge) MUST NOT use PyMuPDF or PyO3. Found restricted word: '{word}'");
        }
    }
}

/// Guardrail against the "zombie fork" regression.
///
/// `src/app/runtime.rs` is the single live runtime module. Historically, a
/// duplicate `src/app/runtime/` directory (core.rs, client.rs, jobs.rs,
/// python_job.rs, tracking.rs) existed alongside it as dead, never-compiled
/// code that kept being mistaken for the real implementation. It was removed;
/// this test fails the suite if anyone reintroduces it.
#[test]
fn test_zombie_runtime_fork_directory_is_gone() {
    let zombie_dir = Path::new("src/app/runtime");
    assert!(
        !zombie_dir.exists(),
        "ZOMBIE FORK DETECTED: 'src/app/runtime/' must not exist. \
         'src/app/runtime.rs' is the single source of truth for the runtime. \
         If you need new runtime code, extend 'src/app/runtime.rs' or create a \
         properly declared module — never an undeclared sibling directory."
    );
}

/// Guardrail: no module may be force-routed into a runtime directory fork via
/// `#[path = "..."]` attributes anywhere under `src/`.
#[test]
fn test_no_path_attribute_resurrection_of_runtime_fork() {
    fn visit(dir: &Path, findings: &mut Vec<String>) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, findings);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    for line in content.lines() {
                        let trimmed = line.trim_start();
                        if trimmed.starts_with("#[path") && trimmed.contains("runtime") {
                            findings.push(format!("{}: {}", path.display(), trimmed));
                        }
                    }
                }
            }
        }
    }

    let mut findings = Vec::new();
    visit(Path::new("src"), &mut findings);
    assert!(
        findings.is_empty(),
        "ZOMBIE FORK RESURRECTION VIA #[path]: found runtime-directed path attributes:\n{}",
        findings.join("\n")
    );
}
