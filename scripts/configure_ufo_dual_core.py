#!/usr/bin/env python3
"""
scripts/configure_ufo_dual_core.py
Configures and hardens Microsoft UFO for seamless Dual-Core integration with BankFidelity
while preserving 100% standalone capabilities for both applications.
"""

import os
import sys
from pathlib import Path
import yaml

UFO_DIR = Path(os.environ.get("BANKFIDELITY_UFO_DIR", "C:/ufo/ufo"))

BANKFIDELITY_API_PROMPT = """balance_statement:
  summary: |-
    "balance_statement" is to automatically audit and rebalance a bank statement PDF using BankFidelity's cryptographic math engine.
  class_name: |-
    BalanceStatementCommand
  usage: |-
    [1] API call: balance_statement(input: str, output: str, auto_approve: bool = True)
    [2] Args:
      - input: The absolute path to the input PDF bank statement.
      - output: The absolute path where the balanced output PDF will be saved.
      - auto_approve: Whether to automatically accept and commit math reconciliation fixes (Default: True).
    [3] Example: balance_statement(input="C:\\\\bankfidelity\\\\statement.pdf", output="C:\\\\bankfidelity\\\\statement_balanced.pdf", auto_approve=True)
    [4] Available control item: BankFidelity MCP
    [5] Return: Execution summary and mathematical status

modify_text:
  summary: |-
    "modify_text" is to modify or replace a text string on a specific page of a PDF with 100% cryptographic visual font fidelity.
  class_name: |-
    ModifyTextCommand
  usage: |-
    [1] API call: modify_text(input: str, output: str, old: str, new: str, bbox: str, page: int = 1)
    [2] Args:
      - input: Absolute path to the original PDF.
      - output: Absolute path for the modified PDF.
      - old: The exact existing text to replace.
      - new: The replacement text to render.
      - bbox: Bounding box coordinates "x0,y0,x1,y1" (in 72-DPI PDF points).
      - page: 1-indexed page number (Default: 1).
    [3] Example: modify_text(input="C:\\\\bankfidelity\\\\statement.pdf", output="C:\\\\bankfidelity\\\\output.pdf", old="Account 123", new="Account 999", bbox="100,200,300,220", page=1)
    [4] Available control item: BankFidelity MCP
    [5] Return: Modification status

extract_data:
  summary: |-
    "extract_data" is to extract document-level tabular transactions and metadata as structured JSON.
  class_name: |-
    ExtractDataCommand
  usage: |-
    [1] API call: extract_data(input: str, output: str)
    [2] Args:
      - input: Absolute path to the input PDF statement.
      - output: Absolute path where structured JSON will be written.
    [3] Example: extract_data(input="C:\\\\bankfidelity\\\\statement.pdf", output="C:\\\\bankfidelity\\\\statement.json")
    [4] Available control item: BankFidelity MCP
    [5] Return: Extraction JSON manifest

verify_layout:
  summary: |-
    "verify_layout" is to verify structural, font, SSIM, and mathematical integrity between an original and edited PDF.
  class_name: |-
    VerifyLayoutCommand
  usage: |-
    [1] API call: verify_layout(original: str, edited: str, output_dir: str)
    [2] Args:
      - original: Absolute path to the baseline reference PDF.
      - edited: Absolute path to the modified/candidate PDF.
      - output_dir: Directory where verification audit logs and visual diffs will be saved.
    [3] Example: verify_layout(original="C:\\\\orig.pdf", edited="C:\\\\edit.pdf", output_dir="C:\\\\audit")
    [4] Available control item: BankFidelity MCP
    [5] Return: Verification report with PASS/FAIL gate statuses

click_input:
  summary: |-
    "click_input" is to click a UI control element (Button, Menu, Tab, or Edit field) in the BankFidelity desktop window.
  class_name: |-
    ClickInputCommand
  usage: |-
    [1] API call: click_input(button: str = "left", double: bool = False, pressed: str = None)
    [2] Args:
      - button: Mouse button to click ('left', 'right', 'middle'). Default: 'left'
      - double: Whether to perform a double click (Default: False).
      - pressed: Keyboard modifier key held during click (e.g. 'CONTROL').
    [3] Example: click_input(button="left", double=False)
    [4] Available control item: All UI controls in BankFidelity
    [5] Return: None

set_edit_text:
  summary: |-
    "set_edit_text" is to input text into a BankFidelity text field, search box, or natural language input prompt.
  class_name: |-
    SetEditTextCommand
  usage: |-
    [1] API call: set_edit_text(text: str, clear_current_text: bool = True)
    [2] Args:
      - text: The string content to enter.
      - clear_current_text: If True, clears existing text before typing.
    [3] Example: set_edit_text(text="Shift all transaction dates by 3 days", clear_current_text=True)
    [4] Available control item: Edit controls
    [5] Return: None
"""

