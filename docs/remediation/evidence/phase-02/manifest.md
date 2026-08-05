# Gate 02 Evidence Manifest

**Disposition:** `PASS`
**Phase:** Phase 02 — Critical integrity, exact success, and immutable history
**Gate:** Gate 02
**Base commit:** `a85c8889ca6a27ed2bcfe9e9181eea5a607f992c`
**Candidate branch:** `remediation/phase-02-core-integrity`
**Candidate commit:** `500167b3caff3090d477c15e1f28f17e92073b57`
**Remote CI run:** [30673473276](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30673473276) — overall `success`
**Prepared by:** Manus AI
**Date:** 2026-08-01

## Scope and ticket disposition

| Ticket | Severity | State | Evidence |
|---|---:|---|---|
| INT-001 | P0 | Implemented and cross-platform verified | Rust and Python share a strict, hash-backed `ApplyReport`; malformed, partial, unknown, count-mismatched, or hash-mismatched reports are rejected. |
| INT-002 | P0 | Implemented and cross-platform verified | No-overlap edits return complete failure evidence, preserve source/output bytes, and cannot publish blank redaction. |
| INT-003 | P0 | Implemented and cross-platform verified | Short documents use one ordered exact batch and one durable commit; the Python suite proves an exact repeatable twenty-edit transaction. |
| INT-004 | P1 | Implemented | Per-keystroke fire-and-forget PDF writes and their obsolete runtime job were removed; explicit preview/confirm is the only mutation path. |
| INT-005 | P0 | Implemented and cross-platform verified | Preview is non-mutating, derived balance edits are materialized into the exact renderer edit set, and unbalanced input is rejected before PDF mutation. |
| INT-006 | P0 | Implemented and cross-platform verified | Zero-row extraction/balance is an explicit failure; balance success requires exact applied counts and a nonempty requested output artifact. |
| INT-007 | P0 | Implemented and cross-platform verified | Snapshots are independent content-addressed objects with hash, size, parent, creation, and manifest evidence; tamper and missing objects are rejected. |
| INT-008 | P1 | Implemented and cross-platform verified | Output, snapshot, history, segment, and audit publication use staged all-or-nothing commit barriers with automatic rollback. |
| INT-009 | P0 | Implemented and mandatory | Windows, macOS, and Linux P0 jobs run Python exact-edit, audit tamper, commit rollback, and real runtime integrity regressions on every push and pull request. |
| INT-010 | P3 | Implemented for Phase 02 paths | Destructive prevention, count mismatch, unbalanced preview, empty extraction, and commit failure now emit explicit actionable errors. Broader copy consistency remains covered by the UX phase. |

## Critical repairs

| Area | Audited failure | Repair |
|---|---|---|
| Python batch editing | A no-match edit could redact content, place no text, and report success. | Validate every match and placement, return per-edit evidence, verify source/output hashes, and publish only exact success. |
| Rust/Python contract | Runtime accepted arbitrary JSON and did not require exact edit counts. | Parse a deny-unknown typed report and enforce requested = matched = placed with zero failures. |
| Multi-edit lifecycle | Short documents queued independent writers against one output and could lose edits. | One ordered document transaction writes to a unique scratch file and commits once after exact verification. |
| Balance preview | Preview could mutate transaction amounts and show a balanced state not represented in renderer edits. | Preview is non-mutating; every derived balance update becomes an explicit edit; unbalanced or unresolved previews are blocked. |
| Empty success | Empty extraction and balance results could exit successfully without the requested artifact. | Zero rows are incomplete and success requires exact counts plus a durable nonempty output. |
| Audit history | Hard-linked snapshots changed when the live output changed. | Independent content-addressed snapshot objects and manifests are hash/size verified before replay. |
| Commit ordering | Output and evidence writes could partially succeed. | A reusable staged file barrier restores every prior destination on any later failure. |
| Terminal lifecycle | Intermediate results could be treated as terminal and late results could duplicate success/failure. | Terminal classification is explicit and exactly one final event is emitted; post-terminal events are suppressed. |
| GUI mutation | Editing text could launch overlapping full-PDF background writes. | The per-keystroke writer was removed; edits remain in memory until explicit serialized confirmation. |
| Geometry interoperability | Native extraction emitted PDF bottom-left rectangles while PyMuPDF consumed top-left rectangles. | The engine contract now requires canonical visible top-left geometry; native extraction and writes convert at their boundary. |

