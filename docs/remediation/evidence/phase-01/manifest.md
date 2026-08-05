# Phase 01 Gate Manifest

**Disposition:** `PASS`
**Phase:** Phase 01 — Executable base state
**Gate:** Gate 01
**Base commit:** `5c3678c`
**Candidate branch:** `remediation/phase-01-base-state`
**Candidate commit:** `7cb54c2993242b3b3614589d58f3f1dbbde8f646`
**Remote CI run:** [30653780202](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30653780202) — overall `success`
**Prepared by:** Manus AI
**Date:** 2026-07-31

## Scope and ticket disposition

| Ticket | Severity | State | Evidence |
|---|---:|---|---|
| BASE-001 | P1 | Implemented and cross-platform verified | Host-neutral Rust toolchain/Cargo config, target-scoped UIAutomation, portable Windows Cargo wrapper; Windows, macOS, and Linux development jobs passed. |
| BASE-002 | P1 | Implemented and remote verified | Blocking format/lint/base-state jobs and concurrency cancellation passed; deferred hardening inventory remains explicitly advisory. |
| BASE-003 | P1 | Partially implemented | Required deterministic library/runtime/startup suites are explicit; broader live/UI suite classification remains in owning phases. |
| BASE-004 | P2 | Deferred by ticket scope | Node visual-test disposition remains a later base/quality cleanup item and does not block this executable subset. |
| BASE-005 | P1 | Deferred to health/readiness repair | Current base state proves import, compile, and startup; truthful component/readiness schema remains open. |
| BASE-006 | P2 | Partially implemented | `--help` and `--version` are configuration/telemetry/audit/runtime free with new integration regressions; full subcommand exit-code matrix remains open. |
| BASE-007 | P1 | Deferred | Version/claim/branding reconciliation remains open after the executable base exists. |
| BASE-008 | P2/P3 | Implemented for tracked artifacts | Historical logs, generated outputs, stale scratch scripts, and machine-specific Pdfium DLLs removed; ignore rules strengthened. |
| BASE-009 | P3 | Implemented | One documented verification command for Windows and one for macOS/Linux development hosts. |

## Functional base-state changes

| Area | Baseline failure | Repair |
|---|---|---|
| Rust toolchain | Repository pinned a Windows GNU host triple on every OS. | Pin host-neutral Rust 1.89.0 with `rustfmt` and `clippy`. |
| Cargo target | Global Windows target and developer-local MinGW paths broke Linux/macOS. | Remove repository-wide target/linker/archiver; use native host selection. |
| Test dependencies | Windows UIAutomation dependency compiled on unsupported hosts. | Scope `uiautomation` under `cfg(windows)`. |
| Python | CI installed an unpinned subset and had no production-module smoke. | Pin PyMuPDF 1.28.0, pin optional PyMuPDF Pro 1.28.0, and execute the real bridge smoke. |
| Library tests | `jwt_claim_shape` read a missing service-account/private-key fixture. | Validate deterministic claim serialization in memory with no secret fixture. |
| CLI startup | Help/version initialized dotenv, Sentry, config, telemetry, audit, and runtime. | Parse Clap before all side effects; regression tests require empty stderr without configuration. |
| Build time | Test profile rebuilt the full dependency graph at a different optimization level. | Align test/development profiles for dependency reuse. |
| Repository state | Generated logs, PDFs, PNGs, scratch scripts, and Pdfium DLLs were tracked. | Remove generated artifacts and prevent re-entry through `.gitignore`. |

## Platform and toolchain matrix

| Platform | Architecture | Rust | Python | PyMuPDF | Result |
|---|---|---|---|---|---|
| Linux development | x86_64 | 1.89.0 | 3.12.3 local | 1.28.0 + optional Pro package present | PASS |
| Windows | x86_64 MSVC | 1.89.0 | 3.11 | 1.28.0 base and optional-Pro package modes | PASS — job `91233067547`; Pro job `91233067542` |
| macOS | Apple Silicon | 1.89.0 | 3.11 | 1.28.0 base and optional-Pro package modes | PASS — job `91233067603`; Pro job `91233067553` |

