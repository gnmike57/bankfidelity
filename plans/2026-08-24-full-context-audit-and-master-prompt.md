# BankFidelity — Full Context Audit & Master Prompt

Date: 2026-08-24 · Mode: Architect · Scope: full-repo context gathering + fault hunt

---

## PART 1 — WHAT THE APP IS (verified context)

**Crate:** `dual-core-pdf-pipeline` v2.0.0 ([Cargo.toml](../Cargo.toml)) — Rust 1.89.0, edition 2021.
**Product:** "Bank Statement Fidelity Editor" — pixel-perfect, evidence-verified **in-place editing of AU bank statement PDFs** with an exact-decimal balance engine and an 8-gate cryptographic evidence ledger.

### Module map (live code)
| Layer | Path | Role |
|---|---|---|
| app | [src/app/runtime.rs](../src/app/runtime.rs) (10,281 lines) | Job runtime: dispatch, workflows, parser fallback chain, transfer engine |
| app | [src/app/gui.rs](../src/app/gui.rs) (5,753 lines), [modals.rs](../src/app/modals.rs) | egui GUI, `__DISPATCH:` signal routing, interactive fallback modals |
| app | [nlp_router.rs](../src/app/nlp_router.rs), [config.rs](../src/app/config.rs), [cli.rs](../src/app/cli.rs), [server.rs](../src/app/server.rs) | NLU intent router, ApiAvailability/config, CLI parity, HTTP server |
| engine | [offline_parser.rs](../src/engine/offline_parser.rs), [transfer.rs](../src/engine/transfer.rs), [balance.rs](../src/engine/balance.rs), verification*.rs | Deterministic parsing, exact-decimal rebalancing, multi-gate verification |
| pdf | [selector.rs](../src/pdf/selector.rs) | Engine rank: PyMuPDF Pro → Native (Pdfium/lopdf) → Typst reconstruct |
| ai | gemini/document_ai/llamaparse/reducto/pdfrest/openai/local_llm/mcp/ufo/python_worker | Multi-provider AI + MCP stdio server + supervised Python worker |
| security | [software_root.rs](../src/security/software_root.rs) | DUAL_CORE_PASSPHRASE root-of-trust, ChaCha20-Poly1305 |

### Runtime data flow
GUI/CLI → `NlpCommand` → `Job` → runtime actor → workflow → parser chain (**Reducto → Document AI → LlamaParse → OfflineHeuristic**, with interactive fallback modals, 300 s choice timeout) → balance engine (rust_decimal) → PDF edit (engine selector) → verification gates → audit evidence JSON.

---

## PART 2 — FAULT REGISTER (findings from this audit)

### P0 — Critical

**[NEW-P0-1] Dead duplicate runtime fork (~10k lines of zombie code).**
[src/app/runtime/](../src/app/runtime/) (`core.rs` 8,272 lines + `client.rs`, `jobs.rs`, `python_job.rs`, `tracking.rs`) is **never compiled**: no `mod` declarations exist anywhere ([app/mod.rs](../src/app/mod.rs:19) declares only `pub mod runtime;` → [runtime.rs](../src/app/runtime.rs); zero `#[path]` attributes). The directory contains a stale near-copy of the live monolith (e.g. Qwen directives at core.rs:783 vs runtime.rs:2140; identical `interactive_fallback_or_continue!` macro at core.rs:6635 vs runtime.rs:8000). Any fix applied to the wrong copy silently diverges — [AUDIT_SUMMARY.md](../AUDIT_SUMMARY.md) already cites line numbers from both copies.

**[NEW-P0-2] `AppConfig::default()` ships a hardcoded passphrase.**
[config.rs:385](../src/app/config.rs:385): `passphrase: "DEV_PASSPHRASE".into()`. In release builds `is_dev_mode = cfg!(debug_assertions)` is false, yet any caller constructing `Default` bypasses the mandatory `DUAL_CORE_PASSPHRASE` env contract; `validate()` flags it only if invoked. Compounding: [docai_cache.rs:70-74](../src/ai/docai_cache.rs:70) derives the encryption key as **unsalted single-pass SHA-256(passphrase)** — no KDF, no salt.

**[NEW-P0-3] MCP stdio server panics mid-session.**
[mcp.rs:41-43](../src/ai/mcp.rs:41): `serde_json::to_string(&response).unwrap()` + `writeln!(stdout).expect(...)` inside the request loop — a closed/broken stdout pipe kills the whole MCP bridge (UFO integration dies). Also advertises fabricated `"protocolVersion": "2026-07-28"` and `serverInfo.version "1.0.0"` while crate is 2.0.0.

### P1 — High

