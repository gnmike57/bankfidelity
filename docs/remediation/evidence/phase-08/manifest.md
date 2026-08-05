# Gate 08 Evidence Manifest

## Decision

**Gate:** 08 — optional local LLM evaluation  
**Decision:** **NO-GO for shipping a local LLM in v1**  
**Capability disposition:** `Capability::LocalLlm` is explicitly `Unavailable`  
**Core workflow impact:** None; deterministic extraction, editing, finance, verification, audit, and publication remain complete without a model

Gate 08 permits a local LLM only after measurable benchmark value, approved Windows/macOS resource budgets, redistributable licensing, strict schema/adversarial qualification, optional-component lifecycle evidence, and proof that the model cannot override deterministic gates. Those shipping criteria are not met. The correct production decision is therefore to ship no local model or runtime in v1 rather than add an unqualified dependency.

## Ticket disposition

| Ticket | Disposition | Evidence | Result |
|---|---|---|---|
| LLM-001 | Candidate uses and non-goals are defined; financial truth, fidelity approval, direct writes, and terminal success are excluded. | `docs/remediation/adr/ADR-0003-conditional-local-llm.md`. | PASS |
| LLM-002 | No benchmark is required for a rejected shipping candidate; future reopening requires a versioned synthetic/redacted benchmark. | ADR reopening criteria. | NO-GO |
| LLM-003 | No runtime/model is selected because the full licensing, architecture, quantization, package, and maintenance gate has not passed. | Five-way engineering review and Gate 08 requirements. | NO-GO |
| LLM-004 | No inference adapter is introduced; model absence therefore cannot mutate a document or create success. | `src/app/capabilities.rs`; no local inference job exists. | PASS |
| LLM-005 | No owner-approved cross-platform performance/resource budgets or qualifying benchmark matrix exist. | ADR gate outcome. | NO-GO |
| LLM-006 | No model/runtime is bundled; core package remains model-free and fully functional. | Capability registry and package surface. | PASS |
| LLM-007 | No prompt-processing attack surface ships. Existing deterministic and optional-provider validation remains authoritative. | No local LLM code path. | PASS |
| LLM-008 | Formal comparison concludes support, package, resource, nondeterminism, and maintenance costs are unjustified without measured benefit. | Updated ADR and this manifest. | PASS — NO-GO |
| LLM-009 | User controls/model management are intentionally not added because the gate did not pass. | Explicit capability unavailability. | NOT APPLICABLE |

## Product guarantees

| Guarantee | Result |
|---|---|
| Application works offline without a model | PASS |
| No model package increases Windows/macOS distribution size | PASS |
| No LLM can alter a PDF, financial value, audit record, or verifier disposition | PASS |
| Capability and UI can truthfully explain absence | PASS |
| Future evaluation requires a new approved benchmark and ADR revision | PASS |

## Reopening criteria

A future proposal must provide a versioned benchmark, strict JSON schemas, invalid-output and adversarial results, approved Windows x64 and macOS Apple Silicon latency/memory/disk/startup/thermal budgets, redistribution licenses, checksums, offline network-blocked operation, optional install/remove/upgrade/rollback qualification, and proof that deterministic postconditions remain authoritative.

## Advancement

Gate 08 is closed as a **NO-GO**. The next active phase is Phase 09: coherent GUI, real batch processing, recovery, audit UX, and accessibility. No later work may silently introduce a local model dependency or give any language model financial, fidelity, mutation, audit, or terminal-success authority.
