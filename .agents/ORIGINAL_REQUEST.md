# Original User Request

## Initial Request — 2026-08-06T15:39:24Z

<USER_REQUEST>
Execute an open-ended, continuous improvement loop to optimize and harden the End-to-End (E2E) joint interaction between the BankFidelity Rust orchestrator and the Microsoft UFO Python backend.

Working directory: `C:\bankfidelity\bankfidelity` (and `C:\UFO` for Python)
Integrity mode: development

## Requirements

### R1. E2E Lifecycle & Function Audit
Perform a comprehensive lifecycle check mapping every function and sub-function of the joint application, tracking the data flow from initial PDF parsing and UI state, through the async Rust dispatcher, into the UFO offline agent (`qwen2.5-coder-7b-instruct-q4_k_m`), and back to the BankFidelity transaction transfer phase.

### R2. Local LLM Integration Hardening
Apply the `ufo_local_llm_integration` rules to `C:\UFO`. Ensure `config/ufo/system.yaml` is optimized for latency (`SLEEP_TIME: 0.2`, `SAVE_EXPERIENCE: "always_not"`, `VISUAL_MODE: False`) and implement the RegEx case-normalizer in the UFO JSON parser to fix PascalCase key emissions (`Function` -> `function`) and pop aliased keys before Pydantic validation.

### R3. Continuous Improvement Loop
Start a recursive optimization loop targeting both codebases:
1. Review the codebases and detect errors or regressions.
2. Repair found errors and remove over-engineered patterns.
3. Suggest UI/UX or integration improvements.
4. Implement the improvements.
5. Repeat until further improvements risk regression or over-engineering.

## Acceptance Criteria

### Verification & Stopping Condition
- [ ] A complete lifecycle audit document is generated detailing the joint interaction data flow.
- [ ] UFO JSON parsers are robustly hardened against local LLM formatting drift (e.g. `Function` -> `function` normalization).
- [ ] The Continuous Improvement loop runs until the Orchestrator Agent (Agent-as-Judge) determines that further edits introduce over-engineering.
- [ ] Programmatic Verification: If any changes cause `cargo check` or `pytest` to fail, the agent team must repair the break immediately. If repair is impossible, the team must rollback the app-breaking code before halting.
</USER_REQUEST>
