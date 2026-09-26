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
/// dead fork lived in `src/app/runtime/` (core.rs, client.rs, jobs.rs,
/// python_job.rs, tracking.rs) as never-compiled duplicate code that kept
/// being mistaken for the real implementation. It was removed.
///
/// Today the directory may exist ONLY for submodules that
/// `src/app/runtime.rs` explicitly declares (e.g. `mod parser_chain;`).
/// Any `.rs` file in that directory without a matching declaration is dead
/// code by construction — this test fails the suite if one appears.
#[test]
fn test_zombie_runtime_fork_files_are_declared_or_absent() {
    let runtime_dir = Path::new("src/app/runtime");
    if !runtime_dir.exists() {
        return; // No submodules at all is fine.
    }
    let runtime_rs =
        fs::read_to_string("src/app/runtime.rs").expect("src/app/runtime.rs must exist");
    let declares = |stem: &str| {
        runtime_rs.lines().any(|line| {
            let trimmed = line.trim();
            trimmed == format!("mod {stem};")
                || trimmed == format!("pub mod {stem};")
                || trimmed == format!("pub(crate) mod {stem};")
        })
    };
    let mut undeclared = Vec::new();
    for entry in fs::read_dir(runtime_dir).expect("read src/app/runtime dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let stem = path
                .file_stem()
                .expect("file stem")
                .to_string_lossy()
                .to_string();
            if !declares(&stem) {
                undeclared.push(path.display().to_string());
            }
        }
    }
    assert!(
        undeclared.is_empty(),
        "ZOMBIE FORK DETECTED: undeclared .rs files under src/app/runtime/ \
         are dead code that is never compiled:\n{}",
        undeclared.join("\n")
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
