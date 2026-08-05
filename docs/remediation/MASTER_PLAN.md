# Bank Statement Fidelity Editor — Ticketed, Verified Remediation Execution Plan

**Author:** Manus AI
**Source audit:** 57 verified findings at revision `41993a8daf73266eaae5d6d4abcc2cc13ac85662`
**Execution rule:** No phase advances until its blocking gate passes and its evidence is committed or attached to the phase pull request.

> This plan is sequenced by dependency and risk, not by convenience. It first creates a trustworthy Windows/macOS development baseline and closes all critical data-loss and false-success paths. It then consolidates runtime contracts, bulletproofs the permanent Python/PyMuPDF pipeline, repairs extraction and PDF editing, rebuilds independent verification, evaluates an optional local LLM, finishes the GUI, and packages the application. Non-blocking privacy and defense-in-depth work is deliberately completed after functional qualification, followed by a full final rerun. Critical data-integrity and active security blockers are never deferred. A failed gate returns work to the responsible ticket; it is never waived by changing a threshold or disabling a test.

## 1. Execution and Git protocol

### 1.1 Branch and ticket flow

| Step | Required action | Evidence |
|---:|---|---|
| 1 | Create one issue per ticket with severity, finding IDs, source evidence, expected behavior, regression risks, and exact acceptance tests. | Issue URL or versioned Markdown ticket. |
| 2 | Branch from the latest passing phase branch using `remediation/<phase>/<ticket>-<slug>`. Never work directly on `master`. | Branch SHA recorded in ticket. |
| 3 | Reproduce the defect and add a failing regression test before implementation, except for pure documentation/repository-governance tickets. | Red test output or documented non-code baseline. |
| 4 | Implement the narrowest coherent fix. Do not combine unrelated cleanup or feature work. | Reviewed diff and change note. |
| 5 | Run ticket-local format, lint, unit, contract, and integration checks. | Command/status table attached to ticket. |
| 6 | Re-run all earlier P0 regressions and any affected lifecycle suite. | Regression log. |
| 7 | Open a pull request with risk, migrations, before/after behavior, test evidence, and rollback path. | Pull request. |
| 8 | Merge only after required checks and review pass. Delete the ticket branch after merge. | Merge SHA. |
| 9 | At phase end, run the complete phase gate from a clean checkout and publish an evidence manifest. | `docs/remediation/evidence/phase-XX/manifest.md` plus CI run links. |

### 1.2 Ticket state model

`Planned → Reproduced → Test Added → Implemented → Ticket Verified → Phase Verified → Merged → Closed`

A ticket may enter `Blocked` only with a written blocker, attempted alternatives, security/data impact, and a decision request. It may not be marked complete because a dependency, credential, fixture, or provider is unavailable.

### 1.3 Verification hierarchy

| Level | Runs when | Minimum checks |
|---|---|---|
| Ticket-local | Every implementation iteration | Formatter, affected lints, focused unit/contract tests, negative and boundary cases. |
| Domain regression | Before ticket PR | Full affected module tests, all prior P0 regressions, relevant CLI/runtime/GUI lifecycle. |
| Phase gate | After all tickets in phase | Clean build, hard CI checks, complete phase suites, evidence/artifact validation, source diff and secret scan. |
| Release gate | After all phases | Mandatory Windows and macOS tests, Linux development-CI checks, clean-machine package smoke, authorized corpus, fault injection, performance, accessibility, final security/privacy, SBOM/provenance, and signed artifacts. |

## 2. Severity and closure policy

| Severity | Meaning in this program | Closure rule |
|---|---|---|
| **Critical / P0** | Reachable data loss, destructive false success, falsified financial result, or mutable historical evidence. | Fixed and regression-locked before architectural or feature work. No waiver. |
| **Major / P1** | Core correctness, reliability, security, build, verification, packaging, or product-truth failure. | Zero open P1 before release candidate. No waiver at general availability. |
| **Medium / P2** | Secondary workflow, accessibility, maintainability, operational consistency, or incomplete user surface. | Fix, or remove the affected surface and every claim before release. |
| **Minor / P3** | Naming, copy, metadata, dead code, developer experience, low-risk consistency, or cosmetic polish discovered during repair. | Closed in the owning phase or the final P3 sweep. No visible known P3 remains at general availability. |

## 3. Phase 00 — Access, scope, governance, and release freeze

**Purpose:** Prevent uncontrolled changes or premature releases while converting the audit into an executable backlog.

| Ticket | Sev. | Work | Depends on | Ticket verification |
|---|---:|---|---|---|
| GOV-001 | P1 | Confirm write access, author identity, merge authority, branch model, and whether GitHub Issues/PR APIs or deploy-key-only Git will be used. | User decisions | Authenticated dry-run fetch; push/delete a temporary non-default branch; no modification to `master`. |
| GOV-002 | P1 | Record accepted ADRs: permanent Python/PyMuPDF production pipeline; mandatory Windows and macOS support; benchmark-gated optional local LLM; critical-integrity-first sequencing with non-blocking privacy hardening last. Record remaining provider, OCR, licensing, signing, and feature-disposition decisions as explicit open ADRs. | Questionnaire and owner directives | Accepted ADRs are versioned; every unresolved architecture decision has an owner, default, deadline, and blocking phase. |
| GOV-003 | P2 | Create labels, milestones, issue templates, ticket template, phase project, severity definitions, and evidence manifest schema. | GOV-001 | One sample ticket validates required fields and automated checks. |
| GOV-004 | P0 | Freeze release automation and add a pre-production warning until Phase 10. Remove public “production-ready,” “100% fidelity,” and unsupported security claims immediately. | GOV-001 | Release workflow cannot publish; README/UI banner accurately states current status. |
| GOV-005 | P2 | Preserve audit artifacts, source hashes, current head, dependency lockfile, fixture hashes, and baseline commands in `docs/remediation/baseline/`. | GOV-001 | Hash manifest verifies; no private statement or secret is committed. |

