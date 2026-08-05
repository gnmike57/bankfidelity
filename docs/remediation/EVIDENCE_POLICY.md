# Remediation Evidence Policy

**Status:** Required for every remediation ticket and phase gate
**Applies to:** Rust, Python, GUI, CLI, PDF engines, providers, packaging, and documentation

## Operating rule

A code change is not complete when it compiles or appears to work. It is complete only when the original defect is reproduced, a meaningful regression test fails before the fix, the narrow fix passes, earlier critical regressions remain green, and the evidence can be replayed from the recorded revision.

No gate may be passed by disabling a test, broadening a mask, widening a calibrated threshold after seeing a failure, swallowing an error, accepting a missing artifact, adding an arbitrary sleep, or converting a failed postcondition into a warning.

## Ticket evidence

| Field | Required content |
|---|---|
| Identity | Ticket ID, severity, audited finding IDs, owner, branch, base SHA, candidate SHA, and owning phase/gate. |
| Reproduction | Fixture and source hashes, exact command, environment/tool versions, exit status, observed output, and why the result is wrong. |
| Expected contract | Required preconditions, exact success postconditions, failure/no-op semantics, forbidden outcomes, and artifact requirements. |
| Red test | Test name/path, command, pre-fix output, and proof that the assertion detects the defect rather than incidental behavior. |
| Implementation | Changed paths, design/ADR link, schema or migration impact, and explicit non-goals. |
| Ticket verification | Formatter, focused lint, unit/contract/integration tests, negative cases, boundaries, and required platform results. |
| Regression | All earlier P0 tests, affected lifecycle suites, Python cross-language contracts where applicable, and prior phase gates. |
| Artifacts | Logs, JSON reports, output hashes, edit-membership reports, verifier overlays, package manifests, and screenshots only when necessary. |
| Risk and rollback | Data-integrity, compatibility, performance, platform, package, provider, security, and rollback considerations. |
| Diff review | Secret/private-data scan, generated artifacts, disabled tests, suppressions, TODO/FIXME/stubs, unrelated changes, and clean-tree result. |

## Phase evidence manifest

Create `docs/remediation/evidence/phase-XX/manifest.md` at every gate with the following sections:

1. Scope and ticket disposition.
2. Base and merge revisions.
3. Platform and toolchain matrix.
4. Commands, exits, and exact test counts.
5. P0 and earlier-gate regression results.
6. Python protocol/runtime/package results where applicable.
7. PDF/financial/fidelity fixture hashes and outcomes.
8. Packaging, migration, upgrade, rollback, and clean-machine results where applicable.
9. New findings and their owning tickets.
10. Residual risks, blockers, and explicit non-claims.
11. Evidence-file inventory with SHA-256 hashes.
12. Gate disposition: `PASS`, `FAIL`, or `BLOCKED`.

A phase may advance only on `PASS`. `BLOCKED` requires an owner decision; `FAIL` returns to the responsible ticket.

## Platform evidence

Windows and macOS are mandatory customer platforms. Linux is a development/CI host. Platform-specific tickets must record the exact runner/OS version, architecture, Rust/Python/PyMuPDF matrix, package contents, and clean-machine behavior. Source-tree-only success does not qualify a packaged application.

## Python evidence

Every Python operation must record a versioned typed result with operation ID, requested/matched/placed/failed counts where relevant, per-item evidence, warnings, review flags, capability/license tier, source/output hashes, timing, and typed terminal disposition. Rust must independently validate the output and postconditions before publication.

## Artifact handling

Commit small deterministic fixtures, schemas, manifests, and reports that are needed for regression. Do not commit credentials, private keys, service-account files, real customer statements, generated customer PDFs, full raw provider payloads, or large transient build outputs. Store large authorized evidence in the approved external evidence location and commit only the hash, metadata, reproduction command, and access-controlled reference.

## Final closure

General availability requires a final evidence pack that replays every phase after the last privacy/security hardening change. It must map every audited and newly discovered finding to a merged ticket and passing proof, with zero unresolved P0/P1 and no user-visible P2/P3 unless the complete affected surface and claim were removed.
