# Gate 03 Evidence Manifest

## Decision

**Gate:** 03 — unified runtime state, jobs, storage, and recovery

**Decision:** **PASS**

**Base SHA:** `8688ebb6281d8bf0c8a286d3a1f0a9991e5da20e`

**Verified candidate SHA:** `c84c631e62590a366608b9dd2d9c20c476568f78`

**Workflow run:** [30698338534](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30698338534)

**Branch:** `remediation/phase-03-unified-runtime`

**Default branch modified:** No

**Release publication:** Frozen

The candidate satisfies Gate 03: runtime jobs have stable identity and job-scoped routing, terminal delivery is exactly once, cancellation and timeout are bounded, headless and interactive fallback are explicit, workspaces are collision-free, configuration and capability state have one owner, and the complete Windows/macOS/Linux P0 suite remains green.

## Ticket evidence

| Ticket | Implemented outcome | Principal implementation evidence | Regression or gate evidence | Result |
|---|---|---|---|---|
| RUN-001 | One authoritative workflow state/event model replaced duplicate and disconnected application state machines. Illegal transitions are rejected. | `src/engine/workflow.rs`, `src/app/gui.rs`, `src/lib.rs`; retired `src/app/workflow_state.rs`, `src/app/e2e_tests.rs`, and `src/app/gui_state.rs`. | Exhaustive state/event tests in `src/engine/workflow.rs`; all host library suites passed. | PASS |
| RUN-002 | Every submitted job receives a job ID, hashed document ID when applicable, correlation ID, deadline, execution mode, cancellation token, job-scoped sink, and exactly-one terminal barrier. Timeout/cancel monitors suppress stale results. | `src/app/runtime.rs` (`JobMetadata`, `JobEnvelope`, `ResultSink`, `TerminalTracker`, lifecycle monitors). | Runtime metadata, terminal, cancellation, timeout, and silent-drop regressions; all base-state and P0 jobs passed. | PASS |
| RUN-003 | `RuntimeClient` and private `JobTicket` routes prevent shared-receiver result theft or cross-talk. Server readiness uses its own ticket. | `src/app/runtime.rs`, `src/app/server.rs`, migrated CLI/GUI/test consumers. | Routed identity/isolation test and real server E2E test; Windows/macOS/Linux passed. | PASS |
| RUN-004 | Shutdown closes intake, requests cancellation, waits within a bound, records audit state, drains the runtime, and flushes telemetry. Fixed-delay process exit was removed. | `src/app/runtime.rs`, `src/main.rs`. | Shutdown primitive regressions and direct executable lifecycle in each host base-state job. | PASS |
| RUN-005 | Document AI LRO, interactive fallback, and runtime jobs are bounded. CLI/server submissions are explicitly headless and cannot wait for GUI input or silently change cloud providers. Caller no-offline-fallback intent is honored. | `src/ai/document_ai.rs`, `src/app/runtime.rs`, `src/app/cli.rs`, `src/app/server.rs`. | Timeout, stale-route cleanup, execution-mode, and fallback-policy regressions; all host suites passed. | PASS |
| RUN-006 | One platform application root and collision-free per-document/per-run workspaces own drafts, cache, verification, audit, output, temp, and support artifacts. | `src/app/paths.rs`, `src/app/gui.rs`, `src/app/modals.rs`, `src/main.rs`. | Workspace collision and working-directory independence tests; Windows/macOS/Linux passed. | PASS |
| RUN-007 | One truthful capability registry distinguishes ready, configured-but-unverified, and unavailable dependencies; backend choices are disabled with explicit reasons. Pdfium readiness probing never downloads. | `src/app/capabilities.rs`, `src/pdf/native_engine.rs`, `src/app/gui.rs`, `src/app/modals.rs`. | Capability matrix tests, GUI compilation/tests, optional Pro smoke on Windows/macOS. | PASS |
| RUN-008 | Decorative v1 remote-engine configuration and claims were removed; local processing is the truthful v1 contract. | `src/app/config.rs`, `src/lib.rs`, `README.md`, `docs/ALL_DOCUMENTATION.txt`. | Repository search plus all-target compilation proves no public `ConnectionMode`/remote engine surface remains. | PASS |
| RUN-009 | `Succeeded`, `NoOp`, `Partial`, `Failed`, `Cancelled`, and `TimedOut` have one serialized runtime/GUI/CLI contract. Date adjustment stages all edits, rejects zero/partial application, atomically publishes exact output, and verifies the requested artifact before exit 0. | `src/app/runtime.rs`, `src/app/cli.rs`, `src/app/gui.rs`. | Table-driven terminal classification; final host library suites and P0 regressions passed. | PASS |
| RUN-010 | Runtime execution carries structured job/document/correlation fields. Daily app/error logs have bounded retention. Support reports include only a bounded re-scrubbed tail. | `src/app/runtime.rs`, `src/app/telemetry.rs`. | Correlated routed-job privacy regression; retention and support-tail scrubbing tests; all host library suites passed. | PASS |

