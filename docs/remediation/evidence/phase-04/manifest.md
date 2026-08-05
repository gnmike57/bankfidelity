# Gate 04 Evidence Manifest

## Decision

**Gate:** 04 — permanent Python/PyMuPDF production-pipeline fortification

**Decision:** **PASS**

**Base SHA:** `c84c631e62590a366608b9dd2d9c20c476568f78`

**Verified candidate SHA:** `85e15fca5574068035eccab9e3a700958ee94b92`

**Workflow run:** [30707260996](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30707260996)

**Branch:** `remediation/phase-04-python-fortification`

**Default branch modified:** No

**Release publication:** Frozen

The candidate satisfies Gate 04. Python 3.12 and PyMuPDF 1.28.0 remain permanent production components behind a versioned, typed, supervised subprocess protocol. Mutations are staged and atomically published, worker crashes and hangs are bounded without replay, runtime compatibility is verified before work, bundled-runtime discovery is first-class on Windows and macOS, and cross-platform base-state plus P0 integrity gates remain green.

## Ticket evidence

| Ticket | Implemented outcome | Principal implementation evidence | Regression or gate evidence | Result |
|---|---|---|---|---|
| PY-001 | A versioned Rust/Python schema covers all 15 production operations, stable IDs, hashes, dispositions, counts, warnings, metrics, typed failures, and strict unknown-field rejection. | `src/ai/python_protocol.rs`, `python/bridge_protocol.py`, `fixtures/python-protocol/`. | Cross-language golden fixtures, malformed payloads, deny-unknown parsing, and fixture-generation checks run in every host base-state job. | PASS |
| PY-002 | Python runs as one supervised JSON-lines worker with strict handshake, bounded queue, deadlines, crash/hang detection, no replay, deterministic restart, cancellation-compatible callers, and exactly one correlated response. | `python/worker.py`, `src/ai/python_worker.rs`, `python/worker_fault_fixture.py`. | Crash, hang, malformed stdout, broken pipe, response-timeout, overload, restart, and correlation regressions pass on all hosts. | PASS |
| PY-003 | Every mutating operation writes to a unique worker stage, verifies input identity and exact counts, flushes bytes, atomically replaces the requested artifact, and returns a hash/size/count manifest. | `python/worker.py` (`MutationTransaction`), `src/app/runtime.rs`, `src/app/commit.rs`. | Hash mismatch, missing/empty artifact, partial edit, atomic publication, and real-runtime P0 regressions pass; prior output remains untouched on failure. | PASS |
| PY-004 | Python 3.12, PyMuPDF 1.28.0, and PyMuPDF Pro 1.28.0 are exactly pinned and checked in handshake, manifest, base CI, and optional-Pro CI. | `requirements-ci.txt`, `requirements-pro.txt`, `python/verify_runtime_versions.py`, `python/runtime-manifest.json`. | Windows/macOS optional-Pro jobs and all host manifest/version checks pass. | PASS |
| PY-005 | Embedded PyO3 state was removed. Worker recycling is bounded by operation count, RSS growth, and handle growth; core extraction closes documents deterministically; native-extension stdout is quarantined from the protocol. | `src/ai/python_worker.rs`, `python/worker.py`, `python/pymupdf_pro_integration.py`; removed `src/ai/pyo3_bridge.rs` and `pyo3`. | Hundred real-PDF operations, worker recycling, Pro-installed protocol, and complete worker suites pass. | PASS |
| PY-006 | Strict protocol schemas, input-hash guards, file existence checks, Pro page-limit enforcement, exact mutation evidence, and centralized typed errors reject malformed, stale, unsupported, and incomplete work before publication. | `python/bridge_protocol.py`, `python/worker.py`, `python/pymupdf_pro_integration.py`. | Unknown-field, malformed JSON, tamper, version drift, page-limit, missing-input, no-overlap, and incomplete-mutation regressions pass. Broader corpus qualification remains mandatory in Phase 11. | PASS |
| PY-007 | A GUI-independent Python contract harness covers handshake, protocol parity, extraction, exact application, atomic artifacts, Pro limits, failures, manifest drift, and copied-runtime startup. | `python/test_worker.py`, `python/test_bridge_protocol.py`, `python/test_runtime_manifest.py`, `python/test_apply_many_edits_contract.py`, `scripts/test_python_bundle_layout.py`. | The harness is blocking in Windows, macOS, and Linux base-state CI; Pro-installed exact-count tests are additionally blocking in P0 CI. | PASS |
| PY-008 | Every response carries operation identity, capability tier, duration, RSS, handles, GC activity, stable failure code/class, and count/hash evidence without statement payloads in routine supervisor diagnostics. | `python/worker.py`, `src/ai/python_protocol.rs`, `src/ai/python_worker.rs`. | Golden response fixtures, typed-error regressions, correlation tests, and bounded stderr-tail behavior pass. | PASS |
| PY-009 | Production discovery is bundled-runtime-first for Windows and macOS layouts, with deterministic copied-runtime inputs, hashes, entrypoint, package pins, and no reliance on PATH in the package smoke. | `src/ai/python_worker.rs`, `python/runtime-manifest.json`, `scripts/test_python_bundle_layout.py`, `docs/remediation/PYTHON_RUNTIME_POLICY.md`. | Offline copied-runtime smoke passes on all base-state hosts. Final installer assembly and signing remain Phase 10 responsibilities, not a relaxation of this runtime contract. | PASS |
| PY-010 | Runtime drift, tamper, Python-version mismatch, package-version mismatch, incomplete bundle layout, and worker failure are detected before document mutation; restart never replays an ambiguous operation. | `python/verify_runtime_manifest.py`, `python/test_runtime_manifest.py`, `src/ai/python_worker.rs`. | Tamper/version tests, incomplete-bundle discovery tests, crash/hang/no-replay tests, and manifest checks pass on every host. Upgrade packaging and installer rollback remain Phase 10. | PASS |
| PY-011 | The supported runtime, extension, protocol, packaging-input, upgrade, and debugging policy is versioned; the embedded bridge and stale duplicate surface were removed. | `docs/remediation/PYTHON_RUNTIME_POLICY.md`, protocol fixtures, runtime manifest, `Cargo.toml`, `Cargo.lock`. | Dead dependency removal, all-target compilation, copied-runtime replay, strict Clippy, and repository hygiene checks pass. | PASS |

