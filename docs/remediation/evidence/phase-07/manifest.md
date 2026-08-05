# Gate 07 Evidence Manifest

## Decision

**Gate:** 07 — independent fail-closed verification  
**Decision:** **PASS**  
**Verified candidate SHA:** `c354094b83e31ca7e026e7749c567931a97a43f4`  
**Workflow run:** [30730576656](https://github.com/flak3dd/bank-statement-fidelity-editor-remediated/actions/runs/30730576656)  
**Branch:** `master` in the owner-authorized public remediation repository  
**Release publication:** Frozen pending later GUI, package, qualification, hardening, and final-release gates

The candidate makes mandatory verification independent from editor success. It performs full-document structural, visual, exact-content, live-text editability, and strict financial checks; persists replayable atomic evidence with input and artifact hashes; uses immutable calibrated thresholds; treats every optional provider outcome explicitly; and makes report language evidence-scoped rather than universal.

## Ticket evidence

| Ticket | Implemented outcome | Principal evidence | Regression evidence | Result |
|---|---|---|---|---|
| VER-001 | Standalone verification passes identical controls, emits machine-readable evidence, and returns typed non-success for mutations or evidence failures. | `src/engine/verification.rs`, `src/app/runtime.rs`, `src/app/cli.rs`. | `tests/verifier_cli_contract.rs`, `tests/engine_verification_tests.rs`. | PASS |
| VER-002 | Independent structural checks cover openability, page count/order, MediaBox, CropBox, rotation, content presence, fonts/resources, and metadata policy. | `src/engine/verification_structural.rs`. | `tests/verification_structural_tests.rs`: identical, page loss/order, blank page, geometry, font, and metadata controls. | PASS |
| VER-003 | Exact old/new edit membership requires one source target, one replacement target, no stale source, no blanking, and no over-application; full-document visual checks reject unrelated changes. | `src/engine/verification_content.rs`, exact intent schema in `src/engine/verification.rs`. | `tests/verification_content_tests.rs`: missing, stale, wrong, duplicate, over-applied, and blanked controls. | PASS |
| VER-004 | Vision outcomes are typed `Passed`, `Rejected`, or `Unavailable`; malformed data, missing keys, authorization failure, and bounded timeout cannot create a pass. | `src/ai/vision.rs`, optional gates in `src/engine/verification.rs`. | Six mocked Vision provider contracts. | PASS |
| VER-005 | Financial verification is non-mutating and validates exact row count, order, page, date, description, sign, amount, running balances, and closing balance against an independent local reparse. | `src/engine/verification.rs`, workflow local reparse in `src/app/runtime.rs`. | Strict continuity and expected-ledger mutation tests. | PASS |
| VER-006 | Mandatory checks use one full-document pass with immutable thresholds, zero adaptive mask widening, and no exhausted-retry or near-perfect bypass. The exported legacy visual API ignores caller overrides. | `assets/verification-calibration-v2.json`, `src/engine/verification.rs`, `src/engine/verification_v2.rs`, `src/engine/workflow.rs`. | Calibration consistency, override rejection, and immutable policy tests. | PASS |
| VER-007 | Retained Vision and pdfRest providers are optional, bounded, privacy-scoped, and contract-tested. The nonexistent Applitools backend was removed from code, configuration, packages, UI, docs, and benchmarks. | `src/ai/vision.rs`, `src/ai/pdfrest.rs`, `src/app/config.rs`, `package.json`. | Five pdfRest and six Vision mocked contracts. | PASS |
| VER-008 | Evidence includes policy/version, calibration hash, original/edited hashes, exact intent set, thresholds, checked pages, gate outcomes, rendered artifact hashes/sizes, diagnostics, and disposition; writes are atomic and read back. | `src/engine/verification.rs`, evidence schema v2. | Evidence persistence failure blocks return; rendered artifact and readback assertions pass. | PASS |
| VER-009 | The supported corpus contains identical, intended edit, unrelated visible mutation, page loss/order, blank, font, geometry, edit-membership, provider, evidence, and financial mutation controls. | `tests/verification_*`, `tests/engine_verification_tests.rs`, provider unit tests. | All focused Gate 07 controls pass locally. | PASS |
| VER-010 | Human and GUI reports use evidence-scoped PASS/FAIL wording, immutable threshold context, and exact failed-gate identifiers without universal fidelity claims. | `src/engine/verification.rs`, `src/app/gui.rs`, `src/app/modals.rs`, `README.md`. | GUI/report compilation and source audit. | PASS |

## Local qualification

| Check | Result |
|---|---|
| Rust library suite | 309 passed, 0 failed |
| Structural negative controls | 4 passed, 0 failed |
| Exact content/editability controls | 5 passed, 0 failed |
| Engine evidence controls | 3 passed, 0 failed |
| Standalone verifier CLI contracts | 2 passed, 0 failed |
| Three-run repeatability | 1 passed, 0 failed; dispositions, gates, thresholds, input hashes, and rendered hashes repeat |
| Vision provider contracts | 6 passed, 0 failed |
| pdfRest provider contracts | 5 passed, 0 failed |
| Disabled font substitution | No artifact produced; stable `FONT_SUBSTITUTION_DISABLED` result |
| Generated Python protocol/runtime manifests | Current and base-runtime verified |
| All-target compilation | PASS |
| Strict Clippy | PASS with `-D warnings` |
| Formatting and diff hygiene | PASS |

## Gate invariants

| Invariant | Result |
|---|---|
| Optional provider failure cannot override mandatory local failure | PASS |
| Missing renderer/dependency/evidence cannot produce success | PASS |
| Every page is structurally and visually covered | PASS |
| Exact intended edits are independently proven in live text | PASS |
| Unrelated, duplicate, missing, stale, wrong, blank, and over-applied edits fail | PASS |
| Financial evidence is checked without reconciliation or silent correction | PASS |
| Thresholds cannot widen during a run | PASS |
| Evidence is replayable and tamper-evident through hashes/readback | PASS |
| Reports avoid universal or “perfect” claims | PASS |

## Public workflow

Run `30730576656` completed successfully for candidate `c354094b83e31ca7e026e7749c567931a97a43f4`. Blocking jobs passed: rustfmt `91450137742`; Clippy `91450137747`; Ubuntu base `91450137751`; macOS base `91450137738`; Windows base `91450137739`; optional-Pro macOS `91450137754`; optional-Pro Windows `91450137735`; P0 macOS `91450535515`; P0 Ubuntu `91450535516`; and P0 Windows `91450535517`. The deferred hardening inventory `91450137725` remains an expected advisory failure until its scheduled final hardening gate and does not weaken any functional verifier requirement.

## Advancement

Gate 07 is closed. Gate 08 closes with a documented **no-go for a mandatory or bundled local LLM in v1**, preserving the deterministic verified core and explicit unavailable capability. The next active phase is Phase 09: coherent GUI, batch, recovery, audit UX, and accessibility.
