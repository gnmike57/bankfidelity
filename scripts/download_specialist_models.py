"""
BankFidelity specialist model pre-staging.

Stages optional specialist weights under $BF_MODELS_DIR (default: <repo>/models):
  1. Microsoft Florence-2-Large (image grounding; opt-in via BF_SPECIALIST_FLORENCE=1)
  2. Surya layout / reading order (opt-in via BF_SPECIALIST_SURYA=1)

Large VLMs for the GX10 are NOT downloaded here: serve them with vLLM and point
LOCAL_VLM_URL / LOCAL_VLM_MODEL at the server (see scripts/gx10/).
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MODELS_ROOT = Path(os.environ.get("BF_MODELS_DIR") or REPO_ROOT / "models")


def stage_florence2() -> bool:
    print("[1/2] Staging Florence-2-Large ...")
    dest = MODELS_ROOT / "florence2"
    dest.mkdir(parents=True, exist_ok=True)
    try:
        from transformers import AutoModelForCausalLM, AutoProcessor

        model_id = "microsoft/Florence-2-large"
        AutoProcessor.from_pretrained(model_id, trust_remote_code=True, cache_dir=str(dest))
        AutoModelForCausalLM.from_pretrained(model_id, trust_remote_code=True, cache_dir=str(dest))
    except (ImportError, OSError, ValueError, RuntimeError) as exc:
        print(f"      [FAIL] {type(exc).__name__}: {exc}")
        return False
    print("      [OK]")
    return True


def stage_surya() -> bool:
    print("[2/2] Staging Surya ordering weights ...")
    (MODELS_ROOT / "surya").mkdir(parents=True, exist_ok=True)
    try:
        from surya.model.ordering.model import load_model
        from surya.model.ordering.processor import load_processor

        load_processor()
        load_model()
    except ImportError as exc:
        print(f"      [FAIL] installed surya lacks the legacy ordering API: {exc}")
        return False
    except (OSError, ValueError, RuntimeError) as exc:
        print(f"      [FAIL] {type(exc).__name__}: {exc}")
        return False
    print("      [OK]")
    return True


def main() -> int:
    print(f"Model root: {MODELS_ROOT}")
    MODELS_ROOT.mkdir(parents=True, exist_ok=True)
    results = {"florence2": stage_florence2(), "surya": stage_surya()}
    failed = [name for name, ok in results.items() if not ok]
    if failed:
        print(f"[INCOMPLETE] failed: {', '.join(failed)}")
        return 1
    print("[COMPLETE]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