## Mandatory workflow evidence

| Job | Job ID | Conclusion |
|---|---:|---|
| rustfmt | `91388433323` | PASS |
| Clippy production surfaces | `91388433321` | PASS |
| Base state — Ubuntu | `91388433293` | PASS |
| Base state — macOS 14 / Apple Silicon | `91388433302` | PASS |
| Base state — Windows | `91388433285` | PASS |
| Optional PyMuPDF Pro import — macOS | `91388433312` | PASS |
| Optional PyMuPDF Pro import — Windows | `91388433294` | PASS |
| P0 integrity regressions — Ubuntu | `91388887084` | PASS |
| P0 integrity regressions — macOS | `91388887124` | PASS |
| P0 integrity regressions — Windows | `91388887095` | PASS |
| Deferred hardening inventory | `91388433296` | Expected advisory failure; non-blocking until Phase 12 |

The workflow conclusion is `success`. The advisory dependency inventory remains intentionally non-blocking under the approved functional-first sequence and is still assigned to Phase 12.

## Gate invariants

| Invariant | Evidence | Result |
|---|---|---|
| Python remains first-class | Runtime starts the supervised worker; capability probing and production operations use it rather than an embedded fallback. | PASS |
| Protocol stdout is machine-readable | File-descriptor-level quarantine isolates native Pro license banners; Pro-installed worker tests pass on Windows, macOS, and Linux. | PASS |
| No ambiguous replay | Crash, hang, timeout, and malformed response retire the in-flight operation and restart only for subsequent work. | PASS |
| Exact mutation accounting | Bridge `placed` evidence maps to protocol `applied_count`; two-of-two real runtime application is P0-blocking. | PASS |
| Atomic publication | Unique stage, fsync, exact count/hash validation, and atomic replacement precede success. | PASS |
| Runtime compatibility | Python/package/protocol/source hashes are checked before bridge readiness. | PASS |
| Bounded resources | Operation, RSS-growth, and handle-growth budgets recycle workers; 100 real-PDF operations remain bounded. | PASS |
| Core/Pro truthfulness | Core text extraction does not require Pro; Pro readiness requires an exact compatible package and is reported separately. | PASS |
| Prior guarantees retained | Complete Windows/macOS/Linux base-state and P0 matrices pass. | PASS |

## Residual work

Gate 04 verifies the runtime contract and deterministic package inputs; it does not authorize a customer release. Final installer construction, signing, clean-machine package qualification, and upgrade rollback execution remain mandatory in Phases 10 and 11. Broader malformed/encrypted/large-document corpus qualification remains Phase 11. Release publication stays frozen.

## Evidence files

| File | Purpose |
|---|---|
| `candidate.txt` | Base SHA, candidate SHA, workflow, and branch identity. |
| `ci-run.json` | Complete public workflow-run response. |
| `ci-jobs.json` | Complete public job and step response. |
| `ci-summary.tsv` | Concise job outcomes and URLs. |
| `commits.tsv` | Ordered Phase 04 commit ledger. |
| `changed-paths.tsv` | Name/status change scope. |
| `change-stat.txt` | Phase diff statistics. |
| `SHA256SUMS` | Evidence-file checksums. |

## Advancement

Gate 04 is closed. The next executable phase is **Phase 05 — extraction, financial algorithms, OCR, dates, and transfer workflows**. No later phase may weaken the versioned Python protocol, supervised lifecycle, non-replay rule, atomic publication, exact-count evidence, compatibility manifest, resource bounds, bundled-runtime-first discovery, or prior P0 integrity gates.
