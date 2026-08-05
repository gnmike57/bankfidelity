use enigo::{Button, Coordinate, Enigo, Mouse, Settings};
use std::process::Command;
use std::time::Duration;

#[test]
#[ignore = "Requires active desktop session and compiled binary"]
fn test_computer_use_framework_bootstrap() {
    // 1. Launch the binary
    let mut child = Command::new("cargo")
        .args(["run", "--bin", "dual-core-pdf-pipeline"])
        .spawn()
        .expect("Failed to start application");

    // Wait for the window to appear
    std::thread::sleep(Duration::from_secs(5));

    // 2. Initialize Enigo for OS-level input
    let mut enigo = Enigo::new(&Settings::default()).unwrap();

    // 3. Simulate OS-level interaction (e.g. click in the center of the screen)
    // Note: In a real E2E test we would use coordinate mapping or image recognition.
    let _ = enigo.move_mouse(500, 500, Coordinate::Abs);
    let _ = enigo.button(Button::Left, enigo::Direction::Click);

    // Clean up
    child.kill().unwrap();
    let _ = child.wait();
}