**Gate 00:** Access works with least privilege; scope decisions are approved; release publication is blocked; the backlog and evidence format exist; source and fixtures are hashed.

## 4. Phase 01 — Reproducible baseline, CI, and product truth

**Purpose:** Make every later fix measurable and stop false-green CI. This phase changes infrastructure and truthful claims, not financial behavior.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| BASE-001 | P1 | OPS-03 | Remove the global Windows target/compiler path; target-scope platform dependencies; align Rust 1.89, Python, PyO3, and native prerequisites across mandatory Windows/macOS builds and Linux development CI. | Clean `cargo check --all-targets` on Windows and macOS plus Linux development CI; no host-specific repository edit required. |
| BASE-002 | P1 | QA-07 | Make format, Clippy, compile, unit/integration, executable smoke, and minimum critical-path coverage jobs blocking. Add concurrency cancellation for superseded CI runs. Keep dependency/supply-chain reports advisory until Phase 12, except any active credential exposure or data exfiltration blocks immediately. | Deliberately broken build/test/lint branch is rejected; green branch passes; deferred hardening reports remain visible and ticketed. |
| BASE-003 | P1 | QA-03, QA-06 | Classify tests as unit, contract, integration, E2E, live-provider, GUI, accessibility, performance, and package smoke. Rename no-panic suites and replace self-skip with explicit CI disposition. | CI report states executed/skipped counts and reasons; core E2E never silently self-skips. |
| BASE-004 | P2 | QA-04 | Either implement the Node visual test command and pin its dependencies or remove the unused package/workflow claims. | `npm test` passes deterministically or Node is absent from the supported toolchain. |
| BASE-005 | P1 | QA-01 | Separate component health from full readiness. Fix `doctor`/`selftest` result schemas so failed Python/provider initialization cannot appear ready. | Matrix tests cover missing Python, missing Pro key, missing Pdfium, offline mode, and full configured mode. |
| BASE-006 | P2 | OPS-10, OPS-11 | Parse `--help`/`--version` before configuration; formalize exit codes and machine-readable errors across every CLI command. | Contract suite covers all subcommands, invalid input, missing file, config failure, no-op, partial, cancellation, and success. |
| BASE-007 | P1 | QA-05, QA-08 | Source version from Cargo metadata; reconcile product name, bundle ID, README, quickstart, environment template, CLI, UI, Docker, and release metadata. Replace universal fidelity/security claims with evidence-scoped language. | One version/capability matrix; stale-version and prohibited-claim search returns zero. |
| BASE-008 | P2/P3 | Repository hygiene | Remove tracked generated binaries, logs, outputs, coverage, scratch artifacts, stale one-off scripts, and duplicate docs; strengthen ignore rules without hiding required fixtures. | Clean clone and test leave `git status` clean; secret/artifact scan passes. |
| BASE-009 | P3 | Developer experience | Add one documented bootstrap command, one verification command, contribution workflow, supported tool versions, and failure troubleshooting. | New clean environment reaches the same baseline from docs only. |

**Gate 01 commands:** `cargo fmt --all -- --check`; strict cross-platform Clippy; all currently supportable tests; executable smoke; CLI contract suite; Python import/runtime smoke; and clean-tree check. Dependency, privacy, and supply-chain reports remain visible but non-blocking until Phase 12 unless they identify an active exposure. The gate must pass on a clean clone before Phase 02.

## 5. Phase 02 — Critical integrity, exact success, and immutable history

**Purpose:** Close all five P0 findings and related success-contract defects before broad refactoring.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| INT-001 | P0 | COR-02, COR-05 | Define a typed `ApplyReport` shared by Rust and Python: requested, matched, placed, failed, warnings, methods, review flags, source/output hashes, and per-edit evidence. Reject unknown/incomplete payloads. | Schema round-trip and malformed/partial-result tests; runtime refuses every count mismatch. |
| INT-002 | P0 | COR-02 | Fix Python no-overlap behavior: do not publish destructive redaction without verified replacement placement; report the exact failed edit and preserve source output. | Existing isolated no-text reproduction now fails non-destructively; requested text presence and source-region preservation are asserted. |
| INT-003 | P0 | COR-03 | Replace one-job-per-change writes with one ordered document transaction using a unique scratch file and one atomic final commit. | Twenty edits apply exactly once under repeated and concurrent attempts; no last-writer-wins loss. |
| INT-004 | P1 | COR-04 | Remove keystroke-triggered fire-and-forget full-PDF writes or add debounce, serialization, cancellation, job identity, visible state, and terminal error handling. | Rapid typing test produces one final durable edit set and no overlapping writer. |
| INT-005 | P0 | COR-01 | Convert every preview auto-correction into explicit proposed edits; bind preview hash to the exact approved edit set; disable finalization when that set is unbalanced or stale. | Preview, persisted edits, rendered output, and independent reparse produce identical ledger values. |
| INT-006 | P0 | COR-16, COR-20 | Treat zero extracted rows as incomplete unless explicitly valid-empty; require balance prerequisites and the requested output artifact before success. | Repository two-page fixture exits nonzero with diagnostics; no `[]` success and no missing-artifact success. |
| INT-007 | P0 | AUD-01 | Replace hard-link snapshots with immutable content-addressed copies; record and verify hash, size, parent, and creation metadata. | Rewriting live output cannot alter snapshots; tamper and missing-object tests fail before undo/replay. |
| INT-008 | P1 | AUD-02 | Add one commit barrier: output validation, audit records, immutable snapshot, history update, and atomic publication must all succeed before terminal success. | Fault injection at each step yields one failure, no success, and the prior valid output remains. |
| INT-009 | P0 | All P0 | Build a dedicated P0 regression suite and require it on every PR. | Five original reproductions fail on baseline commit and pass on repaired branch; mutation tests prove assertions are meaningful. |
| INT-010 | P3 | Result/error copy | Standardize user-facing messages for destructive prevention, stale preview, empty extraction, and commit rollback. | Snapshot/UI tests confirm actionable, non-misleading copy. |