def configure():
    ufo_root = UFO_DIR
    if not ufo_root.exists():
        print(f"[ERROR] UFO root directory not found at {ufo_root}")
        return False

    print(f"[*] Configuring UFO at {ufo_root} for BankFidelity Dual-Core...")

    # 1. Create prompts/apps/bankfidelity/api.yaml
    prompt_dir = ufo_root / "prompts" / "apps" / "bankfidelity"
    prompt_dir.mkdir(parents=True, exist_ok=True)
    api_yaml_path = prompt_dir / "api.yaml"
    api_yaml_path.write_text(BANKFIDELITY_API_PROMPT, encoding="utf-8")
    print(f"  [+] Wrote BankFidelity App Prompt to {api_yaml_path}")

    # 2. Update config/ufo/agents.yaml
    agents_yaml_path = ufo_root / "config" / "ufo" / "agents.yaml"
    if agents_yaml_path.exists():
        content = agents_yaml_path.read_text(encoding="utf-8")
        data = yaml.safe_load(content) or {}
        
        prompt_map = data.get("APP_API_PROMPT_ADDRESS", {})
        prompt_map["dual-core-pdf-pipeline.exe"] = "ufo/prompts/apps/bankfidelity/api.yaml"
        prompt_map["BankFidelity_Stable.exe"] = "ufo/prompts/apps/bankfidelity/api.yaml"
        data["APP_API_PROMPT_ADDRESS"] = prompt_map

        # Ensure cloud primary with local fallback
        if "HOST_AGENT" in data and not data["HOST_AGENT"].get("API_BASE"):
            data["HOST_AGENT"]["API_BASE"] = "http://127.0.0.1:11434/v1"
            data["HOST_AGENT"]["API_MODEL"] = "qwen2.5-coder-7b-instruct-q4_k_m"

        agents_yaml_path.write_text(yaml.dump(data, default_flow_style=False, sort_keys=False), encoding="utf-8")
        print(f"  [+] Updated App Prompt mappings in {agents_yaml_path}")

    # 3. Update config/ufo/mcp.yaml
    mcp_yaml_path = ufo_root / "config" / "ufo" / "mcp.yaml"
    if mcp_yaml_path.exists():
        content = mcp_yaml_path.read_text(encoding="utf-8")
        data = yaml.safe_load(content) or {}
        
        mcp_servers = data.get("mcp_servers", {})
        mcp_servers["bankfidelity"] = {
            "type": "stdio",
            "command": r"C:\bankfidelity\bankfidelity\target\release\dual-core-pdf-pipeline.exe",
            "args": ["mcp"],
            "cwd": r"C:\bankfidelity\bankfidelity",
            "description": "BankFidelity Native Statement Editor, Precision Balancer & Visual Auditor MCP Server."
        }
        data["mcp_servers"] = mcp_servers

        # Register in HOST_AGENT and APP_AGENT action lists if not already present
        for agent_key in ["HOST_AGENT", "APP_AGENT"]:
            if agent_key in data and "default" in data[agent_key]:
                actions = data[agent_key]["default"].get("action", [])
                if not any(a.get("name") == "bankfidelity" for a in actions):
                    actions.append({
                        "name": "bankfidelity",
                        "namespace": "bankfidelity",
                        "type": "stdio"
                    })
                    data[agent_key]["default"]["action"] = actions

        mcp_yaml_path.write_text(yaml.dump(data, default_flow_style=False, sort_keys=False), encoding="utf-8")
        print(f"  [+] Registered BankFidelity stdio MCP server in {mcp_yaml_path}")

    print("[SUCCESS] BankFidelity Dual-Core configuration applied to UFO.")
    return True

if __name__ == "__main__":
    success = configure()
    sys.exit(0 if success else 1)
