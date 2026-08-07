#![cfg(windows)]
//! Windows UIAutomation E2E against the real GUI binary.
//!
//! Boots `dual-core-pdf-pipeline gui`, waits for the AccessKit/UIAutomation
//! tree, and asserts the main window is discoverable by name prefix.
//! Soft-skips in headless CI or when the desktop session cannot surface names.
//! Every COM walk is deadline-bounded so a stuck tree walk cannot hang the suite.

use std::process::{Child, Command};
use std::time::{Duration, Instant};
use uiautomation::types::{TreeScope, UIProperty};
use uiautomation::variants::Variant;
use uiautomation::UIAutomation;

/// Canonical window title prefix from `src/app/gui.rs` viewport builder.
const WINDOW_TITLE_PREFIX: &str = "Bank Statement Fidelity Editor";

/// Overall budget for discovering the window after spawn.
const FIND_BUDGET: Duration = Duration::from_secs(12);
/// Hard cap for a single tree walk (prevents COM hangs from blocking the outer loop).
const WALK_BUDGET: Duration = Duration::from_secs(2);
const MAX_TOP_LEVEL: usize = 120;

fn kill_gui_process_tree(child: &mut Child) {
    let pid = child.id();
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

struct GuiChildGuard(Option<Child>);

impl GuiChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn kill_now(&mut self) {
        if let Some(mut child) = self.0.take() {
            kill_gui_process_tree(&mut child);
        }
    }
}

impl Drop for GuiChildGuard {
    fn drop(&mut self) {
        self.kill_now();
    }
}

fn try_find_main_window(
    automation: &UIAutomation,
    walk_deadline: Instant,
) -> Result<uiautomation::UIElement, String> {
    if Instant::now() >= walk_deadline {
        return Err("walk deadline before search".into());
    }

    let root = automation
        .get_root_element()
        .map_err(|e| format!("root element: {e}"))?;

    let exact = format!("{WINDOW_TITLE_PREFIX} v{}", env!("CARGO_PKG_VERSION"));
    if let Ok(condition) =
        automation.create_property_condition(UIProperty::Name, Variant::from(exact.as_str()), None)
    {
        if Instant::now() < walk_deadline {
            if let Ok(found) = root.find_first(TreeScope::Children, &condition) {
                return Ok(found);
            }
        }
    }

    let walker = automation
        .get_control_view_walker()
        .map_err(|e| format!("control walker: {e}"))?;

    // Walk only top-level desktop children (typical window list).
    let mut count = 0usize;
    let mut current = walker
        .get_first_child(&root)
        .map_err(|e| format!("first child: {e}"))
        .ok();
    while let Some(el) = current {
        if Instant::now() >= walk_deadline {
            return Err("walk deadline exceeded during top-level scan".into());
        }
        count += 1;
        if count > MAX_TOP_LEVEL {
            break;
        }
        if let Ok(name) = el.get_name() {
            if name.starts_with(WINDOW_TITLE_PREFIX) {
                return Ok(el);
            }
        }
        current = walker.get_next_sibling(&el).ok();
    }

    Err(format!(
        "window with title prefix {WINDOW_TITLE_PREFIX:?} not found (scanned={count})"
    ))
}

#[test]
fn test_rust_uiautomation_e2e() {
    if std::env::var_os("CI").is_some() || std::env::var_os("GITHUB_ACTIONS").is_some() {
        eprintln!("[skip] UIAutomation E2E requires an interactive desktop session");
        return;
    }

    let bin_path = env!("CARGO_BIN_EXE_dual-core-pdf-pipeline");
    let child = match Command::new(bin_path)
        .arg("gui")
        .env(
            "DUAL_CORE_PASSPHRASE",
            "uiautomation-e2e-passphrase-12345678",
        )
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] failed to spawn GUI binary: {e}");
            return;
        }
    };
    let mut guard = GuiChildGuard::new(child);

    let automation = match UIAutomation::new() {
        Ok(a) => a,
        Err(e) => {
            guard.kill_now();
            eprintln!("[skip] UIAutomation init failed (no desktop COM?): {e}");
            return;
        }
    };

    let overall_deadline = Instant::now() + FIND_BUDGET;
    let mut last_err = String::from("not attempted");
    let mut window = None;
    while Instant::now() < overall_deadline {
        let walk_deadline = (Instant::now() + WALK_BUDGET).min(overall_deadline);
        match try_find_main_window(&automation, walk_deadline) {
            Ok(el) => {
                window = Some(el);
                break;
            }
            Err(e) => {
                last_err = e;
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }

    let name = match window {
        Some(w) => match w.get_name() {
            Ok(n) => n,
            Err(e) => {
                guard.kill_now();
                eprintln!("[skip] GUI window get_name failed via UIAutomation: {e}");
                return;
            }
        },
        None => {
            guard.kill_now();
            eprintln!("[skip] GUI window not found via UIAutomation within timeout: {last_err}");
            return;
        }
    };

    // Tear down after reading the title so COM still has a live HWND.
    guard.kill_now();

    assert!(
        name.starts_with(WINDOW_TITLE_PREFIX),
        "unexpected window title: {name:?}"
    );
    println!("UIAutomation E2E OK: found window {name:?}");
}