## Mandatory cross-platform evidence

| Platform / gate | Job | Result |
|---|---:|---|
| Windows x64 base state | `91295849364` | PASS |
| macOS Apple Silicon base state | `91295849381` | PASS |
| Linux development base state | `91295849378` | PASS |
| Windows P0 integrity regressions | `91296489736` | PASS |
| macOS P0 integrity regressions | `91296489716` | PASS |
| Linux P0 integrity regressions | `91296489755` | PASS |
| Strict production Clippy | `91295849376` | PASS |
| Rust formatting | `91295849379` | PASS |
| Windows optional-Pro import | `91295849337` | PASS |
| macOS optional-Pro import | `91295849398` | PASS |

The deferred dependency and supply-chain inventory remained intentionally non-blocking under the owner-approved base-state-first sequencing. It reported the previously audited advisories and is assigned to final hardening; no active credential or data-exfiltration issue was discovered in this phase.

## Regression evidence

| Contract | Result |
|---|---|
| Python no-overlap source/output preservation | PASS on all P0 runners |
| Python exact counts, hashes, and per-edit evidence | PASS on all P0 runners |
| Twenty-edit semantic transaction repeatability | PASS on all P0 runners |
| Typed Rust `ApplyReport` strict parsing and invariants | PASS in base-state library suites |
| Unbalanced ledger rejected before output mutation | PASS on all P0 runners |
| Zero-row extraction and balance never report success | PASS on all P0 runners |
| Immutable snapshot independence, hash, size, parent, and tamper checks | PASS on all P0 runners |
| Commit barrier success, overwrite, create, and injected rollback | PASS on all P0 runners |
| Short-document multi-edit publishes both replacements before success | PASS on all P0 runners |
| Exactly-once terminal classification and silent-drop failure | PASS in base-state library suites |
| Native canonical coordinate round trip | PASS in base-state library suites |

## Failures encountered and repaired during gate construction

| Failure | Classification | Resolution |
|---|---|---|
| The first real-runtime P0 run failed on every platform because native bboxes used bottom-left PDF content coordinates and PyMuPDF expected top-left page coordinates. | Production cross-engine geometry defect | Defined one engine-wide top-left contract, resolved inherited page boxes, converted at the native boundary, and added round-trip tests. |
| The runtime regression assumed PyMuPDF would preserve one text operator per page. | Test characterization defect | Assert exact normalized replacement semantics across valid span fragmentation and require original text absence. |
| Fresh P0 runners could conflict while auto-installing `cargo-fmt` during cache probing. | CI toolchain setup defect | Install pinned rustfmt and Clippy components explicitly before cache probing, matching the passing base-state jobs. |

## Evidence artifacts

| Artifact | Purpose |
|---|---|
| `ci-run-30673473276.json` | Complete final workflow state and job/step evidence. |
| `ci-jobs-30673473276.tsv` | Concise mandatory/advisory job matrix with timestamps. |
| `commits.tsv` | Ordered Phase 02 implementation checkpoint list. |
| `evidence.sha256` | SHA-256 checksums for the captured evidence files. |
| `python/test_apply_many_edits_contract.py` | Non-destructive no-match, exact success, and twenty-edit stress regressions. |
| `tests/integrity_regressions.rs` | Real runtime balance, empty-result, and multi-edit lifecycle regressions. |
| `src/app/commit.rs` | Unit-tested all-or-nothing file commit barrier. |

## Migration and rollback

No customer schema migration is required. Existing history records remain readable because snapshot evidence is optional on legacy records. New records include content-addressed evidence and are verified before replay. Rollback is a branch revert to the Gate 01 checkpoint; reverting individual integrity commits is prohibited because later contracts depend on typed exact results, canonical geometry, and immutable history.

## Gate decision

| Requirement | Result |
|---|---|
| Five original P0 failure classes closed | PASS |
| Exact edit accounting before success | PASS |
| Durable requested artifact before success | PASS |
| Deterministic balance evidence before mutation | PASS |
| Independent snapshot and audit evidence | PASS |
| All-or-nothing publication and rollback | PASS |
| Mandatory Windows/macOS/Linux P0 matrix | PASS |
| Gate 01 regressions remain green | PASS |
| Repository release remains frozen | PASS |

**Final disposition:** `PASS`
