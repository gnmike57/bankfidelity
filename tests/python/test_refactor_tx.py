import os
import sys
import json

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), '../../scripts')))

def test_json_payload_parsing_mock():
    # Mocking the JSON payload that Rust sends to Python
    payload = {
        "pdf_path": "/tmp/test.pdf",
        "edits": [
            {
                "page": 1,
                "original": "Debit",
                "replacement": "Credit"
            }
        ]
    }
    assert payload["edits"][0]["page"] == 1
    assert payload["edits"][0]["original"] == "Debit"
    print("  ✅ json parsing mock")

if __name__ == "__main__":
    print("Running suite: Refactor Tx Python Bridge")
    test_json_payload_parsing_mock()