**Gate 02:** The five P0 reproductions pass on mandatory Windows and macOS runners and the Linux development runner. No success is emitted without exact edit accounting, a durable requested artifact, deterministic balance evidence, independent validation, complete audit persistence, and immutable history.

## 6. Phase 03 — Unified runtime state, jobs, storage, and recovery

**Purpose:** Establish one protocol that every GUI, CLI, batch, and future service adapter must use.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| RUN-001 | P2 | UX-06 | Select one authoritative workflow state machine; migrate tests and UI; delete or archive duplicate models and disconnected `current_view` state. | Model-based tests cover every state/event pair and reject illegal transitions. |
| RUN-002 | P1 | OPS-07, COR-13 | Introduce a job envelope with job/document/correlation IDs, deadline, cancellation, and exactly one typed terminal result. Protect every handler against panic/cancellation loss. | Generated test iterates every `Job` variant through success/error/cancel/panic and observes exactly one terminal result. |
| RUN-003 | P2 | OPS-06 | Route results by job ID instead of consuming a shared receiver. Readiness/progress may not discard another job’s result. | Concurrent ping, render, parse, and edit test shows no loss, cross-talk, or hang. |
| RUN-004 | P1 | OPS-01 | Implement graceful shutdown: stop intake, cancel, await workers, roll back/commit writes, flush audit/telemetry, and exit with correct status. | Forced shutdown at each lifecycle stage leaves only a prior valid or fully committed output. |
| RUN-005 | P1 | OPS-02, COR-15 | Apply bounded deadlines/retries to Document AI LRO, interactive fallback, and provider calls; honor caller fallback policy. | Never-completing mock provider times out deterministically; interactive and headless policies differ explicitly. |
| RUN-006 | P2 | OPS-08, OPS-09 | Centralize a platform application root with per-document/run workspaces for drafts, cache, verification, audit, output, temp, and support bundles. | Two instances and two documents execute concurrently without path collision; history is independent of working directory. |
| RUN-007 | P1 | COR-09, OPS-04, QA-01 | Build a capability registry with tested probes for Python, Pro license, Pdfium, OCR models, cloud providers, and writable storage. Disable unavailable actions. | Capability matrix tests; no backend is selectable or reported ready when dependencies are absent. |
| RUN-008 | P2 | OPS-05 | Remove remote-engine claims/configuration from v1, or isolate a separately designed authenticated service. No decorative remote badge remains. | Search and UI tests show no remote claim unless a real processing contract is present. |
| RUN-009 | P1/P2 | COR-17, OPS-11 | Standardize `Succeeded`, `NoOp`, `Partial`, `Failed`, `Cancelled`, and `TimedOut` across CLI/GUI/runtime; require artifacts and postconditions. | Date no-op and missing output return explicit non-success dispositions. |
| RUN-010 | P3 | Observability | Add structured local logs by job/document ID, bounded log retention, and privacy-safe support diagnostics. | Correlation test follows one job end to end without exposing statement data or secrets. |

**Gate 03:** Every job terminates exactly once; cancellation and timeout are bounded; concurrent operations are isolated; storage paths are deterministic; the full P0 suite remains green.

## 7. Phase 04 — Permanent Python/PyMuPDF pipeline fortification

**Purpose:** Preserve Python and PyMuPDF as permanent first-class production components while making every process, protocol, memory, packaging, error, and recovery boundary deterministic and independently verifiable.

| Ticket | Sev. | Work | Ticket verification |
|---|---:|---|---|
| PY-001 | P0 | Define versioned Python request/response schemas for every operation, including operation ID, input/output hashes, exact per-edit evidence, typed failures, warnings, timing, capability tier, and protocol version. Generate or validate the Rust models from the same schema. | Cross-language golden fixtures, malformed/unknown-field tests, and compatibility tests reject every ambiguous or partial payload. |
| PY-002 | P0 | Make the Python worker lifecycle explicit: deterministic initialization, health/capability handshake, serialized library access, bounded queue, operation deadlines, cancellation, crash detection, clean restart, and exactly-one terminal result. | Kill, hang, panic, malformed stdout, broken pipe, cancellation, and restart tests preserve the prior valid output and terminate predictably. |
| PY-003 | P0 | Apply every Python mutation to a unique per-operation scratch file; validate and flush it; let Rust independently verify and atomically publish it. Python must never rewrite the live output in place. | Disk-full, permission, process-kill, and simultaneous-document fault injection never exposes a partial or cross-contaminated PDF. |
| PY-004 | P1 | Pin a supported matrix for Python, PyO3, PyMuPDF, PyMuPDF Pro, fontTools, Pillow, and native libraries; add runtime compatibility and license-tier probes. | Matrix CI reports exact versions/capabilities and blocks unsupported combinations or false Pro readiness. |
| PY-005 | P1 | Remove import-time side effects, global mutable document state, unbounded caches, leaked handles, and exception swallowing. Add deterministic document/context cleanup. | Repeated 1/10/50/100-page loops show bounded handles and memory; injected exceptions preserve typed context and clean resources. |
| PY-006 | P1 | Harden page, bbox, text, font, encoding, path, size, encryption, malformed-PDF, and decompression validation before expensive work. | Property/fuzz corpus produces bounded typed failures with no panic, path escape, uncontrolled allocation, or silent fallback. |
| PY-007 | P1 | Create a Python contract-test harness independent of the Rust GUI, with golden fixtures for rendering, extraction, exact text placement, fonts, rotations, segmentation, no-op, ambiguity, and failures. | Harness passes locally and in packaged Windows/macOS CI; every original Python reproduction is regression-locked. |
| PY-008 | P1 | Add structured operation observability: stage, operation ID, duration, memory/handle counters, retry, and typed error class. Keep statement content and credentials out of routine logs. | One operation is traceable end to end; log tests detect missing stages and prohibited payload fields. |
| PY-009 | P1 | Build deterministic embedded/distributed Python runtimes for Windows x64 and macOS Apple Silicon, including required modules, libraries, license notices, and capability manifest. | Fresh machines run the Python contract suite without a developer Python installation or network dependency. |
| PY-010 | P1 | Add upgrade, rollback, cache invalidation, bytecode/native-ABI compatibility, and runtime self-repair or reinstall diagnostics for the bundled Python environment. | Broken/mismatched runtime is detected before document work; upgrade failure rolls back to the previous verified runtime. |
| PY-011 | P2/P3 | Document the supported Python extension surface, schema evolution rules, debugging workflow, package inventory, and release checklist; remove stale one-off bridge scripts and duplicate implementations. | Documentation replay and dead-code/script inventory pass from a clean checkout. |