**[NEW-P1-4] Version drift across every surface.** Cargo.toml=2.0.0 · README=1.0.0 · REMAINING_WORK=1.1.1 · .env.example="v0.5.0" · MCP serverInfo=1.0.0 · MSI artifact=v2.0.0.

**[NEW-P1-5] Local-LLM endpoint drift docs vs code.** Skill/docs state Qwen @ `127.0.0.1:11434` (Ollama); [local_llm.rs:36](../src/ai/local_llm.rs:36) defaults to `http://127.0.0.1:8080/v1` (llama-server) via `LOCAL_LLM_URL`. Out-of-box NLU/forensics fail unless the user guesses the right port.

**[NEW-P1-6] Default-parser story contradicts itself.** README: "Mindee (default)". AGENTS.md table: LlamaParse default. Actual workflow chain in runtime.rs: Reducto → Document AI → LlamaParse → Offline (Mindee absent from the primary chain).

**[NEW-P1-7] License-key-shaped string embedded in source.** [env_spec.rs:62](../src/app/env_spec.rs:62) `example: "s50Hve2NbxCLVLIVqEU3lzFY"` matches the documented 24-char PyMuPDF trial-key format — leak risk / user confusion.

**[NEW-P1-8] E2E suite still self-skips (FND-002 open).** `workflow_e2e` hard-requires missing fixture `AU Bank Statements/IA_Bank_Statement_202602.pdf`; CI green ≠ verified.

**[NEW-P1-9] Async brittleness in the live monolith.**
- Busy-wait cancellation polling: [runtime.rs:1623-1624](../src/app/runtime.rs:1623) (10 ms sleep spin loop).
- `tokio::task::block_in_place` + `Handle::current().block_on` at [runtime.rs:6181](../src/app/runtime.rs:6181) — panics under a current-thread runtime; ties correctness to runtime flavor.

**[NEW-P1-10] Monolith complexity.** runtime.rs 10.3k lines with `macro_rules!` declared *inside* nested async fns (>10 indentation levels, e.g. lines 8000-8061); gui.rs 5.7k lines. Every edit risks merge/regression damage; clippy/fmt drift recurs (FND-004 pattern).

### P2 — Medium / hygiene

- **[NEW-P2-11]** Hardcoded fallback DocAI processor version `"pretrained-bankstatement-v5.0-2023-12-06"` ([runtime.rs:8129](../src/app/runtime.rs:8129)).
- **[NEW-P2-12]** Repo-root litter: ~30 one-off `fix_*.py` scripts, `coverage*.txt` ×7, `check*.json`, trace dumps (`brace_trace.txt`, `paren_trace.txt`), stray download artifact `Blocked by CloudFlare (via headers)`, committed MSI `.wixpdb`.
- **[NEW-P2-13]** Font bootstrap pins GitHub raw URLs by commit ([fontcache.rs:39-81](../src/app/fontcache.rs:39)) — pinned is good, but no offline bundle ⇒ first-run font fetch fails air-gapped.
- **[NEW-P2-14]** Naive charset entropy estimator ([software_root.rs:72](../src/security/software_root.rs:72)) — warn-only, fine, but presented as "cryptographic attestation".
- **[NEW-P2-15]** FND-001 (dev passphrase bypass) fixed at [software_root.rs:126](../src/security/software_root.rs:126) ✔ but P0-2 shows the same class of bug survives in `AppConfig::default()`.

### What is healthy (verified)
Toolchain pinned 1.89.0 ✔ · Zero TODO/FIXME debt in src ✔ · Production unwraps almost all guarded/annotated ✔ · offline_parser panics caught via catch_unwind ✔ · Atomic writes + CRC32 on transfer audits ✔ · Telemetry PII scrubbing ✔ · Extensive test tree (70+ integration files) ✔.

---

## PART 3 — THE MASTER PROMPT

> Copy-paste everything below the line as the next task for any agent working this repo.

---

**MISSION: Make BankFidelity trustworthy to change again — kill the zombie fork, close the security default holes, and make docs/tests stop lying. Work in the order given. Do not refactor anything else until Phase 1 is merged.**

You are working in `c:/bankfidelity/bankfidelity` — the Rust orchestrator half of BankFidelity (`dual-core-pdf-pipeline` v2.0.0, egui GUI + CLI, tokio job runtime, rust_decimal balance engine, multi-provider AI with mandatory offline fallbacks, MCP stdio bridge to Microsoft UFO, local Qwen 2.5 Coder 7B via OpenAI-compatible endpoint). Toolchain is pinned in `rust-toolchain.toml` (1.89.0) — do not touch it.

