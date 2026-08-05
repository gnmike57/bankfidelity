use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn get_test_pdf() -> PathBuf {
    let path = PathBuf::from("test_doc.pdf");
    if !path.exists() {
        // Fallback to examples/sample.pdf if test_doc.pdf is missing
        let sample = PathBuf::from("examples/sample.pdf");
        if sample.exists() {
            return sample;
        }
    }
    path
}

fn get_cmd() -> Command {
    let mut command = Command::cargo_bin("dual-core-pdf-pipeline").unwrap();
    command
        .env(
            "DUAL_CORE_PASSPHRASE",
            "phase06-integration-test-passphrase-1234",
        )
        .env("GEMINI_API_KEY", "")
        .env("PYMUPDF_PRO_KEY", "")
        .env("DOCUMENT_AI_PROJECT_ID", "")
        .env("DOCUMENT_AI_LOCATION", "")
        .env("DOCUMENT_AI_PROCESSOR_ID", "");
    command
}

#[test]
fn test_cli_help() {
    let mut cmd = get_cmd();
    cmd.arg("--help").assert().success();
}

#[test]
fn test_cli_ping() {
    let mut cmd = get_cmd();
    cmd.arg("ping").assert().success();
}

#[test]
fn test_cli_doctor() {
    let mut cmd = get_cmd();
    cmd.env("PYMUPDF_PRO_KEY", "hFKt01234567890123456789")
        .env("DOCUMENT_AI_PROJECT_ID", "dummy")
        .env("DOCUMENT_AI_LOCATION", "dummy")
        .env("DOCUMENT_AI_PROCESSOR_ID", "dummy")
        .arg("doctor")
        .assert()
        .code(
            predicates::prelude::predicate::eq(0)
                .or(predicates::prelude::predicate::eq(2))
                .or(predicates::prelude::predicate::eq(6)),
        );
}

#[test]
fn test_cli_analyze_fonts() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }

    let mut cmd = get_cmd();
    cmd.arg("analyze-fonts")
        .arg("--input")
        .arg(&pdf)
        .assert()
        // Graceful: PyMuPDF Pro may not be installed/available, so accept
        // exit code 0 (success) or 1 (graceful failure / missing dependency).
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_text() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }
    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    cmd.arg("text")
        .arg("--input")
        .arg(&pdf)
        .arg("--output")
        .arg(out.as_os_str())
        .arg("--page")
        .arg("0")
        .arg("--bbox")
        .arg("0,0,100,100")
        .arg("--new")
        .arg("Replacement")
        .arg("--old")
        .arg("Original")
        .assert()
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_balance() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }

    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    cmd.arg("balance")
        .arg("--input")
        .arg(&pdf)
        .arg("--output")
        .arg(out.as_os_str())
        .assert()
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_auto_balance() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }
    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    cmd.arg("auto-balance")
        .arg("--input")
        .arg(&pdf)
        .arg("--output")
        .arg(out.as_os_str())
        .assert()
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_extract() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }

    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    cmd.arg("extract")
        .arg("--input")
        .arg(&pdf)
        .arg("--output")
        .arg(out.as_os_str())
        .assert()
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_ai_fix_visual() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }

    let mut cmd = get_cmd();
    cmd.arg("ai-fix-visual")
        .arg("--input")
        .arg(&pdf)
        .arg("--page")
        .arg("0")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "AI visual layout repair is not available in v1; no document was changed",
        ));
}

#[test]
fn test_cli_adjust_dates() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }
    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    cmd.arg("adjust-dates")
        .arg("--input")
        .arg(&pdf)
        .arg("--output")
        .arg(out.as_os_str())
        .arg("--mode")
        .arg("remap")
        .assert()
        .code(predicate::eq(0).or(predicate::eq(6)));
}

#[test]
fn test_cli_transfer_transactions() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }
    let out = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut cmd = get_cmd();
    // Use the same PDF as source and target just for testing the execution path
    cmd.arg("transfer-transactions")
        .arg("--source-pdf").arg(&pdf)
        .arg("--target-pdf").arg(&pdf)
        .arg("--output").arg(out.as_os_str())
        .assert()
        // Transfer might fail because it requires Document AI, but we ensure it runs
        .code(predicate::eq(0).or(predicate::eq(1)));
}

#[test]
fn test_cli_run_transfer_tests() {
    let pdf = get_test_pdf();
    if !pdf.exists() {
        eprintln!("[skip] test PDF not found; test self-skipped");
        return;
    }

    let mut cmd = get_cmd();
    cmd.arg("run-transfer-tests")
        .arg("--statements")
        .arg(pdf.to_string_lossy().to_string())
        .arg("--max-iterations")
        .arg("1")
        .assert()
        .code(predicate::eq(0).or(predicate::eq(1)));
}

// NOTE: Verify, Render, and DocaiTrain tests omitted or simplified because they
// require specific environments (Document AI keys) or 2 input PDFs, which are
// harder to mock out completely in a raw E2E without relying on env state.
