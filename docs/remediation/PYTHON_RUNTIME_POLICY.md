# Permanent Python Runtime Policy

## Contract

The Python/PyMuPDF pipeline is a permanent production component. Windows and macOS packages must ship a self-contained Python runtime and must never download, install, or upgrade Python packages during application startup or document processing.

| Component | Production policy |
|---|---|
| Python | Bundled CPython 3.12 for the first polished release |
| PyMuPDF | Exact pin from `requirements-ci.txt` |
| PyMuPDF Pro | Exact matching pin from `requirements-pro.txt` |
| Supported desktop targets | Windows x86_64; macOS arm64 and x86_64 |
| Process model | One supervised single-threaded worker process per application runtime |
| Network behavior | No package installation or dependency download at runtime |

PyMuPDF documents that its wheels support Windows, macOS, and Linux and current Python releases. PyMuPDF Pro publishes wheels for Windows x86_64, macOS x86_64/arm64, and Linux, and its release history states which exact PyMuPDF version each Pro release supports. The repository therefore requires the installed core and Pro versions to match exactly rather than relying on an unverified compatibility range. See [PyMuPDF on PyPI](https://pypi.org/project/PyMuPDF/) and [PyMuPDF Pro on PyPI](https://pypi.org/project/PyMuPDFPro/).

## Startup capability decision

The Rust supervisor prefers the application-bundled runtime and worker modules on Windows and macOS, with explicit development overrides only. Before importing the production bridge, the worker verifies `python/runtime-manifest.json`: Python major/minor version, protocol version, exact package tier, entry point, and SHA-256 hashes for every production Python module. Any drift produces an unavailable handshake and no document operation runs.

The worker emits a strict startup handshake containing its Python version, PyMuPDF version, PyMuPDF Pro version, exact-version compatibility result, supported operations, platform, and import failure classes. The Rust capability registry may report the core pipeline as ready only after the manifest check and handshake succeed. Pro operations are selectable only when all of the following are true:

1. The Pro package imports successfully.
2. The PyMuPDF and PyMuPDF Pro versions match exactly.
3. A Pro key is configured.
4. The operation respects the verified three-page segmentation boundary.

A missing or incompatible Pro package must not disable core rendering, extraction, page cloning, page removal, or worker health checks. It must disable only Pro-gated operations with an actionable reason.

## Upgrade procedure

A Python runtime upgrade is accepted only in a dedicated ticket that updates both exact pins, regenerates clean environments on every packaged target, runs protocol and worker tests, runs the 100-operation resource regression, runs crash/hang/malformed-output recovery tests, executes the complete bank-statement corpus, and passes clean-machine package installation. Independent core or Pro upgrades are prohibited.

## Release evidence

Each signed release must preserve the following evidence:

- installed Python, PyMuPDF, and PyMuPDF Pro versions;
- successful `python/verify_runtime_versions.py` and `python/verify_runtime_manifest.py --tier pro` output;
- complete worker handshake with secrets and document paths excluded;
- cross-platform protocol, lifecycle, resource, and P0 integrity results;
- hashes of bundled Python runtime files and wheel inputs.
