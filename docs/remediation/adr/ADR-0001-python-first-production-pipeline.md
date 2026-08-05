# ADR-0001: Python-First Production PDF Pipeline

**Status:** Accepted
**Date:** 2026-07-31
**Decision owner:** Repository owner
**Implementation owner:** Manus AI remediation program

## Context

The existing application uses Rust for the desktop/CLI/runtime and PyO3 plus `python/pymupdf_pro_integration.py` for primary PDF extraction, editing, font, rendering, segmentation, and recovery operations. The audit found critical defects at the Rust/Python success boundary, including opaque JSON acceptance, inaccurate applied counts, blank-redaction false success, incomplete packaging, and weak process/recovery semantics. These are contract and engineering failures; they are not a reason to remove Python.

The repository owner explicitly requires the Python/PyMuPDF pipeline to be preserved and fortified as a permanent production component.

## Decision

Python and PyMuPDF remain **first-class production dependencies** on Windows and macOS. The remediation will not replace or de-scope the Python pipeline. It will make the boundary explicit, typed, testable, self-contained, and recoverable.

The production design will enforce all of the following:

| Boundary | Required decision |
|---|---|
| Runtime | Ship a controlled Python runtime and pinned compatible packages with the desktop application, or an equivalently deterministic embedded distribution. Do not depend on an arbitrary user Python installation for the verified core path. |
| Protocol | Replace opaque success JSON with versioned request/response schemas containing operation ID, requested/matched/placed/failed counts, per-edit evidence, warnings, review flags, source/output hashes, timings, and typed failure codes. |
| Process model | Keep Python work serialized where library safety requires it, but isolate each document/run, bound memory and time, support cancellation, detect worker death, and restart safely without publishing partial output. |
| Output commit | Python writes only to a unique scratch output. Rust independently validates the result, persists audit evidence, flushes, and atomically publishes the final artifact. |
| Correctness | A Python success is advisory until Rust verifies exact edit membership, requested output existence, structural validity, visual fidelity, and deterministic financial invariants. |
| Packaging | Windows and macOS packages include every required Python module, shared library, font-processing dependency, templates, model/capability metadata, license notice, and first-run diagnostic. |
| Compatibility | Pin and test a supported matrix of Rust/PyO3, Python, PyMuPDF, PyMuPDF Pro, fontTools, Pillow, and platform architectures. Upgrades require contract, corpus, and packaged smoke tests. |
| Observability | Log operation IDs, state, timings, and typed error classes without logging statement content, credentials, or raw provider payloads. |
| Native engine | Retain the native engine as an independently tested fallback and verifier aid, not as a pretext to weaken or bypass Python correctness. |

## Non-goals

This decision does not guarantee that every optional PyMuPDF Pro feature is available without a valid license. Capability detection must state the exact active tier. The decision also does not permit a Python operation to bypass independent verification or audit persistence.

## Consequences

The release artifacts will be larger, cross-platform packaging is more complex, and dependency/license management becomes a permanent responsibility. In return, the application retains its strongest existing PDF functionality while gaining deterministic behavior, reproducibility, and clean-machine support.

## Required verification

The ADR is satisfied only when:

1. The no-overlap blank-redaction reproduction fails non-destructively.
2. Every Python operation emits a valid versioned typed result and exactly one terminal state.
3. Worker crash, timeout, cancellation, malformed response, memory pressure, disk-full, and missing-license tests fail safely.
4. Exact-count and edit-membership tests pass across representative PDF fixtures.
5. Fresh Windows and macOS machines install and run the complete offline core lifecycle without a developer Python installation.
6. Python package and license versions appear in the release capability/evidence manifest.