**Gate 04:** The standalone and Rust-integrated Python contract suites pass on Windows, macOS, and Linux development CI. Worker crash, timeout, cancellation, malformed response, memory/handle pressure, missing license, disk-full, and packaging tests fail safely. No Python success can publish output without exact typed evidence and independent Rust verification.

## 8. Phase 05 — Extraction completeness and financial correctness

**Purpose:** Make the semantic ledger trustworthy before further fidelity work.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| EXT-001 | P1/P2 | Semantic foundation | Define a canonical transaction model with exact decimal/minor-unit money, currency, locale, sign, date, row identity, page geometry, provenance, confidence, and review status. | Serialization/property tests preserve every field without lossy normalization. |
| EXT-002 | P1 | COR-15 | Implement one parser strategy/router that honors selected backend, fallback policy, capability, deadline, and headless/interactive mode. | Table-driven tests cover every backend and fallback edge. |
| EXT-003 | P0/P1 | COR-16 | Repair offline extraction on representative born-digital fixtures; require exact row count and required fields. | The original failing fixture matches versioned expected JSON; removed/malformed rows fail completeness. |
| EXT-004 | P1 | COR-08 | Enforce completeness, row/page coverage, opening/closing balance, running-balance continuity, required fields, and confidence gates. Route low confidence to explicit human review. | Missing row/page/column, duplicate row, low confidence, and inconsistent math cannot enter Editing automatically. |
| EXT-005 | P1 | COR-09 | Complete local OCR with controlled model installation and deterministic tests, or remove it from v1. | Scanned fixture passes with models; without models the action is disabled with precise guidance. |
| EXT-006 | P1 | COR-06 | Centralize deterministic financial invariants before preview and after render; AI may explain but never decide correctness. | Property and mutation tests catch amount, sign, sequence, opening, running, and closing changes. |
| EXT-007 | P2/P1 | COR-14, COR-17 | Validate date inputs strictly; remove hardcoded fallback dates; distinguish no-op; prove every intended record changed exactly once. | Invalid date blocks; zero-record operation is `NoOp`; transformed fixture has expected dates and unchanged unrelated content. |
| EXT-008 | P1/P2 | Transfer lifecycle | Make transfer extraction/mapping/review/commit use the same typed contracts, deadlines, balance gates, and audit trail. | Deterministic source/target fixtures cover ambiguity, duplicates, provider absence, cancellation, and rollback. |
| EXT-009 | P1 | Cloud parser boundaries | Add deterministic mock-server contracts for required cloud parsers and version administration; quarantine unapproved integrations. | Authentication, rate limit, malformed payload, timeout, privacy, and schema-drift tests. |
| EXT-010 | P1 | Fixture matrix | Create authorized synthetic/redacted born-digital, scanned, hybrid, sparse, dense, multi-page, locale, duplicate-description, and malformed fixtures. | Expected ledger and failure disposition are versioned; no private statement enters Git or CI artifacts. |
| EXT-011 | P3 | Error guidance | Provide field/row/page-specific extraction and financial-review messages. | Snapshot tests and human-review checklist pass. |

**Gate 05:** Every supported fixture extracts the exact expected ledger or fails explicitly; every injected financial mutation fails; offline and configured cloud modes have deterministic contracts; no empty/incomplete/no-op result masquerades as success.

## 9. Phase 06 — Exact PDF editing, geometry, fonts, segmentation, and atomic output

**Purpose:** Ensure the editor changes exactly one intended target while preserving all unrelated document content.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| PDF-001 | P1 | COR-05, COR-10 | Apply the shared typed edit contract to every Python/native/Pro operation; require exact requested/matched/placed counts. | Cross-engine contract suite rejects zero, partial, duplicate, and over-applied results. |
| PDF-002 | P1 | COR-10 | Correct page/text transformation handling: CTM, text matrix, rotation, crop/media boxes, direction, origins, and coordinate conversions. | Rotated/transformed fixtures alter only the selected target at measured geometry. |
| PDF-003 | P1 | COR-10 | Detect duplicate/ambiguous text operators and require stable target identity rather than origin-only matching. | Duplicate descriptions at same/near origins produce one explicit selection or a review block, never multiple replacements. |
| PDF-004 | P1/P2 | Font fidelity | Make font extraction, subset coverage, glyph availability, fallback, embedding, and substitution explicit; remove dead synthesis claims. | Unicode/subset/missing-glyph tests; unsupported glyph blocks or uses approved disclosed substitution. |
| PDF-005 | P1 | Segmentation | Prove global/local page mapping, edit membership, page order, metadata/page boxes, boundary edits, retry, and merge atomicity. | Boundary and interruption matrix preserves page count/order and exact edit membership. |
| PDF-006 | P1 | OPS-04 | Replace malformed Pdfium auto-download with a pinned, checksummed, licensed installation strategy or bundle/system discovery. | Clean host installs/verifies Pdfium; corrupt/wrong-platform artifact fails closed. |
| PDF-007 | P1 | COR-11, COR-18 | Remove Typst from automatic fidelity fallback. If retained, rename as a separate reconstruction export with explicit non-fidelity warning and complete content/page requirements. | No fidelity workflow can reduce a two-page statement to a generic one-page output and report success. |
| PDF-008 | P2 | COR-12 | Remove or correctly implement mislabeled Pdfium/Typst alternatives; preserve true engine identity and error evidence. | Each presented alternative is produced by the named engine and validates page/edit mapping. |
| PDF-009 | P1 | Output integrity | Standardize same-filesystem temp write, validation, flush, atomic replace, and previous-output preservation across every transformation. | Disk-full, permission, crash, and cancellation fault injection never publishes partial output. |
| PDF-010 | P2/P3 | Malformed-input hardening | Add fuzz/property tests for malformed PDFs, huge coordinates, invalid Unicode, encrypted files, missing resources, and decompression limits. | No panic/OOM/path escape; bounded failure and clear unsupported disposition. |

