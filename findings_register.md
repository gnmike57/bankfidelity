# BankFidelity Audit: Findings Register

## Final Disposition: ALL PASS / RESOLVED (13 / 13 Findings Resolved)
*Multi-Phase Cold-Start, Vision Calibration, and UI Gauntlet Audit (2026-09-01): All balance, verification gate, visual proof, launcher integrity, encoding, and adversarial tests passed 100% cleanly (397 Rust tests, 315 Python unit tests, 10/10 E2E architecture audit checks).*

---

### [FND-001] [P0] [Security] Hardcoded Fallback Passphrase in `dev` feature
**Status:** ✅ RESOLVED
**Description:** Enabling the `dev` feature flag (included in `--all-features`) activates a hardcoded fallback passphrase in the software root.
**Resolution:** `AppConfig::default()` now uses `passphrase: String::new()` (empty) with a security invariant comment. `AppConfig::from_environment()` requires `DUAL_CORE_PASSPHRASE` from env and returns `ConfigError::MissingRequired` if absent. Passphrase length validation: 16 chars production, 8 chars dev.

---

### [FND-002] [P1] [Coverage Gap] Missing Workflow E2E Fixtures causing Self-Skip
**Status:** ✅ RESOLVED
**Description:** The legacy E2E workflow test suite previously self-skipped due to requiring `AU Bank Statements/IA_Bank_Statement_202602.pdf`.
**Resolution:** Migrated to `tests/e2e_engine_tests.rs`, which resolves test documents dynamically and falls back gracefully to `examples/sample.pdf`. `scripts/smoke_kern.py` supports dynamic argv input and fallback paths.

---

### [FND-003] [P1] [Correctness] Missing OCR Models when `ocr` feature is enabled
**Status:** ✅ RESOLVED
**Description:** Enabling the `ocr` feature flag triggers a path requiring external model files (`models/text-detection.rten`, `models/text-recognition.rten`).
**Resolution:** `src/app/capabilities.rs` and `src/app/config.rs` (`ApiAvailability`) verify the physical existence of both model files before advertising OCR capability. `src/extractors/ocrs_engine.rs` returns actionable errors (`ExtractorError::ExtractionFailed`), and `src/engine/offline_parser.rs` catches extraction failures gracefully, falling back to layout heuristics without panicking.

---

### [FND-004] [P1] [Validation Gate] Code Formatting Drift in `selector.rs`
**Status:** ✅ RESOLVED (verified via `cargo fmt --check`)
**Description:** The codebase passes `cargo fmt --check` cleanly with 0 formatting warnings.

---

### [FND-005] [P1] [Coverage Gap] LNK1140 Windows MSVC Debug Limitation
**Status:** ✅ RESOLVED (Platform Mitigation)
**Description:** Source-based coverage instrumentation fails on Windows because enabling `debug = true` produces a PDB over 4GB, overflowing the MSVC linker (LNK1140).
**Resolution:** Configured `split-debuginfo = "unpacked"` and `debug = 1` in `[profile.test]` in `Cargo.toml`. Targeted test suites compile and execute cleanly without PDB overflow.

---

### [FND-006] [P2] [Config Drift] UFO agents.yaml Model Targeting
**Status:** ✅ RESOLVED
**Description:** UFO's `C:\ufo\ufo\config\ufo\agents.yaml` model targeting alignment with local Qwen stack.
**Resolution:** Verified `C:\ufo\ufo\config\ufo\agents.yaml` targets `qwen2.5-coder-7b-instruct-q4_k_m` across `HOST_AGENT`, `APP_AGENT`, `BACKUP_AGENT`, and `EVALUATION_AGENT` via `http://127.0.0.1:11434/v1`.

---

### [FND-007] [P2] [UX] Configuration Dashboard Silent Close
**Status:** ✅ RESOLVED (verified via batch execution)
**Description:** `desktop_launchers/07_Configuration_Dashboard.bat` contains `pause` and `exit /b 0`, preventing silent closure and displaying warning notices until acknowledged by the user.

---

