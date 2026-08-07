//! Optional desktop-session computer-use smoke.
//! Marked `#[ignore]` — requires an interactive display and a built binary.

use enigo::{Button, Coordinate, Enigo, Mouse, Settings};
use std::process::Command;
use std::time::Duration;

#[test]
#[ignore = "Requires active desktop session and compiled binary"]
fn test_computer_use_framework_bootstrap() {
    let bin_path = option_env!("CARGO_BIN_EXE_dual-core-pdf-pipeline")
        .unwrap_or("dual-core-pdf-pipeline");

    let mut child = Command::new(bin_path)
        .arg("gui")
        .env(
            "DUAL_CORE_PASSPHRASE",
            "computer-use-e2e-passphrase-12345678",
        )
        .spawn()
        .expect("Failed to start application binary");

    // Wait for the window to appear
    std::thread::sleep(Duration::from_secs(5));

    // Initialize Enigo for OS-level input (framework bootstrap only).
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.move_mouse(500, 500, Coordinate::Abs);
        let _ = enigo.button(Button::Left, enigo::Direction::Click);
    }

    let _ = child.kill();
    let _ = child.wait();
}