**Gate 06:** Exact edit count and membership pass across all supported engines and fixture geometry. Font/segment/output fault tests pass. Generic reconstruction and mislabeled fallbacks cannot enter the fidelity path. Prior phases remain green.

## 10. Phase 07 — Independent fail-closed verification

**Purpose:** Make verification independent from editor success and capable of proving structural, visual, semantic, and financial postconditions.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| VER-001 | P1 | COR-19 | Repair the standalone local verifier so an identical pair passes and always emits a machine-readable report. | Identical control passes on all supported hosts; known mutation controls fail. |
| VER-002 | P1 | Verification foundation | Add structural checks for openability, page count/order, page boxes, metadata policy, fonts/resources, text/content membership, and forbidden page loss. | One mutation fixture per structural invariant. |
| VER-003 | P1 | COR-05, COR-10 | Add independent requested-edit membership: exact target count, placed text, region, and no unrequested visible change outside approved masks. | Missing, duplicate, over-applied, blanked, and unrelated edits all fail. |
| VER-004 | P1 | COR-07 | Make vision/provider errors and unavailable optional checks explicit `Unavailable`/`Failed`, never pass. Local visual evidence remains mandatory. | Network error, malformed response, missing key, and timeout cannot create a pass. |
| VER-005 | P1 | COR-06 | Independently reparse output and enforce deterministic row count, values, signs, sequence, running balances, and closing balance. | Every financial mutation blocks commit. |
| VER-006 | P1/P2 | Retry policy | Remove permissive high-diff/exhausted-retry acceptance; calibrate renderer and thresholds before candidate evaluation; keep thresholds immutable in a run. | Threshold-widening attempt is rejected; bounded retries preserve failed evidence. |
| VER-007 | P2 | Cloud additive checks | Contract-test any retained pdfRest, Applitools, and Gemini layers; distinguish additive warning from mandatory local gate. | Provider outage does not weaken local acceptance; privacy-safe request tests pass. |
| VER-008 | P1 | Evidence | Persist verifier version/config, renderer calibration, hashes, edit set, scores, overlays, diagnostics, and disposition in a reproducible package. | Failure to persist evidence blocks success; evidence replay reproduces disposition. |
| VER-009 | P1 | Fidelity corpus | Build golden identical, intended-edit, unrelated-change, font drift, geometry drift, page loss, blank replacement, and math mutation controls. | Zero false pass/false fail in the supported corpus. |
| VER-010 | P3 | Reporting polish | Produce concise user report plus deep diagnostics without universal “perfect” claims. | Report language is evidence-scoped and accessible. |

**Gate 07:** Identical controls pass; every known defect mutation fails; no verifier exception, missing dependency, provider outage, retry exhaustion, or evidence-write failure produces a pass.

## 11. Phase 08 — Optional local LLM evaluation and gated integration

**Purpose:** Determine whether an on-device model materially improves offline natural-language and ambiguity-review workflows without becoming a source of financial truth, weakening deterministic gates, or making Windows/macOS packaging unreliable.

| Ticket | Sev. | Work | Ticket verification |
|---|---:|---|---|
| LLM-001 | P2 | Inventory candidate uses, explicit non-goals, typed inputs/outputs, review requirements, and deterministic postconditions. Exclude balance authority, fidelity approval, direct PDF writes, and terminal-success decisions. | Architecture tests and API review prove no LLM path can bypass canonical validation or verification. |
| LLM-002 | P2 | Build a versioned synthetic/redacted benchmark for natural-language edit intent, transaction categorization, error explanation, and parser-ambiguity suggestions. | Expected schemas, accuracy measures, invalid-output rate, and human-review labels are reproducible. |
| LLM-003 | P2 | Research current redistributable local runtimes/models for Windows x64 and macOS Apple Silicon, including license, architecture, quantization, context, JSON constraints, package size, and maintenance. | Evidence-based shortlist and rejection reasons; no model is selected on marketing claims alone. |
| LLM-004 | P1/P2 | Implement a provider-neutral local inference adapter behind the typed job protocol, deadline, cancellation, capability registry, strict JSON schema, and graceful absence. | Malformed output, timeout, model absence, cancellation, and worker failure cannot change a document or create success. |
| LLM-005 | P2 | Benchmark candidates on representative low/mid/high Windows and macOS hardware for accuracy, latency, peak memory, disk/package size, startup, thermal behavior, and fixed-configuration repeatability. | Published benchmark matrix meets owner-approved budgets or produces a no-go decision. |
| LLM-006 | P1/P2 | If approved, package the model/runtime as an optional component with checksum, license, version, capability manifest, install/remove/upgrade/rollback, and offline network-blocked operation. | Clean-machine optional-component lifecycle passes; core app remains fully functional when absent. |
| LLM-007 | P1/P2 | Add adversarial prompt, schema confusion, document-content injection, oversized-input, denial-of-service, and unsupported-language tests. | Unsafe/invalid proposals are rejected before reaching an edit or audit commit. |
| LLM-008 | P2 | Conduct a formal go/no-go review comparing measurable benefit against support, performance, package, and maintenance cost. | Decision and evidence are versioned; failure to meet thresholds results in no shipped local LLM. |
| LLM-009 | P2/P3 | If and only if the gate passes, integrate user controls, status, model management, review UI, diagnostics, documentation, and regression tests. | Every accepted proposal resolves to typed deterministic operations and passes the normal verification/audit pipeline. |

