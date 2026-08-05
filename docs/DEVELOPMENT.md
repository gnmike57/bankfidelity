# Development and Executable Base State

Windows x64 and macOS Apple Silicon are the mandatory customer platforms. Linux is a supported development and CI host. The repository selects the native host target automatically; do not add a repository-wide target, linker, archiver, or machine-local tool path.

## Required tools

| Component | Required baseline |
|---|---|
| Rust | 1.89.0 through `rustup`; `rust-toolchain.toml` installs `rustfmt` and `clippy`. |
| Python | 3.11 in CI; a compatible local interpreter selected through `PYO3_PYTHON`. |
| Python PDF runtime | Exact packages in `requirements-ci.txt`; optional Pro extension in `requirements-pro.txt`. |
| Windows | Standard `windows-latest`/MSVC toolchain; no MSYS2 or hard-coded developer path. |
| macOS | Apple Silicon runner/device with `pkg-config` and OpenSSL 3. |
| Linux development | C/C++ build tools, `pkg-config`, OpenSSL headers, Fontconfig, FreeType, and matching Python development headers. |

## Bootstrap

From a clean checkout, install the pinned Python base runtime:

```bash
python3 -m pip install --requirement requirements-ci.txt
```

Windows PowerShell uses the same manifest:

```powershell
python -m pip install --requirement requirements-ci.txt
```

Install `requirements-pro.txt` only when exercising the optional PyMuPDF Pro package path. Package presence alone does not prove a valid license or Pro readiness; later capability probes determine the active tier.

## One-command verification

On macOS or Linux development hosts:

```bash
./scripts/verify-base-state.sh
```

On Windows PowerShell:

```powershell
./scripts/verify-base-state.ps1
```

The command blocks on formatting, the real Python bridge smoke, host all-target compilation, production Clippy, library tests, runtime actor smoke, configuration-free CLI startup, production-binary build, and a clean working tree.

## Constrained Linux hosts

The project’s dependency graph is large. On a low-memory development host, use one Cargo job and a low-memory linker without committing host-specific configuration:

```bash
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR=/path/to/external/build-cache
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/path/to/local/cc-lld-wrapper
./scripts/verify-base-state.sh
```

Windows and macOS CI use their native linkers. Host-specific linker choices must remain outside `.cargo/config.toml`.

## Test classification during remediation

| Class | Current Gate 01 role |
|---|---|
| Library unit tests | Blocking. |
| `runtime_smoke` | Blocking, local and deterministic. |
| `cli_startup_contract` | Blocking, configuration-free and deterministic. |
| Python production-bridge smoke | Blocking in base and optional-Pro package modes. |
| Live-provider workflows | Not part of the base state; they require explicit provider fixtures/accounts and are qualified in their owning phase. |
| Desktop/UI automation | Platform-labelled and qualified after authoritative workflow-state repair. |
| Dependency/security inventory | Visible but advisory until final hardening, except active credential exposure or data exfiltration is an immediate blocker. |

## Failure handling

Do not obtain a passing result by disabling a test, accepting both success and failure, widening a threshold after observing a defect, swallowing an error, or allowing a missing fixture to self-skip a required suite. Record new implementation findings in `docs/remediation/FINDINGS.md`, assign a ticket in `docs/remediation/MASTER_PLAN.md`, and preserve the reproduction in the phase evidence manifest.
