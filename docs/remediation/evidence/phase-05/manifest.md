# Gate 05 Evidence Manifest

## Decision

**Gate:** 05 — extraction completeness and financial correctness

**Decision:** **PASS**

**Base SHA:** `dc2df68`

**Verified candidate SHA:** `9e5e8a1ca7ee32b3a8c8ee4bf73c3cbb2958c64f`

**Workflow run:** [30712005667](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30712005667)

**Branch:** `remediation/phase-05-extraction-correctness`

**Default branch modified:** No

**Release publication:** Frozen

The candidate defines one exact canonical ledger, repairs the original two-page zero-row failure to a verified 30-row result, honors the selected provider without unrelated cloud calls, blocks incomplete or low-confidence ledgers before Editing, removes unqualified Local OCR from v1, requires deterministic financial evidence before and after output, validates dates, supports provider-free transfer, and adds bounded folder extraction with complete per-file results.

## Ticket evidence

| Ticket | Implemented outcome | Principal implementation evidence | Regression or gate evidence | Result |
|---|---|---|---|---|
| EXT-001 | The canonical transaction schema carries exact `Decimal` amounts, explicit money direction, currency/locale metadata, stable row ID, date, page/field geometry, provenance, bounded confidence, and review state. Legacy artifacts normalize deterministically. | `src/engine/model.rs`, `src/ai/document_ai.rs`, parser constructors and caches. | Canonical round-trip and legacy-JSON tests; all-target constructor migration. | PASS |
| EXT-002 | Extraction takes an explicit parser mode. Offline stays local; selected LlamaParse or Document AI may fall back only to offline; Local OCR fails explicitly. Parser-specific cache keys and deterministic validation prevent cross-mode or incomplete cache reuse. | `src/app/runtime.rs`, `src/app/cli.rs`, `src/app/gui.rs`, `src/app/modals.rs`. | Table-driven provider-order test; selected offline 30-row runtime integration; zero-row rejection. | PASS |
| EXT-003 | ISO/hyphenated and existing date formats parse correctly. Spatial amount fields, running-balance continuity, direction, and field geometry produce the exact representative ledger instead of `[]`. | `src/engine/offline_parser.rs`. | Original two-page fixture: 30 rows, exact transitions, closing balance, required field geometry, and canonical confidence. | PASS |
| EXT-004 | Row/page ordering, duplicate identity, required values, geometry, confidence/review state, opening/closing balance, and every running-balance transition are deterministic prerequisites. Any issue emits `Incomplete` and returns before Editing. | `src/engine/workflow.rs`, `src/app/runtime.rs`. | Exact ledger passes; missing row/page/column, duplicate, low confidence, and financial mutations fail closed. | PASS |
| EXT-005 | Local OCR remains backward-deserializable but is not a supported v1 PDF parser, is absent from the customer selector, and legacy/programmatic requests receive precise deferred guidance. | `src/app/config.rs`, `src/app/modals.rs`, `src/app/runtime.rs`. | Configuration compatibility and supported-parser regressions; all-target GUI/runtime compilation. | PASS |
| EXT-006 | Deterministic math is mandatory before rendering, during visual verification, and after configured output reparse. Missing rows, zero/partial evidence, mismatched closing values, and mutation-count differences fail. Optional AI is advisory only. | `src/engine/workflow.rs`, `src/engine/verification.rs`, `src/app/runtime.rs`. | Zero-opening, missing-evidence, closing-mismatch, cross-page cascade, stale edit, malformed money, and final-output mutations are regression locked. | PASS |
| EXT-007 | Invalid date input no longer substitutes hardcoded dates; malformed shift/remap values disable submission with field guidance. Runtime no-op semantics remain explicit rather than fabricated success. | `src/app/modals.rs`, existing typed runtime dispositions. | Date-adjust and all-target tests; hardcoded fallback removed. | PASS |
| EXT-008 | Exact-capacity transfer planning is deterministic and provider-free, preserves document order, validates date ambiguity and editable geometry, recomputes balances, and retains optional provider assistance without making it a prerequisite. | `src/engine/transfer.rs`, `src/app/runtime.rs`. | Provider-free mapping, ambiguity, capacity, geometry, stage, rollback/integrity, and external-key-free integration tests. | PASS |
| EXT-009 | Cloud parsers are attempted only when selected and configured; no unrelated cloud cascade occurs. Unapproved Document AI production use remains explicitly quarantined while existing mock contracts cover parser payload behavior. | `src/app/runtime.rs`, `src/ai/document_ai.rs`, `src/ai/llamaparse.rs`. | Provider-order table and parser unit/mock tests; offline and headless paths make no cloud call. | PASS |
| EXT-010 | The authorized v1 matrix covers representative born-digital multi-page, empty/non-statement, malformed, unbalanced, font-corrupt, subtle-shift, duplicate/ambiguous, and generated synthetic fixtures. Scanned OCR is explicitly unsupported in v1 rather than silently attempted. | `tests/stress_pdfs/`, `tests/fixtures/`, parser and integrity tests. | Versioned representative hash, exact 30-row expectation, mutation cases, and explicit unsupported/failed dispositions. Broader Phase 11 qualification remains mandatory. | PASS |
| EXT-011 | Extraction failures identify provider, row/page, field, confidence/review reason, missing geometry, financial transition, or exact target identity instead of returning generic/empty success. | `src/engine/workflow.rs`, `src/app/runtime.rs`, `src/engine/transfer.rs`. | Message assertions in completeness, zero-row, transfer, and invalid-edit regressions. | PASS |

