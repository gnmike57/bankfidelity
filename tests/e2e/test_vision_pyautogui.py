import pytest
import os
import time
import subprocess
import pyautogui
import pytesseract
import shutil

# OCR Visual Testing Foundation
#
# This tests the running application using purely visual pixel-reading via OCR.
# It requires the Rust binary to be built and running.
# IMPORTANT: Tesseract OCR MUST be installed on the system (e.g. winget install UB-Mannheim.TesseractOCR)

# Configure tesseract path if it's installed in the default winget location
TESSERACT_PATH = r"C:\Program Files\Tesseract-OCR\tesseract.exe"
if os.path.exists(TESSERACT_PATH):
    pytesseract.pytesseract.tesseract_cmd = TESSERACT_PATH

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

    if not shutil.which("tesseract") and not os.path.exists(TESSERACT_PATH):
        pytest.skip(
            "Tesseract-OCR is not installed. Please install it (winget install UB-Mannheim.TesseractOCR)."
        )

    print(f"Starting app at {APP_PATH}")
    process = subprocess.Popen([APP_PATH, "gui"])

    # Wait for window to render completely
    time.sleep(4)

    yield process

    process.terminate()
    process.wait()


def find_and_click_text(target_text: str) -> bool:
    """Takes a screenshot, extracts bounding boxes for all text, and clicks the target_text."""
    # 1. Take a full-screen screenshot
    screenshot = pyautogui.screenshot()

    # 2. Extract detailed data including bounding boxes
    # Output format is a dict with keys: 'level', 'page_num', 'block_num', 'par_num', 'line_num', 'word_num', 'left', 'top', 'width', 'height', 'conf', 'text'
    data = pytesseract.image_to_data(screenshot, output_type=pytesseract.Output.DICT)

    # 3. Find the text
    for i in range(len(data["text"])):
        text = data["text"][i].strip()
        if target_text.lower() in text.lower():
            # Extract coordinates
            x = data["left"][i]
            y = data["top"][i]
            w = data["width"][i]
            h = data["height"][i]

            # Calculate the exact center of the word
            center_x = x + w / 2
            center_y = y + h / 2

            print(
                f"Found '{text}' at ({center_x}, {center_y}) with {data['conf'][i]}% confidence."
            )

            # 4. Physically move and click
            pyautogui.moveTo(center_x, center_y, duration=0.5)
            pyautogui.click()
            return True

    return False


def test_visual_ocr_button_click(app_instance):
    """
    Reads the screen using OCR, finds the 'Settings' label, and clicks it.
    """
    # Look for the word "Settings" on the screen and click it
    found = find_and_click_text("Settings")

    if not found:
        # Fallback to looking for something else if Settings is hidden
        found = find_and_click_text("Bank")

    assert found, "Could not locate the target text anywhere on the screen via OCR."

    # If clicked Settings, we should be able to find and click "Close"
    time.sleep(1)
    find_and_click_text("Close")


if __name__ == "__main__":
    pytest.main(["-v", "-s", __file__])