**Gate 08:** A local LLM ships only if it provides measurable benchmark value within approved Windows/macOS budgets, passes strict schema and adversarial tests, operates offline, remains optional, and cannot override deterministic extraction, financial, edit-membership, or fidelity gates. Otherwise the phase closes with a documented no-go decision.

## 12. Phase 09 — Coherent GUI, real batch processing, audit UX, recovery, and accessibility

**Purpose:** Build polish only on verified core contracts and remove every false affordance.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| UX-001 | P2 | UX-06 | Route the GUI exclusively through the authoritative workflow state; remove disconnected navigation/settings models. | Real interaction tests assert every view transition and resulting runtime action. |
| UX-002 | P1 | UX-02 | Implement a real batch queue using the qualified single-document worker: bounded concurrency, per-file states, cancellation, retry, unique outputs, summary export, and failure isolation. | Mixed 20-file batch completes deterministically; one failure does not corrupt or block unrelated files. |
| UX-003 | P2 | UX-03 | Implement Audit Explorer over immutable records, hashes, versions, edit evidence, verification, retention, and export; otherwise remove from v1. | User can inspect and export every version; tamper warning is visible and blocking. |
| UX-004 | P1 | UX-01 | Remove simulated autonomous scraping/training from v1, or build it only under a separately approved automation/data-rights design. | No UI/status/log claims external work that did not occur. |
| UX-005 | P2 | UX-04 | Remove the toast-only Chaos Suite or wire a real controlled test runner with evidence and safety boundaries. | Visible action has a real job, progress, cancellation, terminal result, and report. |
| UX-006 | P2 | COR-13 | Remove the AI Visual Fix stub until a verified implementation exists; no stub command may return success. | Command/UI absent or complete with negative and success E2E. |
| UX-007 | P1/P2 | COR-04, OPS-07 | Redesign progress, cancellation, stale state, retries, errors, no-op, rollback, and last-known-good output around job IDs. | No indefinite spinner; every failure provides one actionable recovery path. |
| UX-008 | P1 | UX-05 | Add accessible names/roles, logical focus order, modal containment, visible focus, non-hover help, keyboard operation, contrast/target sizing, 200% scaling, screen-reader announcements, and test coverage. | WCAG 2.2 AA-informed checklist plus Windows/macOS/Linux keyboard and assistive-technology tests. |
| UX-009 | P2/P3 | Localization and copy | Centralize strings, remove emoji-only semantics, use consistent terminology, and make dates/currency locale-aware without changing canonical values. | Locale, truncation, scaling, and accessible-label snapshots pass. |
| UX-010 | P1/P2 | QA-05, QA-08 | Rewrite onboarding, quickstart, limitations, provider requirements, offline behavior, CLI examples, and evidence-based fidelity language to match the product. | Every documented command is executed in docs CI; capability matrix matches runtime probes. |
| UX-011 | P3 | Visual polish | Normalize spacing, responsive sizing, empty states, confirmations, destructive-action styling, icons, and status hierarchy after accessibility structure is stable. | Screenshot/interaction review on supported OSes and small/high-DPI displays. |

**Gate 09:** The complete critical path works with mouse, keyboard, and assistive technology on Windows and macOS; batch and audit UI use real jobs/evidence; no simulated, dead, or success-returning stub remains visible; prior integrity gates remain green.

## 13. Phase 10 — Self-contained Windows/macOS packaging, deployment, and release engineering

**Purpose:** Turn passing source into installable, truthful customer artifacts.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| PKG-001 | P1 | REL-01 | Implement the accepted self-contained runtime: bundle controlled Python/PyMuPDF/Pdfium/templates/models, required native libraries, license notices, and capability manifest. Python is permanent and may not be eliminated. | Offline clean Windows and macOS machines run parse/edit/verify/history without development tools or a global Python install. |
| PKG-002 | P1/P2 | REL-01, REL-03 | Build signed Windows installer with dependencies, app data layout, shortcuts, uninstall, upgrade, and rollback. | Fresh Windows VM install, core E2E, upgrade, rollback, uninstall, signature verification. |
| PKG-003 | P1/P2 | REL-01, REL-03 | Build universal or approved-architecture macOS bundle with correct dynamic libraries, signing, notarization, version, bundle ID, and entitlements. | Fresh macOS VM/device smoke, Gatekeeper/notarization, upgrade/rollback. |
| PKG-004 | P2 | REL-04 | Maintain Linux as a reproducible development/CI host and explicitly remove unsupported customer-release claims unless a later ADR promotes it. | Linux clean-clone build/test passes; public platform matrix accurately distinguishes development from customer support. |
| PKG-005 | P1 | REL-02 | Correct or remove Docker/headless deployment. If retained, align toolchain/target/runtime and expose only honestly supported processing APIs with auth/isolation. | Clean image build and representative processing smoke; health/readiness reflect real capability. |
| PKG-006 | P2 | REL-03 | Release only from protected semantic-version tags after every gate; publish checksums, SBOM, provenance, changelog, migration, limitations, and rollback. | Unsigned/unverified/failed-CI tag cannot publish. |
| PKG-007 | P1 | QA-01 | Add first-run diagnostics for storage, engines, licenses, fonts, models, renderer, and optional providers; disable unavailable capabilities. | Clean-machine matrix reports precise state and never false-ready. |
| PKG-008 | P2/P3 | Updates/support | Add controlled update notification or documented manual update, evidence-preserving migration, support bundle, and rollback procedure. | Failed migration restores prior app/data; support bundle passes privacy checks. |
| PKG-009 | P3 | Metadata/legal | Correct authorship, copyright, license notices, third-party notices, icons, identifiers, and release notes. | Package metadata scan and legal checklist pass. |

**Gate 10:** Signed, self-contained packages pass clean-machine installation, first-run diagnostics, the bundled Python contract suite, offline core lifecycle, optional-provider capability checks, upgrade, rollback, and uninstall on mandatory Windows and macOS. No source-tree-only success is accepted.

## 14. Phase 11 — Functional qualification and minor sweep