## Local commands and results

| Command / invariant | Result | Evidence summary |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | No formatting diff. |
| `python3 python/smoke_test.py` | PASS | Real production bridge imported; one-page PDF created, counted, rendered to valid PNG with exact 612×792 point geometry. |
| `cargo clippy --locked --lib --bins -- -D warnings` | PASS | Production library and binaries strict-clean. |
| `cargo check --locked --all-targets` | PASS | Every Linux host target compiled after target scoping. |
| `cargo test --locked --lib --no-fail-fast` | PASS | 228 passed; 0 failed; 0 ignored. |
| `cargo test --locked --test runtime_smoke` | PASS | 2 passed; runtime ping/history lifecycle. |
| `cargo test --locked --test cli_startup_contract` | PASS | 2 passed; help/version configuration-free and side-effect-free. |
| Production binary build | PASS | Debug production binary linked on constrained Linux host using an external local LLD wrapper; repository config remains host-neutral. |
| Direct `--version` / `--help` without configuration | PASS | Exit 0; expected content; stderr empty. |
| Phase 01 deterministic validator | PASS | Workflow YAML, matrix, config, pins, cleanup, and evidence checks passed. |

## Failures encountered and repaired

| Failure | Classification | Resolution |
|---|---|---|
| Linux attempted the Windows GNU target. | Repository configuration defect | Host-neutral toolchain and Cargo config. |
| Linux all-target tests pulled Windows UIAutomation. | Manifest scoping defect | Windows target-specific dev dependency. |
| Local lib-test link could not find `libpython3.12`. | Development-host prerequisite | Installed matching Python development library; CI uses setup-python 3.11. |
| One library test failed on absent `tests/fixtures/test_service_account.json`. | Non-portable test defect | In-memory deterministic JWT claim-shape test. |
| Help/version emitted OTLP startup warning. | Startup-order defect | Clap parse moved before all side effects and regression-locked. |
| Initial binary link exceeded the constrained-host memory budget. | Local resource constraint | One Cargo job and external LLD wrapper; no host linker committed. |

## Repository and workflow validation

The CI workflow parses as YAML, uses read-only permissions, runs on every branch/PR, cancels superseded runs, and defines mandatory Ubuntu, Windows, and macOS base-state jobs. The publication workflow remains manual, read-only, and frozen. Remote run [30653780202](https://github.com/rogertqq-code/bank-statement-fidelity-editor/actions/runs/30653780202) completed with overall `success` on candidate commit `7cb54c2`: rustfmt, production Clippy, all three base-state platforms, and both optional-Pro import jobs passed. The explicitly deferred dependency-advisory job reported the previously audited dependency findings and concluded `failure` under job-level `continue-on-error`; it did not weaken or bypass any functional gate and remains assigned to final hardening.

## Migration and rollback

There is no customer-data or schema migration. Developer environments must stop relying on the removed Windows GNU default and developer-local Cargo path. Windows uses its native MSVC host toolchain. Python setup uses the exact requirements manifests. Rollback is a branch revert, but the previous global target and false-green CI must not be restored.

## New findings

| Finding | Severity | Disposition |
|---|---:|---|
| Missing Python development library was undocumented for Linux local PyO3 test linking. | P2 | Added to `docs/DEVELOPMENT.md` prerequisite; hosted CI uses setup-python. |
| JWT shape test depended on an absent private-key fixture. | P1 | Fixed and regression-verified in this phase. |
| Help/version performed telemetry/configuration startup. | P1 | Fixed and regression-verified in this phase. |
| Test profile caused a complete differently optimized dependency rebuild. | P2 | Aligned with development profile and verified. |

## Gate decision

| Requirement | Result |
|---|---|
| Linux development executable base state | PASS |
| Windows x64 base-state CI | PASS |
| macOS Apple Silicon base-state CI | PASS |
| Optional Pro package import smoke on Windows/macOS | PASS |
| Local evidence and diff hygiene | PASS |
| Remote clean-branch reproducibility | PASS |

**Final disposition:** `PASS`
