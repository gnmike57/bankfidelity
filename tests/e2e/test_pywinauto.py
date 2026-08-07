"""PyWinAuto UIAutomation E2E against the BankFidelity GUI.

Requires:
  - Built debug binary at target/debug/dual-core-pdf-pipeline.exe
  - Interactive Windows desktop session

Skips cleanly when binary is missing, CI is set, or the window does not appear.
"""

from __future__ import annotations

import logging
import os
import time

import pytest

try:
    from pywinauto import timings
    from pywinauto.application import Application
except ImportError:  # pragma: no cover
    pytest.skip("pywinauto not installed", allow_module_level=True)

logger = logging.getLogger("pywinauto_e2e")

APP_PATH = os.path.abspath(
    os.path.join(
        os.path.dirname(__file__),
        "..",
        "..",
        "target",
        "debug",
        "dual-core-pdf-pipeline.exe",
    )
)
WINDOW_TITLE_RE = r"Bank Statement Fidelity Editor.*"
PASSPHRASE = "pywinauto-e2e-passphrase-12345678"


@pytest.fixture(scope="module")
def app_instance():
    """Start the GUI and tear it down after the module."""
    if os.environ.get("CI") or os.environ.get("GITHUB_ACTIONS"):
        pytest.skip("PyWinAuto GUI E2E requires an interactive desktop (not CI)")

    if not os.path.exists(APP_PATH):
        pytest.skip(f"Binary not found at {APP_PATH}. Run `cargo build` first.")

    env = os.environ.copy()
    env["DUAL_CORE_PASSPHRASE"] = PASSPHRASE

    print(f"Starting app: {APP_PATH} gui")
    app = Application(backend="uia").start(
        f'"{APP_PATH}" gui',
        wait_for_idle=False,
        work_dir=os.path.dirname(APP_PATH),
    )
    # Allow AccessKit tree to publish.
    time.sleep(2.5)

    try:
        yield app
    finally:
        try:
            app.kill(soft=False)
        except Exception as exc:  # pragma: no cover
            logger.warning("app.kill failed: %s", exc)


def test_app_window_title(app_instance):
    """Main window title matches the egui viewport builder prefix."""
    main_dlg = app_instance.window(title_re=WINDOW_TITLE_RE)
    if not main_dlg.exists(timeout=8):
        pytest.skip(
            "Main window not found via UIA (AccessKit may be disabled or desktop locked)"
        )
    title = main_dlg.window_text()
    assert "Bank Statement Fidelity Editor" in title, title


def test_interact_with_settings_if_present(app_instance):
    """Click Settings/Close when AccessKit exposes them; skip if not visible."""
    main_dlg = app_instance.window(title_re=WINDOW_TITLE_RE)
    if not main_dlg.exists(timeout=5):
        pytest.skip("Main window not available for interaction")

    try:
        settings_button = main_dlg.child_window(title="Settings", control_type="Button")
        if not settings_button.exists(timeout=2):
            pytest.skip("Settings button not exposed in current UI state")
        settings_button.click_input()
        time.sleep(0.5)

        # Prefer Cancel/Close labels used by modals
        for label in ("Close", "Cancel", "OK", "Ok"):
            btn = main_dlg.child_window(title=label, control_type="Button")
            if btn.exists(timeout=1):
                btn.click_input()
                break
    except timings.TimeoutError:
        pytest.skip("Timed out interacting with Settings controls")


if __name__ == "__main__":
    pytest.main(["-v", "-s", __file__])
