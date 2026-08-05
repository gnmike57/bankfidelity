# Gate 06 Evidence Manifest

## Decision

**Gate:** 06 — exact PDF editing, geometry, fonts, segmentation, and atomic output

**Decision:** **PASS**

**Base SHA:** `7c785f426c235b4c645dd0995fd276a75ed8d305`

**Verified candidate SHA:** `913672163fb5eee329a3b2b63e8ae601958fda63`

**Workflow run:** [30720299347](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30720299347)

**Branch:** `remediation/phase-06-pdf-exactness`

**Default branch modified:** No

**Release publication:** Frozen

The candidate replaces origin-only and best-effort mutation with one stable target identity and exact-count contract across native and PyMuPDF Pro operations. It handles CTM, text matrices, CropBox origins, and page rotation through shared canonical geometry; rejects duplicate or stale targets before mutation; removes Typst reconstruction and mislabeled alternatives from fidelity finalization; makes font coverage explicit and fail-closed; pins and verifies Pdfium artifacts; validates segmented membership, geometry, metadata, order, interruption, and retry; and publishes only fully verified same-filesystem staged outputs through rollback-capable commit barriers.

## Ticket evidence

| Ticket | Implemented outcome | Principal implementation evidence | Regression or gate evidence | Result |
|---|---|---|---|---|
| PDF-001 | Every single and batch edit carries authoritative `old_text`, page, rectangle, and replacement data. Native and Pro paths require exact requested, matched, and placed counts with zero failures before publication. | `src/pdf/native_engine.rs`, `src/pdf/pymupdf_engine.rs`, `src/app/runtime.rs`, `python/worker.py`, `python/pymupdf_pro_integration.py`, generated protocol fixtures. | Exact single/batch contracts, worker mapping, 20-edit repeatability, mismatch, zero/partial, and prior-output tests. | PASS |
| PDF-002 | Native extraction and editing share one CTM/text-matrix target collector and one canonical top-left visible coordinate system across MediaBox, CropBox, and 0/90/180/270-degree page rotation. | `src/pdf/native_engine.rs`. | CTM-transformed, cropped, rotated, and crop-origin mapping regressions; native/PyMuPDF fixture parity. | PASS |
| PDF-003 | Stable old-text identity and operator membership replace origin-only matching. Zero, duplicate, ambiguous, and double-selected targets fail before mutation. | `src/pdf/native_engine.rs`, `python/pymupdf_pro_integration.py`, workflow target preflight. | Duplicate operator, old-text mismatch, one-operator/two-edit, and repeated-value selector regressions. | PASS |
| PDF-004 | Unsupported font synthesis, donor substitution, generic fallback, and undisclosed embedded-font replacement are disabled. Missing glyph coverage or failed re-embedding is a typed pre-publication failure with explicit UI guidance. | `src/engine/font_replication.rs`, `src/engine/font_analysis.rs`, `src/app/runtime.rs`, `src/app/gui.rs`, `src/app/modals.rs`, Python bridge. | Missing-glyph, re-embedding failure, disabled legacy operation, no-artifact, and GUI coverage regressions. | PASS |
| PDF-005 | Segment maps require contiguous offsets, exact bounded page membership, consistent edited state, total-page reconciliation, and unchanged MediaBox/CropBox/rotation. Merges preserve global page order, document metadata, page geometry, interruption safety, and retryability. | `src/engine/segments.rs`, `src/engine/pdf_split_merge.rs`, segmented runtime paths. | Boundary edits, malformed maps, wrong counts, geometry drift, metadata/page-box/order, interruption, retry, and prior-output tests. | PASS |
| PDF-006 | Pdfium resolution uses an embedded immutable release manifest with platform archive/library SHA-256 values, bounded safe extraction, license installation, local bundle verification, and system-library discovery. Mutable latest-release download behavior is removed. | `assets/pdfium-artifacts.json`, `src/pdf/native_engine.rs`. | Manifest, checksum mismatch, wrong-member, license, local probe, load, and clean-host CI smoke tests. | PASS |
| PDF-007 | Typst is a backward-readable legacy enum only and cannot be selected or invoked to finalize fidelity editing. Standalone reconstruction returns explicit non-success and creates no replacement artifact. | `src/app/config.rs`, `src/app/runtime.rs`, `src/app/gui.rs`, `src/app/modals.rs`, CLI help. | Two-page integrity regression proves explicit failure and preservation of an existing destination. | PASS |
| PDF-008 | Visual alternatives expose only the engine that actually produced each validated artifact. Duplicate native outputs labeled as Pdfium/Typst and unverified candidates are removed. | `src/app/runtime.rs`. | All-target compilation and integrity tests; source audit finds no fabricated engine labels or lossy alternative path. | PASS |
| PDF-009 | Native edits, Pro edits, page surgery, split/merge, segmented workflows, proposed changes, transfer, and confirm-and-render use same-directory staging, validation, flush, rollback-capable replacement, and post-publication verification. | `src/app/commit.rs`, `src/pdf/native_engine.rs`, `src/engine/pdf_split_merge.rs`, `src/engine/segments.rs`, `src/app/runtime.rs`. | Invalid target, missing/corrupt input, interruption, wrong geometry/count, audit failure, and merge fault tests preserve prior destination bytes. | PASS |

