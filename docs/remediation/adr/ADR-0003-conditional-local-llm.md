# ADR-0003: No Mandatory Local LLM for the v1 Release

**Status:** No-go for v1; future optional evaluation remains possible

**Date:** 2026-08-02
**Decision owner:** Repository owner

## Context

The application exposes deterministic extraction, canonical financial validation, exact PDF mutation, and independent fail-closed verification. A local LLM could assist with natural-language intent, categorization, error explanation, or parser-ambiguity suggestions, but it must never become a source of financial truth or a prerequisite for the verified core workflow.

The Phase 08 gate requires measurable benchmark benefit, approved Windows/macOS resource budgets, redistributable licensing, strict typed output, adversarial resilience, offline operation, optional installation, clean-machine lifecycle evidence, and proof that absence or failure cannot affect document state or terminal success. No local runtime/model combination has completed that full benchmark and packaging gate, and no owner-approved resource budgets exist for shipping one in v1.

## Decision

**Do not bundle or require a local LLM in the v1 release.** `Capability::LocalLlm` remains explicitly `Unavailable`, with a precise reason. The complete product remains functional without a model, and deterministic extraction, editing, financial validation, edit-membership checks, fidelity verification, audit persistence, and publication remain authoritative.

Cloud AI providers remain optional and additive where separately configured and contract-tested. Their absence, malformed output, timeout, or disagreement cannot weaken a mandatory local gate.

## Candidate uses retained for future evaluation

| Candidate use | Allowed future output | Authority boundary |
|---|---|---|
| Natural-language edit intent | Typed, reviewable proposal resolving to deterministic edit objects | Cannot write a PDF or approve publication |
| Transaction categorization | Suggested category and confidence | Cannot alter financial values or balances |
| Error and audit explanation | Plain-language explanation from typed evidence | Cannot change the underlying disposition |
| Parser ambiguity review | Candidate interpretation with provenance and confidence | Cannot bypass completeness or human review |

## Explicit non-goals

A future local LLM may not determine whether a statement is balanced, declare visual fidelity, approve a destructive edit, bypass completeness, create terminal success, write directly to a PDF, alter audit evidence, or override deterministic verification.

## Gate outcome

| Requirement | v1 evidence | Outcome |
|---|---|---|
| Material benchmark benefit | No approved cross-platform benchmark demonstrates sufficient benefit | Not met |
| Windows x64 and macOS Apple Silicon budgets | No owner-approved latency, memory, disk, startup, or thermal budgets | Not met |
| Optional packaging lifecycle | No checksummed, licensed, install/remove/rollback component is qualified | Not met |
| Deterministic authority boundary | Existing capability registry marks the feature unavailable; core logic remains deterministic | Met |
| Graceful absence | The verified core workflow has no local-model dependency | Met |

**Gate 08 decision: NO-GO for shipping a local LLM in v1.** This is a completed product decision, not an implementation failure. A later proposal must reopen this ADR with a versioned benchmark, approved budgets, licensing evidence, adversarial tests, and clean-machine optional-component qualification.
