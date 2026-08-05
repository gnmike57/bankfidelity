# ADR-0004: Base-State-First Repair, Final Privacy and Security Hardening

**Status:** Accepted with non-waivable release safeguards
**Date:** 2026-07-31
**Decision owner:** Repository owner

## Context

The repository owner requires a strict dependency hierarchy: first establish a reproducible executable base state, then repair and validate the core business logic, and only afterward apply privacy, security, cryptographic, and defense-in-depth modifiers to the stabilized architecture. The audit also identified destructive false-success paths. A core workflow that loses edits, blanks content, or returns the wrong document is not a valid functional base state; correcting those outputs is business-logic repair, not deferred hardening.

## Decision

Execute in this order: reproducible build/runtime base state; correct and stable core workflow outputs; unified runtime and Python fortification; extraction/editing/verification completion; GUI and Windows/macOS packaging; functional qualification; then final privacy, security, dependency, auditability, and supply-chain hardening. After hardening, rerun every functional and packaged gate so modifiers cannot regress the base state.

The following safeguards remain active throughout all phases:

1. Never commit or print credentials, private keys, service-account JSON, production secrets, or real customer statements.
2. Use synthetic or authorized redacted fixtures in Git and CI.
3. Do not add new telemetry fields containing statement content, identity data, account data, transaction descriptions, or raw provider payloads.
4. Do not weaken existing secret handling or retention behavior merely to simplify a functional fix.
5. Treat any discovered active data exfiltration, credential exposure, or unsafe customer-data handling as an immediate blocker regardless of phase.

Strict schemas, cryptographic integrity chains, keychain migration, telemetry minimization, privacy masks, dependency policy, and supply-chain controls are applied to the stabilized data flows in the final hardening phase. All audited privacy, secret, auditability, dependency, and supply-chain findings remain mandatory closure items. That phase must pass before general availability and will be followed by a complete rerun of every functional and packaged gate.

## Consequences

Buildability, execution, core algorithm correctness, Python reliability, and cross-platform delivery receive the earliest engineering attention. Some internal hardening debt may temporarily coexist on remediation branches, but no release may occur until the final privacy/security gate and the post-hardening functional rerun are both green.