## Mandatory workflow evidence

| Job | Job ID | Conclusion |
|---|---:|---|
| rustfmt | `91422854997` | PASS |
| Clippy production surfaces | `91422854945` | PASS |
| Base state — Ubuntu | `91422854975` | PASS |
| Base state — macOS 14 / Apple Silicon | `91422854961` | PASS |
| Base state — Windows | `91422854965` | PASS |
| Optional PyMuPDF Pro import — macOS | `91422854972` | PASS |
| Optional PyMuPDF Pro import — Windows | `91422854978` | PASS |
| P0 integrity regressions — Ubuntu | `91423306039` | PASS |
| P0 integrity regressions — macOS | `91423306040` | PASS |
| P0 integrity regressions — Windows | `91423306041` | PASS |
| Deferred hardening inventory | `91422855001` | Expected advisory outcome; non-blocking until Phase 12 |

## Local qualification

| Check | Evidence | Result |
|---|---|---|
| Complete Rust library suite | 289 passed, 0 failed | PASS |
| Complete Rust integration surface | `cargo test --locked --tests --no-fail-fast -- --test-threads=1` completed with no failed target | PASS |
| Complete Python suite | 28 passed, 0 failed | PASS |
| Base protocol CI reproduction | Generated fixture parity plus 18 manifest/protocol/worker tests | PASS |
| Strict Clippy | `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Native geometry and exact targeting | CTM, CropBox, rotation, duplicate, mismatch, and exact membership regressions | PASS |
| Segment transaction | Boundary, interruption/retry, malformed map, wrong count, geometry drift, metadata, page order, and prior-output preservation | PASS |

## Gate invariants

| Invariant | Evidence | Result |
|---|---|---|
| One target means one exact operator | Stable text identity plus geometry selects exactly one operator or fails before mutation. | PASS |
| Geometry has one source of truth | Extraction and editing reuse canonical visible-coordinate transforms. | PASS |
| Font fidelity is truthful | Coverage-complete original/re-embedded fonts are required; synthesis and undisclosed substitutions cannot publish. | PASS |
| Segmentation cannot drop or reorder edits | Every global edit maps to exactly one segment; every accepted segment preserves page geometry and count. | PASS |
| Reconstruction cannot masquerade as fidelity | Typst is excluded from selection, automatic fallback, visual alternatives, and finalization. | PASS |
| Pdfium artifacts are immutable and verified | Pinned archive and library digests plus license material are mandatory for bundled installation. | PASS |
| Publication is all-or-nothing | Final destinations remain untouched until every mutation, visual/math, structural, audit, and byte-identity gate passes. | PASS |
| Prior guarantees remain active | Extraction completeness, financial correctness, supervised Python protocol, lifecycle, audit, and P0 integrity remain mandatory. | PASS |

## Residual work

Gate 06 does not authorize release. Phase 07 must rebuild the independent verification gates and calibrated visual/structural acceptance evidence. Optional local LLM work remains gated to Phase 08, GUI and recovery completion to Phase 09, packaging to Phase 10, full clean-machine qualification to Phase 11, deferred hardening to Phase 12, and signed release evidence to Phase 13. Release publication remains frozen.

## Evidence files

| File | Purpose |
|---|---|
| `candidate.tsv` | Base SHA, candidate SHA, branch, and workflow identity. |
| `ci-run.json` | Complete public workflow-run response. |
| `ci-jobs.json` | Complete public job and step response. |
| `ci-summary.tsv` | Concise job outcomes and URLs. |
| `commits.tsv` | Ordered Phase 06 commit ledger. |
| `changed-paths.tsv` | Name/status change scope. |
| `change-stat.txt` | Phase diff statistics. |
| `local-summary.tsv` | Concise local qualification outcomes. |
| `critical-files-sha256.txt` | Identities of the pinned Pdfium manifest and principal exactness regressions. |
| `SHA256SUMS` | Evidence-file checksums. |

## Advancement

Gate 06 is closed. The next executable phase is **Phase 07 — independent verification gates**. No later phase may weaken stable target identity, canonical geometry, exact edit counts, full font coverage, segment membership, pinned Pdfium provenance, or atomic publication.
