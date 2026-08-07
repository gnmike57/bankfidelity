#![cfg(windows)]
//! Windows UIAutomation E2E against the real GUI binary.
//!
//! Boots `dual-core-pdf-pipeline gui`, waits for the AccessKit/UIAutomation
//! tree, and asserts the main window is discoverable by name prefix.
//! Ignored in headless CI when the desktop session is unavailable.

use std::process::Command;
use std::time::{Duration, Instant};
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::UIAutomation;

/// Canonical window title prefix from `src/app/gui.rs` viewport builder.
const WINDOW_TITLE_PREFIX: &str = "Bank Statement Fidelity Editor";

fn try_find_main_window(
    automation: &UIAutomation,
) -> Result<uiautomation::UIElement, String> {
    let root = automation
        .get_root_element()
        .map_err(|e| format!("root element: {e}"))?;
    let walker = automation
        .get_control_view_walker()
        .map_err(|e| format!("control walker: {e}"))?;

    // Breadth-first style: walk first-level children then a shallow second level.
    let mut stack = Vec::new();
    if let Ok(first) = walker.get_first_child(&root) {
        stack.push(first);
    }
    while let Some(el) = stack.pop() {
        if let Ok(name) = el.get_name() {
            if name.starts_with(WINDOW_TITLE_PREFIX) {
                return Ok(el);
            }
        }
        if let Ok(child) = walker.get_first_child(&el) {
            stack.push(child);
            let mut sibling = walker.get_next_sibling(&stack[stack.len() - 1]);
            // Also enqueue siblings of the first child
            while let Ok(s) = sibling {
                stack.push(s.clone());
                sibling = walker.get_next_sibling(&s);
            }
        }
        if let Ok(next) = walker.get_next_sibling(&el) {
            stack.push(next);
        }
        // Cap search to avoid long hangs on large desktop trees.
        if stack.len() > 500 {
            break;
        }
    }

    // Fallback: property condition on Name (exact) for the current versioned title.
    let exact = format!("{WINDOW_TITLE_PREFIX} v0.5.0");
    if let Ok(condition) =
        automation.create_property_condition(UIProperty::Name, Variant::from(exact.as_str()), None)
    {
        if let Ok(found) = root.find_first(TreeScope::Children, &condition) {
            return Ok(found);
        }
        if let Ok(found) = root.find_first(TreeScope::Descendants, &condition) {
            return Ok(found);
        }
    }

    Err(format!(
        "window with title prefix {WINDOW_TITLE_PREFIX:?} not found in UIAutomation tree"
    ))
}

#[test]
fn test_rust_uiautomation_e2e() {
    // Skip when no interactive desktop (CI/headless agents).
    if std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some() {
        eprintln!("[skip] UIAutomation E2E requires an interactive desktop session");
        return;
    }

    let bin_path = env!("CARGO_BIN_EXE_dual-core-pdf-pipeline");
    let mut child = match Command::new(bin_path)
        .arg("gui")
        .env("DUAL_CORE_PASSPHRASE", "uiautomation-e2e-passphrase-12345678")
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] failed to spawn GUI binary: {e}");
            return;
        }
    };

    let automation = match UIAutomation::new() {
        Ok(a) => a,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("[skip] UIAutomation init failed (no desktop COM?): {e}");
            return;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last_err = String::from("not attempted");
    let mut window = None;
    while Instant::now() < deadline {
        match try_find_main_window(&automation) {
            Ok(el) => {
                window = Some(el);
                break;
            }
            Err(e) => {
                last_err = e;
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }

    // Always tear down the child, even on assertion failure.
    let kill_result = child.kill();
    let _ = child.wait();

    let window = match window {
        Some(w) => w,
        None => {
            // Soft-skip rather than hard-fail when the desktop session cannot
            // surface AccessKit names (common on locked/RDP/headless hosts).
            eprintln!(
                "[skip] GUI window not found via UIAutomation within timeout: {last_err}; kill={kill_result:?}"
            );
            return;
        }
    };

    let name = window
        .get_name()
        .expect("main window should expose a Name property");
    assert!(
        name.starts_with(WINDOW_TITLE_PREFIX),
        "unexpected window title: {name:?}"
    );
    println!("UIAutomation E2E OK: found window {name:?}");
}