**Purpose:** Prove the complete functional product under normal, adversarial, interrupted, and packaged conditions before final non-blocking security/privacy hardening.

| Ticket | Sev. | Work | Ticket verification |
|---|---:|---|---|
| QUAL-001 | P0/P1 | Run every unit, Python contract, integration, E2E, GUI, package, and provider-mock suite from clean clones on mandatory Windows and macOS plus Linux development CI. | Complete green functional matrix with exact test counts and no unexplained skip. |
| QUAL-002 | P0/P1 | Reconcile the 57-finding register and every new implementation finding to merged tickets and evidence. | Zero open P0/P1; every P2 fixed or removed from product; mapping is independently reviewed. |
| QUAL-003 | P1/P2 | Run authorized corpus qualification across born-digital, scanned, hybrid, sparse/dense, one/many pages, fonts, rotations, locales, and malformed inputs. | Exact expected ledger/edit membership/fidelity disposition for every fixture. |
| QUAL-004 | P1 | Fault-inject provider outage, timeout, disk-full, permission denial, corrupt cache, crash, forced shutdown, concurrent instances, interrupted merge, and failed upgrade. | Atomicity/recovery matrix passes; no partial published output or false success. |
| QUAL-005 | P2 | Measure startup, extraction, edit, render, verify, batch throughput, memory, disk, and package size at agreed 1/10/50/100-page profiles. | Budgets approved; regression thresholds enforced. |
| QUAL-006 | P1/P2 | Execute accessibility qualification with keyboard-only, screen reader, scaling, contrast, focus, errors, batch, audit, and installation flows. | No critical/major accessibility defect; medium/minor findings closed or fixed before GA. |
| QUAL-007 | P1 | Run manual/scheduled live-provider qualification against approved non-production accounts and synthetic fixtures. | Contract, privacy, rate-limit, cost, timeout, schema, and revocation evidence. |
| QUAL-008 | P3 | Whole-repository minor sweep: TODO/FIXME/stub/dead-code inventory, stale comments, warning suppression, naming, copy, metadata, examples, docs links, error consistency, and developer workflow. | Every item fixed, intentionally removed, or documented as non-shipping internal code; no user-visible known minor remains. |

**Gate 11:** Every functional gate passes from a clean state on mandatory Windows and macOS; the bundled Python pipeline, corpus, fault-injection, performance, accessibility, provider, package, and minor-sweep suites are green. No general-availability release occurs yet.

## 15. Phase 12 — Final non-blocking auditability, secrets, privacy, dependencies, and supply chain

**Purpose:** Complete the deliberately deferred non-blocking privacy and defense-in-depth work after functional qualification. Any active credential exposure or data-exfiltration defect discovered earlier remains an immediate blocker and is not deferred.

| Ticket | Sev. | Findings | Work | Ticket verification |
|---|---:|---|---|---|
| SEC-001 | P1 | SEC-01 | Remove the fake passphrase root-of-trust. If an application lock is approved, implement a real verifier and threat model without logging hashes. | Two unrelated secrets cannot both authenticate; migration and recovery tests pass. |
| SEC-002 | P1 | SEC-02 | Move credentials to OS-backed secret storage; restrict any unavoidable files; minimize clones; zeroize all sensitive buffers. | No plaintext key is written to working directory; permission, memory-lifetime, and migration tests. |
| SEC-003 | P2 | SEC-03 | Make telemetry opt-in and allowlist-based; redact/account for financial, identity, transaction, path, token, and error payloads. | Synthetic PII/secret corpus produces zero prohibited outbound/log fields. |
| SEC-004 | P1 | AUD-02 | Record every production edit with actor/source/output hashes, exact edit evidence, verifier result, parent version, and terminal disposition. | Full lifecycle creates complete records; missing audit persistence blocks success. |
| SEC-005 | P1 | AUD-03 | Replace silent 24-hour/30-day deletion with configurable retention, explicit archive/export, purge confirmation, and purge audit records. | Time-travel tests prove no silent deletion and correct configured retention. |
| SEC-006 | P2 | AUD-04 | Hash-chain audit records and content-address snapshots; add integrity verification and optional signed export. | Modify/delete/reorder test detects every tamper before history/export. |
| SEC-007 | P2 | SEC-04 | Implement truthful bug/support bundles: honor include/exclude choices, preview contents, scrub secrets/PII, and report attachment failures. | Bundle manifest matches UI choice; privacy corpus passes. |
| SEC-008 | P1 | QA-02 | Upgrade/remove vulnerable, unsound, and unmaintained dependencies; document justified temporary exceptions with expiry. | Blocking RustSec/dependency policy passes with no unapproved advisory. |
| SEC-009 | P2/P1 | QA-07, REL-03 | Generate SBOM and provenance, pin actions/toolchains, protect release credentials, verify third-party downloads, and define vulnerability-response cadence. | Artifact has verifiable SBOM/provenance/checksums; compromised/unpinned input test fails. |
| SEC-010 | P2 | Threat model | Document assets, actors, boundaries, cloud data flows, local attacker assumptions, license checks, telemetry, and incident response. | Security review maps each threat to requirement, test, owner, and residual risk. |
| SEC-011 | P3 | Security documentation | Publish safe secret setup, rotation, revocation, data-handling, support-bundle, and incident procedures. | Clean operator drill follows docs without exposing credentials. |

**Gate 12:** Threat model approved; fake attestation removed; secrets use supported secure storage; audit integrity/retention/privacy tests pass; dependency, SBOM, provenance, and secret scans are green.



## 16. Phase 13 — Final complete rerun and production release

**Purpose:** Re-run every functional, Python, platform, security, privacy, package, and evidence gate after final hardening so late changes cannot regress the verified product.

