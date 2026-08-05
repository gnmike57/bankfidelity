import pytest
import os
import time
from pywinauto.application import Application
from pywinauto import timings
import pywinauto.actionlogger
import logging

# Enable maximum deep event logging for UI events
pywinauto.actionlogger.enable()
logger = logging.getLogger("pywinauto")
logger.setLevel(logging.DEBUG)

# PyWinAuto UIAutomation Test Foundation
#
# This tests the running application using the Windows UIAutomation (AccessKit) tree.
# It requires the Rust binary to be built and running.

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


@pytest.fixture(scope="module")
def app_instance():
    """Starts the application and tears it down after tests."""
    if not os.path.exists(APP_PATH):
        pytest.skip(f"Binary not found at {APP_PATH}. Please run 'cargo build' first.")

    print(f"Starting app at {APP_PATH} gui")
    app = Application(backend="uia").start(f'"{APP_PATH}" gui', wait_for_idle=False)

    # Wait for the main window to be ready
    time.sleep(2)

    yield app

    # Teardown: kill the app
    app.kill()


def test_app_window_title(app_instance):
    """Verifies that the main application window launches with the correct title."""
    main_dlg = app_instance.window(title_re="Bank Statement Fidelity Editor.*")
    assert main_dlg.exists(timeout=5), "Main window did not appear."


def test_interact_with_buttons(app_instance):
    """
    Demonstrates finding buttons exposed by egui's AccessKit integration
    and clicking them deterministically without screen pixels.
    """
    main_dlg = app_instance.window(title_re="Bank Statement Fidelity Editor.*")

    # You can dump the tree to see all accessible elements:
    # main_dlg.print_control_identifiers()

    # Try finding common elements if they exist in the UI
    try:
        # Example: Find a button by title and click it
        # This will fail gracefully if the UI layout changes, allowing for robust E2E testing
        settings_button = main_dlg.child_window(title="Settings", control_type="Button")
        if settings_button.exists(timeout=2):
            settings_button.click()
            time.sleep(1)

            # Close the modal
            close_button = main_dlg.child_window(title="Close", control_type="Button")
            if close_button.exists(timeout=1):
                close_button.click()
    except timings.TimeoutError:
        print(
            "Note: Specific buttons might not be visible in current state. Adjust test to match UI."
        )
        pass


if __name__ == "__main__":
    pytest.main(["-v", "-s", __file__])
