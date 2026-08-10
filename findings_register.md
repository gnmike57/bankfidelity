# BankFidelity Audit: Findings Register

## Final Disposition: FAIL
*The audit success criterion requires 0 P0/P1 findings. The audit identified 1 P0 finding and 4 P1 findings.*

---

### [FND-001] [P0] [Security] Hardcoded Fallback Passphrase in `dev` feature
**Description:** Enabling the `dev` feature flag (included in `--all-features`) activates a hardcoded fallback passphrase in the software root. This bypasses the strict `DUAL_CORE_PASSPHRASE` requirement and introduces a critical security vulnerability if compiled or shipped with this feature.
**Reproducibility Evidence:**
- **Command:** Identified via static analysis / feature review during baseline mapping.
- **Expected:** Secure encryption requiring user-provided entropy.
- **Observed:** Feature flag bypasses security constraint.
- **Severity Justification:** Silent corpus-poisoning/data-exposure if deployed.

---

### [FND-002] [P1] [Coverage Gap] Missing Workflow E2E Fixtures causing Self-Skip
**Description:** The E2E workflow test suite self-skips because it hard-requires the `AU Bank Statements/IA_Bank_Statement_202602.pdf` fixture, which does not exist in the repository (fixtures are named per-bank).
**Reproducibility Evidence:**
- **Command:** `cargo test --nocapture`
- **Output Snippet:** `[skip] AU statement not present at {}; e2e test self-skipped`
- **Severity Justification:** E2E workflow remains untested in CI, preventing validation of critical paths.

---

### [FND-003] [P1] [Correctness] Missing OCR Models when `ocr` feature is enabled
**Description:** Enabling the `ocr` feature flag triggers a path requiring external model files that are not present in the repository, altering capability detection and breaking the offline parser.
**Reproducibility Evidence:**
- **Command:** Static analysis / feature review.
- **Severity Justification:** Breaks core functionality when the feature is enabled.

---

### [FND-004] [P1] [Validation Gate] Code Formatting Drift in `selector.rs`
**Description:** The codebase fails `cargo fmt --check`, specifically around recent modifications to the panic-guard logic in `src/pdf/selector.rs`.
**Reproducibility Evidence:**
- **Command:** `cargo fmt --check`
- **Exit Code:** 1
- **Output Snippet:** 
```diff
-        let run_safe = |engine: &dyn PdfEngine| -> Result<T, EngineError> {
-            operation(engine)
-        };
+        let run_safe = |engine: &dyn PdfEngine| -> Result<T, EngineError> { operation(engine) };
```

---

### [FND-005] [P1] [Coverage Gap] LNK1140 Windows MSVC Debug Limitation
**Description:** Source-based coverage instrumentation fails on Windows because enabling `debug = true` produces a PDB over 4GB, overflowing the MSVC linker (LNK1140). This forces `debug = false` in development profiles, preventing automated coverage metrics.
**Reproducibility Evidence:**
- **Command:** Contextual discovery during configuration audit.
- **Severity Justification:** Severely limits automated test coverage visibility on Windows platforms.