## Mandatory workflow evidence

| Job | Job ID | Conclusion |
|---|---:|---|
| rustfmt | `91400984007` | PASS |
| Clippy production surfaces | `91400983991` | PASS |
| Base state — Ubuntu | `91400984073` | PASS |
| Base state — macOS 14 / Apple Silicon | `91400984011` | PASS |
| Base state — Windows | `91400984016` | PASS |
| Optional PyMuPDF Pro import — macOS | `91400984024` | PASS |
| Optional PyMuPDF Pro import — Windows | `91400984018` | PASS |
| P0 integrity regressions — Ubuntu | `91403264733` | PASS |
| P0 integrity regressions — macOS | `91403264730` | PASS |
| P0 integrity regressions — Windows | `91403264734` | PASS |
| Deferred hardening inventory | `91400984020` | Expected advisory failure; non-blocking until Phase 12 |

## Local qualification

| Check | Evidence | Result |
|---|---|---|
| Complete Rust library suite | 285 passed, 0 failed | PASS |
| Integrity regressions | 5 passed, 0 failed | PASS |
| Provider-free transfer integration | 1 passed, 0 failed | PASS |
| Strict Clippy | `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Real offline router | Representative two-page fixture produced 30 canonical, confidence-qualified rows | PASS |
| Bounded batch extraction | Two nested inputs with concurrency 2 produced two 30-row JSON outputs and a complete manifest | PASS |
| Financial cascade/formatting | Cross-page exact balances, materialized downstream cells, and 16 locale-format regressions | PASS |

## Gate invariants

| Invariant | Evidence | Result |
|---|---|---|
| No empty success | Zero-row statement emits explicit failure; no `[]` or missing artifact success. | PASS |
| Selected provider is authoritative | Router plan contains only selected provider and qualified offline fallback. | PASS |
| Canonical evidence survives serialization | New metadata round-trips; legacy JSON remains readable and normalizes deterministically. | PASS |
| Incomplete ledger cannot enter Editing | Structural, geometry, confidence, review, and financial issues cause terminal `Incomplete`. | PASS |
| Math is deterministic | AI cannot approve balances; required evidence, closing balance, and final reparse are exact gates. | PASS |
| Cascade matches rendered intent | Stale or malformed edits fail; every downstream running-balance cell is materialized. | PASS |
| Provider-free transfer works | Exact supported transfer does not require AI configuration. | PASS |
| Batch has no silent truncation | Every discovered PDF has one manifest row and one explicit success/failure result. | PASS |
| Prior guarantees retained | Gate 04 protocol, atomic output, audit, and P0 integrity pass on Windows, macOS, and Linux. | PASS |

## Residual work

Gate 05 does not authorize release. Scanned local OCR remains outside v1 until a controlled model package and deterministic corpus exist. Optional cloud integrations remain quarantined unless their approved provider contracts pass broader Phase 11 qualification. Exact PDF operator targeting, CTM/rotation, segmentation, font fidelity, generic reconstruction removal, and pinned/checksummed Pdfium distribution are Phase 06. Independent verifier expansion is Phase 07. Release publication remains frozen.

## Evidence files

| File | Purpose |
|---|---|
| `candidate.tsv` | Base SHA, candidate SHA, branch, and workflow identity. |
| `ci-run.json` | Complete public workflow-run response. |
| `ci-jobs.json` | Complete public job and step response. |
| `ci-summary.tsv` | Concise job outcomes and URLs. |
| `commits.tsv` | Ordered Phase 05 commit ledger. |
| `changed-paths.tsv` | Name/status change scope. |
| `change-stat.txt` | Phase diff statistics. |
| `local-summary.tsv` | Concise local qualification outcomes. |
| `batch-manifest.json` | Per-file bounded batch extraction evidence. |
| `fixture-sha256.txt` | Representative fixture identity. |
| `SHA256SUMS` | Evidence-file checksums. |

## Advancement

Gate 05 is closed. The next executable phase is **Phase 06 — exact PDF editing, geometry, fonts, segmentation, and atomic output**. No later phase may weaken canonical ledger identity, selected-provider routing, deterministic completeness, confidence review, provider-free transfer, or strict financial acceptance.
