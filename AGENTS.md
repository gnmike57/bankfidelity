# AGENTS.md

## Project type

Rust desktop/CLI project using Cargo (v1.0.0).
Includes GUI (egui), CLI, Python bridge (PyO3), Node.js bridge (Applitools), PDF processing,
multi-backend AI integrations (Gemini, Document AI, Mindee, LlamaParse), tests, scripts, and CI.

## Autonomy level

The agent may operate with high autonomy for development, debugging, repair, and validation.

The agent may:

- inspect the repository
- inspect build/test/runtime logs
- modify source files, configuration, tests, scripts, and CI workflows
- run terminal commands needed for diagnosis and validation
- install or select the required Rust toolchain
- run formatting, linting, build, and test commands
- repeat the diagnose -> fix -> validate loop until resolved or blocked

Prefer durable project-level fixes over local temporary workarounds.

## Preferred Rust toolchain

Use Rust 1.89.0 unless a task explicitly requires another version.

If rust-toolchain.toml contains channel = "dev", replace it with:

    [toolchain]
    channel = "1.89.0"
    components = ["rustfmt", "clippy"]

If the toolchain is missing, run:

    rustup install 1.89.0
    rustup override unset

Then retry the original command.

## Files the agent may modify automatically

- rust-toolchain.toml
- rustfmt.toml
- .cargo/config.toml
- Cargo.toml
- Cargo.lock
- Rust source files under src/
- Rust tests under tests/
- examples under examples/
- scripts under scripts/
- Python support code under python/
- Node.js support code (src/ai/applitools_bridge.js)
- docs: README.md, QUICKSTART.md, CONTRIBUTING.md, CHANGELOG.md, AgentManagement.md, AgentDevelopmentGuide.md
- CI files under .github/workflows/
- Docker/deployment config when the task is about build/deploy repair
- .env.example

## Files requiring explicit confirmation

- .env
- private keys, tokens, credentials
- production deployment secrets
- private user PDFs
- real customer or banking data
- generated audit/history files
- generated output PDFs
- large generated output directories
- Git history
- remote repository state

## Terminal permissions

Allowed (non-destructive):

    cargo check
    cargo build
    cargo build --release
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    cargo fmt
    rustup install 1.89.0
    rustup override unset
    rustc --version
    cargo --version
    python --version
    pip --version
    node --version
    npm --version
    git status
    git diff

Health checks (only if they do not expose secrets):

    cargo run -- doctor
    cargo run -- verify-api-keys

## Commands requiring confirmation

    git reset --hard
    git clean -fdx
    git push
    git commit
    git rebase
    cargo publish
    docker push
    railway up
    railway deploy
    rm -rf
    Remove-Item -Recurse -Force

Also requires confirmation:

- uploading files
- deleting generated PDFs, logs, or audit files
- modifying real bank statements
- commands that may spend significant API credits
- commands that may expose secrets in logs

## Secrets policy

Never print, copy, rewrite, or commit secrets.
May report whether a variable is set, but never its value.

Allowed:

    GEMINI_API_KEY is set
    PDFREST_API_KEY is missing
    MINDEE_API_KEY is set (46 chars)

Forbidden:

    GEMINI_API_KEY=actual-secret-value

May read .env.example. Must not read or modify .env unless explicitly instructed.
If a variable is missing, update .env.example or docs instead of inventing a value.

## API keys and backends

The project uses the following API keys (all optional except DUAL_CORE_PASSPHRASE):

| Key | Backend | Fallback |
|---|---|---|
| DUAL_CORE_PASSPHRASE | Encryption (required) | None |
| GEMINI_API_KEY | AI balance, vision, validation | Manual-only mode |
| MINDEE_API_KEY | Cloud parser (Mindee) | offline_parser |
| LLAMAPARSE_API_KEY | Default parser (LLM) | offline_parser |
| DOCUMENT_AI_* | Google ML parser | offline_parser |
| PDFREST_API_KEY | Cloud verification render | Local Pdfium |
| APPLITOOLS_API_KEY | Visual AI testing | SSIM-only |
| PYMUPDF_PRO_KEY | Enhanced font handling | PyMuPDF free tier |

Boot-time availability detection lives in `src/app/config.rs` (`ApiAvailability`).
Backend preferences UI lives in `src/app/modals.rs` (`draw_backend_preferences`).

## Standard validation commands

Run the narrowest useful check first.

    cargo check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    cargo fmt

Full validation:

    cargo fmt
    cargo check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings

If full validation is too slow, run targeted checks and report what was skipped.

## Runtime debugging strategy

1. Read the exact error text.
2. Identify the failure category:
   toolchain/setup, dependency, compile, test, runtime panic,
   missing configuration, external API, filesystem permissions,
   Python bridge, Node.js bridge, or PDF engine.
3. Inspect only relevant files.
4. Make the smallest durable fix.
5. Re-run the failing command.
6. Continue until fixed or blocked by secrets/external services/private files/confirmation.

## Known setup issue: toolchain 'dev' is not installed

Cause: project or local rustup override targets a toolchain named "dev".

Fix:

1. Open rust-toolchain.toml.
2. Replace channel = "dev" with channel = "1.89.0".
3. Run:

   rustup install 1.89.0
   rustup override unset
   cargo check

## Error-handling strategy

Prefer typed errors, useful context, clear messages, validation before expensive work,
and fail-safe behavior for documents/outputs.

Avoid silent failures, swallowed errors, unchecked unwraps in production paths,
and temporary duct-tape fixes.

## Fallback chain rules

Every pipeline stage must have at least one offline fallback:
- Cloud parsers → offline_parser
- AI balance → local balance engine
- Cloud rendering → local Pdfium
- Visual AI → SSIM-only metrics
- PyMuPDF edit → Pdfium → Typst reconstruct (ultimate)

**Exception**: `TransferTransactions` and `RunTransferTests` strictly require an AI provider (Gemini/Groq/OpenRouter) for layout-agnostic format mapping. Their source and target parsing stages fall back to `offline_parser`, but the actual translation mapping has no offline equivalent.

New integrations must follow this pattern and register in ApiAvailability.

## Reporting format

At the end of each session, summarize:

- root cause
- files changed
- commands run
- validation result
- remaining manual steps, if any