**Ground rules (non-negotiable):**
1. LIVE vs DEAD: `src/app/runtime.rs` is the ONLY live runtime module. Everything under `src/app/runtime/` (core.rs, client.rs, jobs.rs, python_job.rs, tracking.rs) is dead, never-compiled duplicate code. Never "fix" bugs there; never trust it as a reference for current behavior except as historical context.
2. Secrets: never print, copy, or commit key values. Report only set/missing status. Read `.env.example`, never `.env`.
3. Every pipeline stage keeps an offline fallback (cloud parser → offline_parser, AI balance → local engine, cloud render → local Pdfium). TransferTransactions/RunTransferTests keep their Gemini/Groq/OpenRouter requirement.
4. Validation gate after each phase: `cargo fmt && cargo check && cargo test --lib && cargo clippy --all-targets --all-features -- -D warnings`. A phase is not done until the gate passes.

**Phase 1 — Delete the zombie fork (P0-1).**
Confirm `src/app/runtime/*` is unreferenced (no `mod` decls, no `#[path]`). Move it out of the build path (delete or archive outside `src/`), then re-run the validation gate to prove nothing regressed. Add a guardrail so it cannot return: either a `tests/static_analysis.rs` assertion that `src/app/runtime/` does not exist, or a CI grep step. Update `AGENTS.md` and `docs/ARCHITECTURE.md` to state plainly that `runtime.rs` is the single source of truth.

**Phase 2 — Close the security default holes (P0-2, P1-7).**
- Replace `passphrase: "DEV_PASSPHRASE".into()` in `AppConfig::default()` with an empty string plus a loud constructor invariant: production paths must fail fast ("DUAL_CORE_PASSPHRASE not set") instead of silently encrypting with a known constant. Keep dev-mode shortening (`is_dev_mode`) intact.
- Upgrade `DocAiCache` key derivation from bare `SHA256(passphrase)` to a salted KDF (e.g. HKDF-SHA256 with a random per-cache salt persisted beside the cache, versioned in the cache header). Old caches may be invalidated — that is acceptable; document it.
- Remove the license-key-looking example string in `src/app/env_spec.rs` and replace with an obvious placeholder.
- Add tests: default config must NOT validate in non-dev mode; cache rejects wrong passphrase; env_spec contains no 24-char alphanumeric examples.

**Phase 3 — Make the MCP bridge unpanickable and honest (P0-3).**
In `src/ai/mcp.rs`: replace unwrap/expect in the stdio loop with logged errors + graceful continue/shutdown; write responses through a helper that never panics on serialization (fall back to a JSON-RPC error envelope). Set `serverInfo.version` from `env!("CARGO_PKG_VERSION")` and use a real, supported MCP protocol version constant. Add a test simulating a broken stdout pipe asserting clean shutdown, not panic.

**Phase 4 — Make docs and reality agree (P1-4/5/6, P2-11).**
Single-source versions: README, REMAINING_WORK, .env.example header, MCP serverInfo all derive from or match Cargo.toml 2.0.0. Decide ONE local-LLM story (Ollama :11434 vs llama-server :8080) — implement `LOCAL_LLM_URL` default to match the skill/docs, and update the bankfidelity skill + QUICKSTART + NLP_CHAT docs to match code. Reconcile the "default parser" claim (README Mindee vs AGENTS.md LlamaParse vs actual Reducto→DocAI→LlamaParse→Offline chain) everywhere it appears. Move the hardcoded DocAI processor version into config with the current value as default.

**Phase 5 — De-brittleness pass (P1-8/9/10).**
- Fix or explicitly delete the self-skipping `workflow_e2e` fixture dependency: commit a tiny synthetic AU statement fixture it can always find, so CI stops lying.
- Replace the busy-wait cancellation poll in runtime.rs (~line 1623) with a condvar/channel wake; remove or feature-gate the `block_in_place`+`block_on` call so it cannot run under a current-thread runtime.
- Extract the inline `interactive_fallback_or_continue!` macro and the parser-chain loop from runtime.rs into `src/app/runtime/parser_chain.rs` (declared properly this time), shrinking the monolith without behavior change. No other extraction this round.

**Phase 6 — Repo hygiene (P2-12).**
Delete or gitignore: root-level `fix_*.py` one-offs, `coverage*.txt`, `check*.json`, `brace_trace.txt`, `paren_trace.txt`, `mcp_in.txt`, stray HTTP captures, the CloudFlare artifact file, and the committed `.wixpdb`. Keep anything with lasting value by moving it under `scripts/archive/` or `docs/`.

**Definition of done:** all six phases merged in order; validation gate green after each; `cargo test --test static_analysis` (or CI equivalent) actively fails if the zombie fork returns; a short report listing root cause, files changed, commands run, and validation results per AGENTS.md reporting format.

---