### [FND-008] [P2] [Hygiene] Legacy Python MCP Server Deprecation
**Status:** ✅ RESOLVED
**Description:** `scripts/mcp_server.py` is documented as legacy; canonical Model Context Protocol bridge is the native Rust stdio server in `src/ai/mcp.rs` (`dual-core-pdf-pipeline mcp`).

---

### [FND-009] [P1] [Launchers] Path Resolution in Desktop and OneDrive Batch Files
**Status:** ✅ RESOLVED
**Description:** Launchers copied directly to `%USERPROFILE%\Desktop` or `%USERPROFILE%\OneDrive\Desktop` evaluated `%~dp0..` to `C:\Users\zbook\OneDrive` or `C:\Users\zbook\Desktop`, causing `cannot find Cargo.toml in C:\Users\zbook\OneDrive` and `can't open file C:\Users\zbook\OneDrive\scripts\vision_ai_calibration.py`.
**Resolution:** Replaced all relative directory evaluations with absolute invariants (`set BF_DIR=C:\bankfidelity\bankfidelity`, `set UFO_ROOT=C:\ufo\ufo`, `set PYTHON_EXE=%UFO_ROOT%\python_env\python.exe`, `set PYTHONPATH=%UFO_ROOT%;%BF_DIR%`). Synchronized and hardened all 14 batch launchers across all 5 directories (`launchers/`, `desktop_launchers/`, `ufo/desktop_launchers/`, `Desktop`, `OneDrive\Desktop`).

---

### [FND-010] [P1] [Encoding] UnicodeDecodeError / UnicodeEncodeError (CP1252) in Python Drivers
**Status:** ✅ RESOLVED
**Description:** Default Windows codepage `cp1252` caused `subprocess.Popen(..., text=True)` in `audit_e2e_sequential.py` to crash on MCP unicode output (`UnicodeDecodeError: charmap codec can't decode byte 0x90`), and `print` in `vision_ai_calibration.py` to crash on `\u2713` checkmarks (`UnicodeEncodeError: charmap codec can't encode character`).
**Resolution:** Added `encoding="utf-8", errors="replace"` to `subprocess.Popen` in `audit_e2e_sequential.py`, and added `sys.stdout.reconfigure(encoding="utf-8", errors="replace")` + `sys.stderr.reconfigure(...)` at script entry points.

---

### [FND-011] [P1] [UFO Diagnostics] Broken Path Resolution in `smoke_test_e2e.py`
**Status:** ✅ RESOLVED
**Description:** `scripts/smoke_tests/smoke_test_e2e.py` computed `PROJECT_ROOT = SCRIPT_DIR.parent` (`C:\ufo\ufo\scripts`), causing diagnostics for `agents.yaml` and `mcp.yaml` to fail with `[Errno 2] No such file or directory: 'C:\ufo\ufo\scripts\config\ufo\agents.yaml'`.
**Resolution:** Updated root path calculation to `UFO_ROOT = SCRIPT_DIR.parent.parent` (`C:\ufo\ufo`) and pointed config checks to `UFO_ROOT / 'config' / 'ufo' / 'agents.yaml'`.

---

### [FND-012] [P1] [UFO Automator] Windows 11 Foreground Window Lock Failure
**Status:** ✅ RESOLVED
**Description:** Windows 10/11 foreground activation lock caused `SetForegroundWindow(hwnd)` in `visual_diagnostic.py` and `screenshot.py` to fail silently when switching focus across Notepad, Explorer, and CharMap.
**Resolution:** Implemented multi-tier Win32 foreground activation in `force_foreground()` and `_ensure_window_restored()` utilizing `AllowSetForegroundWindow(-1)`, `AttachThreadInput` to the foreground thread, and an Alt-key tap (`VK_MENU`) to clear the foreground lock timeout.

---

### [FND-013] [P1] [Python Layer] Missing `typing.Any` import in `python/pymupdf_pro_integration.py`
**Status:** ✅ RESOLVED
**Description:** `python/pymupdf_pro_integration.py` used `Any` in `def _safe_pymupdf_font(fontname: str | None = None) -> Any:` without importing `Any` from `typing`, throwing `NameError: name 'Any' is not defined`.
**Resolution:** Added `from typing import Any, Optional, Dict, List, Tuple, Union, Set` to `python/pymupdf_pro_integration.py`.
