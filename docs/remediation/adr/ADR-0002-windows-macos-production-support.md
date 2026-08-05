# ADR-0002: Windows and macOS Are Mandatory Production Platforms

**Status:** Accepted
**Date:** 2026-07-31
**Decision owner:** Repository owner

## Context

The audited repository advertises cross-platform behavior, but its checked-in toolchain and Cargo target are Windows-specific, a Windows-only UI automation dependency is not target-scoped, release artifacts omit runtime dependencies, and macOS package metadata is stale. The repository owner requires the repaired application to operate on Windows and macOS.

## Decision

Windows and macOS are mandatory, release-blocking production platforms.

| Platform | Required support |
|---|---|
| Windows | Windows 10/11 x64, signed installer, bundled production Python/PyMuPDF runtime, first-run diagnostics, upgrade/rollback/uninstall, GUI/CLI/package E2E. |
| macOS | Apple Silicon is mandatory. Intel or universal support remains an explicit later owner decision. The app must be correctly bundled, signed, notarized, dependency-complete, upgradeable, and Gatekeeper-valid. |
| Linux | Linux remains a supported development/CI host where practical, but it is not a mandatory customer desktop artifact unless promoted by a later ADR. |

No platform may rely on a developer’s globally installed Python, ad hoc native DLL/dylib placement, a writable repository checkout, or a current working directory. Platform-specific dependencies must be target-scoped.

## Packaging principles

1. Release artifacts are built only from protected, passing tags.
2. Runtime components and licenses are enumerated in a capability manifest.
3. Clean-machine tests run the packaged artifact, not the source tree.
4. Package readiness checks validate Python, PyMuPDF tier, Pdfium/rendering, OCR models, templates, fonts, writable storage, and optional providers.
5. A missing optional capability disables the affected control with exact guidance; it does not make the entire app falsely ready.
6. Version, product name, bundle ID, signing identity, and release notes come from one source of truth.

## Required verification

The ADR is satisfied only when clean Windows and macOS environments pass installation, first run, offline parse/edit/balance/render/verify/history, batch smoke, failure/recovery, upgrade, rollback, and uninstall. Signatures/notarization, checksums, SBOM, provenance, and package contents must be independently verified.