## Mandatory workflow evidence

| Job | Job ID | Conclusion |
|---|---:|---|
| rustfmt | `91364889553` | PASS |
| Clippy production surfaces | `91364889590` | PASS |
| Base state — Ubuntu | `91364889613` | PASS |
| Base state — macOS 14 / Apple Silicon | `91364889606` | PASS |
| Base state — Windows | `91364889578` | PASS |
| Optional PyMuPDF Pro import — macOS | `91364889562` | PASS |
| Optional PyMuPDF Pro import — Windows | `91364889583` | PASS |
| P0 integrity regressions — Ubuntu | `91366292772` | PASS |
| P0 integrity regressions — macOS | `91366292775` | PASS |
| P0 integrity regressions — Windows | `91366292780` | PASS |
| Deferred hardening inventory | `91364889593` | Expected advisory failure; non-blocking until the final hardening phase |

The workflow’s overall conclusion is `success`. The advisory inventory remains intentionally non-blocking under the approved base-state-first sequence; its known dependency findings remain assigned to Phase 12 and do not alter any functional gate result.

## Gate invariants

| Invariant | Evidence | Result |
|---|---|---|
| Exactly one terminal result per job | Shared terminal flag in `ResultSink`; duplicate/post-terminal suppression; lifecycle regressions | PASS |
| Bounded cancellation and timeout | Per-job deadline/token monitor; GUI tracks exact active job ID; headless jobs cannot await UI | PASS |
| Concurrent result isolation | Private job tickets plus hashed document/correlation identity; server readiness no longer drains a shared stream | PASS |
| Deterministic storage | Platform root and UUID run workspace; document identity is independent of working directory | PASS |
| Atomic configuration ownership | Generation-tracked immutable `ConfigManager` snapshots; failed reload preserves prior generation | PASS |
| Truthful capabilities | One registry; unavailable actions disabled; safe non-downloading probes | PASS |
| Standard terminal semantics | Serialized dispositions and verified artifact postconditions | PASS |
| Bounded privacy-safe diagnostics | Structured IDs, scrubbing, 14-day/28-file-per-stream retention, 64 KiB/50-line support tail | PASS |
| Prior integrity guarantees retained | Complete Windows/macOS/Linux P0 matrix | PASS |

## Migration and compatibility record

The active working directories move from process-relative paths into the platform application root. Existing process-relative draft or output files are not deleted or overwritten; automatic legacy-draft discovery is intentionally deferred to the later recovery UX phase. The stale `ConnectionMode`, `EnhancedConfig`, and parallel `config_v2` surfaces were removed from the remediation branch. `REMOTE_ENGINE_URL` is no longer a supported v1 contract. Runtime completion consumers must use the structured `JobCompleted` disposition and artifact fields rather than the former label-only success message.

## Security, privacy, and external-system record

No new credential, paid-provider, or real-statement dependency was introduced. Routine structured logs contain hashed document identity rather than a source path, and support diagnostics are re-scrubbed and bounded before inclusion. Cloud provider fallback is explicit; headless callers cannot silently switch providers. The existing advisory dependency inventory remains recorded for the final hardening phase.

## Residual risks and follow-on work

The full Python/PyMuPDF process protocol, crash/restart behavior, memory/handle pressure, cross-language schema corpus, and self-contained runtime distribution are intentionally owned by Phase 04. Legacy process-relative drafts remain untouched and require an explicit later import/recovery UX. Release publication remains frozen. No signed or customer-facing artifact is authorized by this gate.

## Evidence files

| File | Purpose |
|---|---|
| `candidate.txt` | Base/candidate SHA and workflow identity |
| `ci-run.json` | Complete public workflow-run response |
| `ci-jobs.json` | Complete public job/step response |
| `ci-summary.tsv` | Concise job outcomes and URLs |
| `commits.tsv` | Ordered Phase 03 commit ledger |
| `changed-paths.tsv` | Name/status change scope |
| `change-stat.txt` | Phase diff statistics |
| `SHA256SUMS` | Evidence checksums |

## Advancement

Gate 03 is closed. The next executable phase is **Phase 04 — Permanent Python/PyMuPDF pipeline fortification**, beginning with the versioned cross-language operation schema and worker lifecycle contract. No later phase may weaken the Gate 02 P0 matrix or Gate 03 job, cancellation, routing, storage, configuration, capability, disposition, or diagnostic invariants.