| Ticket | Sev. | Work | Ticket verification |
|---|---:|---|---|
| FINAL-001 | P0/P1 | Re-run all unit, Python contract, integration, E2E, GUI, accessibility, provider-mock, package, and P0 mutation suites from clean Windows/macOS environments and Linux development CI. | Complete green matrix with exact tests, skips, versions, and artifacts. |
| FINAL-002 | P0/P1 | Re-run the full authorized PDF/financial/fidelity corpus and all crash, timeout, disk, cancellation, concurrency, upgrade, rollback, and malformed-input fault injection. | Zero false success, partial publication, edit loss, balance escape, or mutable evidence. |
| FINAL-003 | P1/P2 | Re-run final privacy, secret, audit integrity, retention, dependency, SBOM, provenance, signature, license, and support-bundle checks against packaged artifacts. | Signed review with no release blocker and no regression from Gate 11. |
| FINAL-004 | P0/P1/P2/P3 | Reconcile the 57 audited findings plus every implementation finding to merged tickets and evidence. | Zero open P0/P1; every P2/P3 is fixed or its complete user-facing surface and claim are removed. |
| FINAL-005 | Release | Cut the release candidate, perform packaged acceptance, verify migration and rollback, obtain owner approval, and publish signed Windows/macOS artifacts with evidence and limitations. | All gates green, signatures/checksums valid, rollback verified, owner approval recorded. |

**Gate 13 / General Availability:** Every earlier gate passes again after final hardening; zero P0/P1 remain; all P2/P3 issues are fixed or their complete surface is removed; signed Windows/macOS packages pass clean-machine acceptance; the evidence pack is independently reproducible.

## 17. Finding-to-ticket coverage matrix

| Audit finding range | Owning tickets |
|---|---|
| COR-01 | INT-005, EXT-006, VER-005 |
| COR-02 | INT-001, INT-002, VER-003 |
| COR-03 | INT-003, INT-009 |
| COR-04 | INT-004, UX-007 |
| COR-05 | INT-001, PDF-001, VER-003 |
| COR-06 | EXT-006, VER-005 |
| COR-07 | VER-004, VER-007 |
| COR-08 | EXT-004 |
| COR-09 | RUN-007, EXT-005 |
| COR-10 | PDF-001, PDF-002, PDF-003, VER-003 |
| COR-11 | PDF-007 |
| COR-12 | PDF-008 |
| COR-13 | RUN-002, UX-006 |
| COR-14 | EXT-007 |
| COR-15 | RUN-005, EXT-002 |
| COR-16 | INT-006, EXT-003 |
| COR-17 | RUN-009, EXT-007 |
| COR-18 | PDF-007 |
| COR-19 | VER-001, VER-009 |
| COR-20 | INT-006 |
| AUD-01 | INT-007, SEC-006 |
| AUD-02 | INT-008, SEC-004 |
| AUD-03 | SEC-005 |
| AUD-04 | SEC-006 |
| SEC-01 | SEC-001 |
| SEC-02 | SEC-002 |
| SEC-03 | SEC-003 |
| SEC-04 | SEC-007 |
| OPS-01 | RUN-004 |
| OPS-02 | RUN-005 |
| OPS-03 | BASE-001 |
| OPS-04 | RUN-007, PDF-006 |
| OPS-05 | RUN-008 |
| OPS-06 | RUN-003 |
| OPS-07 | RUN-002, UX-007 |
| OPS-08 | RUN-006 |
| OPS-09 | RUN-006 |
| OPS-10 | BASE-006 |
| OPS-11 | BASE-006, RUN-009 |
| QA-01 | BASE-005, RUN-007, PKG-007 |
| QA-02 | BASE-002, SEC-008 |
| QA-03 | BASE-003 |
| QA-04 | BASE-004 |
| QA-05 | GOV-004, BASE-007, UX-010 |
| QA-06 | BASE-003 |
| QA-07 | BASE-002, SEC-009 |
| QA-08 | BASE-007, UX-010 |
| UX-01 | UX-004 |
| UX-02 | UX-002 |
| UX-03 | UX-003 |
| UX-04 | UX-005 |
| UX-05 | UX-008, QUAL-006 |
| UX-06 | RUN-001, UX-001 |
| REL-01 | PKG-001, PKG-002, PKG-003 |
| REL-02 | PKG-005 |
| REL-03 | SEC-009, PKG-002, PKG-003, PKG-006 |
| REL-04 | PKG-004 |
| New P3/minor findings | Owning-phase P3 ticket and QUAL-008 final sweep |

## 18. Mandatory evidence at every phase

| Artifact | Required contents |
|---|---|
| Phase manifest | Base/merge SHA, ticket list, owners, commands, exits, test counts, artifacts, residual risks, and next gate. |
| Regression log | P0 suite, affected domain suite, prior phase gates, and exact environment/tool versions. |
| Security record | Secret scan, dependency changes, threat-model impacts, external downloads/licenses, and privacy effects. |
| Data/fidelity record | Fixture hashes, expected/actual ledger, requested/applied edits, verifier report, and output hashes. |
| Migration record | Config/history/storage/package change, forward migration, rollback, compatibility, and user notice. |
| Diff review | Accidental files, generated artifacts, credentials, broad suppressions, disabled tests, TODO/FIXME/stubs, and unrelated changes. |

## 19. Stop and escalation rules

Stop the current ticket and ask for a decision when a fix would change the approved product boundary, licensing, real-statement handling, secret storage, supported OSes, signing identity, public CLI contract, data migration, or paid-provider cost. Stop and repair immediately when any prior P0 regression fails. Never make progress by weakening an assertion, widening a calibrated threshold after failure, adding arbitrary sleeps, swallowing errors, disabling tests, accepting missing artifacts, or changing a failure into a warning.

## 20. Expected delivery sequence

The remote work will arrive as small ticket branches and phase pull requests, not one unreviewable mega-commit. The first remote change will be Phase 00 governance/release-freeze material. The second will establish the cross-platform baseline. Only after Gate 01 passes will behavior-changing P0 repairs begin. The final delivery includes signed Windows/macOS release artifacts, the bundled Python capability manifest, complete evidence, migration and rollback notes, a closed finding register, the local-LLM go/no-go record, and a baseline-versus-final report.
