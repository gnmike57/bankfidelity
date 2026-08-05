# Junie Project Guidelines

## Autonomy Policy (MAX)

Junie is granted maximum autonomy on this project. Junie SHOULD act without
asking for confirmation and SHOULD automatically run commands and make changes
needed to diagnose, fix, build, test, and validate the project.

Junie may, without asking first:

- Inspect any file in the repository and read build/test/runtime logs.
- Modify source files, configuration, tests, scripts, and CI workflows.
- Create, edit, move, or delete files that Junie itself generated.
- Install or select the required Rust toolchain (default: 1.88.0).
- Run formatting, linting, build, and test commands.
- Run the application (CLI `doctor`, `verify-api-keys`, and the `gui`) to validate behavior.
- Repeat the diagnose -> fix -> validate loop until resolved or genuinely blocked.

Prefer durable project-level fixes over temporary local workarounds.
Default to acting and reporting afterward rather than pausing to ask.

## Auto-approved (non-destructive) commands

    cargo check
    cargo build
    cargo build --release
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    cargo fmt
    cargo run -- doctor
    cargo run -- verify-api-keys
    cargo run -- gui
    rustup install 1.88.0
    rustup override unset
    rustc --version
    cargo --version
    python --version
    pip --version
    node --version
    npm --version
    git status
    git diff
    git log

## Commands that STILL require explicit confirmation

These remain gated even under MAX autonomy because they are destructive,
irreversible, externally visible, or may expose secrets / spend credits:

    git reset --hard
    git clean -fdx
    git push
    git commit
    git rebase
    cargo publish
    docker push
    railway up / railway deploy
    rm -rf / Remove-Item -Recurse -Force

Also requires confirmation:

- Uploading files or modifying remote repository state / Git history.
- Reading or modifying `.env`, private keys, tokens, or credentials.
- Deleting or modifying real bank statements, customer data, or generated audit/output files not created in the current
  session.
- Commands that may spend significant API credits or expose secrets in logs.

## Secrets policy

Never print, copy, rewrite, or commit secrets. May report whether a variable is
set (e.g. "GEMINI_API_KEY is set") but never its value. May read `.env.example`
but not `.env` unless explicitly instructed.

## Multi-engine invariant

This app must keep ALL PDF engines available for the entire lifecycle via
`PdfEngineSelector` (Auto / DualConcurrent / NativeOnly / PyMuPdfOnly /
TypstReconstruct). Do not remove any engine. Every pipeline stage must have at
least one offline fallback.

## Multi-backend architecture

Every cloud integration must have an offline fallback:
- Cloud parsers (Mindee, LlamaParse, DocAI) → offline_parser
- AI balance/validation (Gemini) → local balance engine
- Cloud rendering (pdfRest) → local Pdfium
- Visual AI (Applitools) → SSIM-only metrics
- PyMuPDF edit → Pdfium → Typst Reconstruct (ultimate)

New backends must:
1. Register in `ApiAvailability` (`src/app/config.rs`)
2. Add to the relevant mode enum
3. Surface in Backend Preferences UI (`src/app/modals.rs`)
4. Add key to `.env.example`

## Validation

Run the narrowest useful check first, escalate as needed:

    cargo check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    cargo fmt

If full validation is too slow, run targeted checks and report what was skipped.

## Reporting format

At the end of each session summarize: root cause, files changed, commands run,
validation result, and any remaining manual steps.
