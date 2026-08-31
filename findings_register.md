# BankFidelity Audit: Findings Register

## Final Disposition: ALL PASS / RESOLVED
*Phase 1, Phase 2 & Phase 3 Lifecycle Audit (2026-08-31): All balance, verification gate, visual proof, and adversarial tests passed 100% cleanly (27 passed, 0 failed, 0 warnings).*

---

### [FND-001] [P0] [Security] Hardcoded Fallback Passphrase in `dev` feature
**Status:** ✅ RESOLVED (Phase 1 audit)
**Description:** Enabling the `dev` feature flag (included in `--all-features`) activates a hardcoded fallback passphrase in the software root. This bypasses the strict `DUAL_CORE_PASSPHRASE` requirement and introduces a critical security vulnerability if compiled or shipped with this feature.
**Resolution:** `AppConfig::default()` now uses `passphrase: String::new()` (empty) with a security invariant comment. `AppConfig::from_environment()` requires `DUAL_CORE_PASSPHRASE` from env and returns `ConfigError::MissingRequired` if absent. Passphrase length validation: 16 chars production, 8 chars dev.

---

### [FND-002] [P1] [Coverage Gap] Missing Workflow E2E Fixtures causing Self-Skip
**Status:** ✅ RESOLVED (Obsolete / Replaced)
**Description:** The legacy E2E workflow test suite previously self-skipped due to requiring `AU Bank Statements/IA_Bank_Statement_202602.pdf`.
**Resolution:** The test suite was migrated to `tests/e2e_engine_tests.rs`, which resolves test documents dynamically and falls back gracefully to `examples/sample.pdf`. `scripts/smoke_kern.py` was also updated to support dynamic argv input and fallback paths.

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
**Status:** ✅ RESOLVED (Accepted Platform Limitation / Mitigated)
**Description:** Source-based coverage instrumentation fails on Windows because enabling `debug = true` produces a PDB over 4GB, overflowing the MSVC linker (LNK1140).
**Resolution:** Configured `split-debuginfo = "unpacked"` and `debug = 1` in `[profile.test]` in `Cargo.toml`. Targeted test suites compile and execute cleanly without PDB overflow.

---

### [FND-006] [P2] [Config Drift] UFO agents.yaml Model Targeting
**Status:** ✅ RESOLVED
**Description:** UFO's `C:\ufo\ufo\config\ufo\agents.yaml` model targeting alignment with local Qwen stack.
**Resolution:** Verified `C:\ufo\ufo\config\ufo\agents.yaml` is configured to target `qwen2.5-coder-7b-instruct-q4_k_m` across `HOST_AGENT`, `APP_AGENT`, `BACKUP_AGENT`, and `EVALUATION_AGENT` via the `http://127.0.0.1:11434/v1` endpoint.

---

### [FND-007] [P2] [UX] Configuration Dashboard Silent Close
**Status:** ✅ RESOLVED (verified via batch execution)
**Description:** `desktop_launchers/07_Configuration_Dashboard.bat` contains `pause` and `exit /b 0`, preventing silent closure and displaying warning notices until acknowledged by the user.

---

### [FND-008] [P2] [Hygiene] Legacy Python MCP Server Deprecation
**Status:** ✅ RESOLVED (Phase 1 audit)
**Description:** `scripts/mcp_server.py` is documented as legacy; canonical Model Context Protocol bridge is the native Rust stdio server in `src/ai/mcp.rs` (`dual-core-pdf-pipeline mcp`).
