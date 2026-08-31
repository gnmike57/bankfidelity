# BankFidelity Audit: Findings Register

## Final Disposition: IMPROVED (was FAIL)
*Phase 1 lifecycle audit (2026-08-30): All P0 findings resolved; 2 original P1 findings remain (FND-002, FND-003); 3 new P2 findings added.*

---

### [FND-001] [P0] [Security] Hardcoded Fallback Passphrase in `dev` feature
**Status:** ✅ RESOLVED (Phase 1 audit 2026-08-30)
**Description:** Enabling the `dev` feature flag (included in `--all-features`) activates a hardcoded fallback passphrase in the software root. This bypasses the strict `DUAL_CORE_PASSPHRASE` requirement and introduces a critical security vulnerability if compiled or shipped with this feature.
**Resolution:** `AppConfig::default()` now uses `passphrase: String::new()` (empty) with a security invariant comment. `AppConfig::from_environment()` requires `DUAL_CORE_PASSPHRASE` from env and returns `ConfigError::MissingRequired` if absent. Passphrase length validation: 16 chars production, 8 chars dev.

---

### [FND-002] [P1] [Coverage Gap] Missing Workflow E2E Fixtures causing Self-Skip
**Status:** 🟡 OPEN
**Description:** The E2E workflow test suite self-skips because it hard-requires the `AU Bank Statements/IA_Bank_Statement_202602.pdf` fixture, which does not exist in the repository (fixtures are named per-bank).
**Reproducibility Evidence:**
- **Command:** `cargo test --nocapture`
- **Output Snippet:** `[skip] AU statement not present at {}; e2e test self-skipped`
- **Severity Justification:** E2E workflow remains untested in CI, preventing validation of critical paths.

---

### [FND-003] [P1] [Correctness] Missing OCR Models when `ocr` feature is enabled
**Status:** 🟡 OPEN
**Description:** Enabling the `ocr` feature flag triggers a path requiring external model files that are not present in the repository, altering capability detection and breaking the offline parser.
**Reproducibility Evidence:**
- **Command:** Static analysis / feature review.
- **Severity Justification:** Breaks core functionality when the feature is enabled.

---

### [FND-004] [P1] [Validation Gate] Code Formatting Drift in `selector.rs`
**Status:** ✅ RESOLVED (needs re-verification after build)
**Description:** The codebase fails `cargo fmt --check`, specifically around recent modifications to the panic-guard logic in `src/pdf/selector.rs`.

---

### [FND-005] [P1] [Coverage Gap] LNK1140 Windows MSVC Debug Limitation
**Status:** 🟡 OPEN (by design — platform limitation)
**Description:** Source-based coverage instrumentation fails on Windows because enabling `debug = true` produces a PDB over 4GB, overflowing the MSVC linker (LNK1140). This forces `debug = false` in development profiles, preventing automated coverage metrics.
**Reproducibility Evidence:**
- **Command:** Contextual discovery during configuration audit.
- **Severity Justification:** Severely limits automated test coverage visibility on Windows platforms.

---

### [FND-006] [P2] [Config Drift] UFO agents.yaml Model Targeting
**Status:** 🟡 OPEN (requires user confirmation to modify UFO configs)
**Description:** UFO's `C:\ufo\ufo\config\ufo\agents.yaml` targets `gghfez/Amoral-gemma3-12B-vision` (HOST_AGENT) and `Qwen3.8-27B-AEON-ULTIMATE-UNCENSORED` (APP_AGENT) instead of the expected `qwen2.5-coder-7b-instruct-q4_k_m` per skill docs and QUICKSTART.md.
**Reproducibility Evidence:**
- **Command:** `Select-String -Path "C:\ufo\ufo\config\ufo\agents.yaml" -Pattern "model"`
- **Severity Justification:** UFO won't route correctly to the local Qwen model specified in BankFidelity docs.

---

### [FND-007] [P2] [UX] Configuration Dashboard Silent Close
**Status:** 🟡 OPEN
**Description:** `desktop_launchers/07_Configuration_Dashboard.bat` uses bare `exit` (line 8) instead of `pause`. If all three config files are missing, warnings flash and the window closes instantly, giving the user no time to read them.

---

### [FND-008] [P2] [Hygiene] Legacy Python MCP Server Not Marked Deprecated
**Status:** 🟡 OPEN (requires confirmation before modification)
**Description:** `scripts/mcp_server.py` is a legacy Python MCP server surface that duplicates the canonical Rust stdio MCP server in `src/ai/mcp.rs`. It should be marked deprecated in docs to avoid confusion.
| NEW-3 | P1 | Local LLM model qwen2.5-coder-7b-instruct-q4_k_m.gguf missing from C:\UFO\models | ?? OPEN |
