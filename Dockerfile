# syntax=docker/dockerfile:1
# ---------------------------------------------------------------------------
# Bank Statement Fidelity Editor — container image for headless deployment.
#
# This image runs the `serve` subcommand (HTTP health surface + worker runtime).
# It does NOT include the GUI (egui requires a display server).
#
# IMPORTANT: This container runs the native Rust stack only. The Python bridge
# (PyO3 / PyMuPDF) is NOT available in this image. All Python-dependent features
# (per-segment editing, font replication) will report as UNAVAILABLE.
#
# For a full-featured local build with Python support, see QUICKSTART.md.
# ---------------------------------------------------------------------------

# ====== Stage 1: build =====================================================
# Pinned to match rust-toolchain.toml (channel = "1.89.0").
FROM rust:1.89-bookworm AS builder

# Build-time system deps:
#   - pkg-config + libssl-dev for reqwest TLS
#   - fontconfig/freetype headers for the GUI crates
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        libfontconfig1-dev \
        libfreetype6-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Leverage layer caching: copy manifests first, then sources.
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY src ./src
COPY tests ./tests
COPY assets ./assets

# DUAL_CORE_PASSPHRASE is only needed to *run* the binary, not to compile it.
RUN cargo build --release --bin dual-core-pdf-pipeline

# ====== Stage 2: runtime ===================================================
FROM debian:bookworm-slim AS runtime

# Runtime system deps (shared-object versions, not the -dev headers):
#   - libfontconfig1 / libfreetype6 : required by the linked binary
#   - ca-certificates               : TLS roots for reqwest
RUN apt-get update && apt-get install -y --no-install-recommends \
        libfontconfig1 \
        libfreetype6 \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Application binary + the runtime assets it reads relative to the cwd.
COPY --from=builder /build/target/release/dual-core-pdf-pipeline /usr/local/bin/dual-core-pdf-pipeline
COPY bank_templates ./bank_templates

# Writable working directories the app expects on startup.
RUN mkdir -p audit output logs cache/fonts

# Security: run as non-root.
RUN groupadd --system appgroup && useradd --system --gid appgroup appuser \
    && chown -R appuser:appgroup /app
USER appuser

ENV RUST_LOG=info
# Railway injects $PORT; default for local `docker run` parity.
ENV PORT=8080
EXPOSE 8080

# Required at runtime (must be supplied as an environment variable at deploy time):
#   DUAL_CORE_PASSPHRASE — software root-of-trust passphrase (≥16 chars)
#
# Optional AI/parsing keys (all have offline fallbacks if not set):
#   GEMINI_API_KEY, LLAMAPARSE_API_KEY, PDFREST_API_KEY, PYMUPDF_PRO_KEY, etc.
#   See .env.example for the full list.

CMD ["dual-core-pdf-pipeline", "serve"]
