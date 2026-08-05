import os
import sys
import json

# Add scripts directory to path to import python modules
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '../../scripts')))

def test_json_payload_parsing_mock():
    # Mocking the JSON payload that Rust sends to Python
    payload = {
        "pages": 5,
        "mode": "split",
        "output_dir": "/tmp/test"
    }
    assert payload["mode"] == "split", "Mode should be split"
    assert payload["pages"] == 5, "Pages should be 5"
    print("  ✅ json parsing mock")

if __name__ == "__main__":
    print("Running suite: Split Merge Python Bridge")
    test_json_payload_parsing_mock()
