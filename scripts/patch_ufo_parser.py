import sys
import os
import re

def patch_ufo_parser(ufo_dir: str):
    """
    Local models (especially Qwen) often emit JSON with PascalCase keys:
    {"Function": "run_shell"} instead of {"function": "run_shell"}.
    This script aggressively patches UFO's parser utility to case-normalize keys
    and strip out extra aliased keys before validation.
    """
    utils_path = os.path.join(ufo_dir, "ufo", "utils", "__init__.py")
    
    if not os.path.exists(utils_path):
        print(f"Cannot patch: {utils_path} does not exist.")
        return

    with open(utils_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # We want to inject a regex normalizer into UFO's JSON parsing or dict extraction.
    # Instead of blindly replacing, we provide a standalone utility they can use.
    # If the user already cloned it, we could inject directly into `json_parser` if it exists.
    # For now, we will just inject a robust function at the bottom.
    
    robust_parser = r"""
# --- INJECTED BY BANKFIDELITY ---
import re
import json

def parse_local_llm_json(raw_str: str) -> dict:
    '''Normalizes PascalCase and strips aliases for Local LLMs (Qwen).'''
    # 1. Normalize Function -> function, Args -> arguments
    normalized = re.sub(r'\"Function\"\s*:', '\"function\":', raw_str)
    normalized = re.sub(r'\"Args\"\s*:', '\"arguments\":', normalized)
    
    try:
        data = json.loads(normalized)
        if isinstance(data, dict):
            # 2. Pop extra aliased keys that break FastMCP/Pydantic
            data.pop("cmd", None)
            data.pop("Action", None)
            
        return data
    except json.JSONDecodeError:
        return {}
# --------------------------------
"""
    if "parse_local_llm_json" not in content:
        with open(utils_path, 'a', encoding='utf-8') as f:
            f.write(robust_parser)
        print("Successfully injected Local LLM robust JSON parser into UFO utils.")
    else:
        print("Robust JSON parser is already injected.")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python patch_ufo_parser.py <path_to_ufo_repo>")
        sys.exit(1)
        
    patch_ufo_parser(sys.argv[1])
