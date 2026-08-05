use assert_cmd::Command;
use predicates::prelude::*;

fn unconfigured_command() -> Command {
    let mut command = Command::cargo_bin("dual-core-pdf-pipeline").expect("binary must build");
    command
        .env_remove("DUAL_CORE_PASSPHRASE")
        .env_remove("SENTRY_DSN")
        .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
        .env_remove("GEMINI_API_KEY")
        .env_remove("PYMUPDF_PRO_KEY");
    command
}

#[test]
fn help_is_configuration_free_and_side_effect_free() {
    unconfigured_command()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Bank Statement Fidelity Editor CLI",
        ))
        .stderr(predicate::str::contains("Configuration Error").not())
        .stderr(predicate::str::contains("OTLP endpoint").not());
}

#[test]
fn version_is_configuration_free_and_side_effect_free() {
    unconfigured_command()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")))
        .stderr(predicate::str::contains("Configuration Error").not())
        .stderr(predicate::str::contains("OTLP endpoint").not());
}
