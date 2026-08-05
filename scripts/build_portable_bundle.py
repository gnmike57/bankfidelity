#!/usr/bin/env python3
"""Build a deterministic, offline Windows or macOS application bundle.

All network access occurs here at build time. The resulting application never
installs or upgrades Python packages during startup or document processing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform as host_platform
import plistlib
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON_ARTIFACTS = ROOT / "assets" / "python-runtime-artifacts.json"
PDFIUM_ARTIFACTS = ROOT / "assets" / "pdfium-artifacts.json"
RUNTIME_MANIFEST = ROOT / "python" / "runtime-manifest.json"
REQUIREMENTS = ROOT / "requirements-pro.txt"
APP_NAME = "BankStatementFidelityEditor"
BINARY_NAME = "dual-core-pdf-pipeline"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_verified(url: str, destination: Path, expected_sha256: str, expected_size: int) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(url, headers={"User-Agent": "bank-statement-fidelity-editor-packager/1"})
    with urllib.request.urlopen(request, timeout=120) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output)
    size = destination.stat().st_size
    if size != expected_size:
        raise RuntimeError(f"download size mismatch for {url}: {size} != {expected_size}")
    actual = sha256_file(destination)
    if actual != expected_sha256:
        raise RuntimeError(f"download hash mismatch for {url}: {actual} != {expected_sha256}")


def extract_tar(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as bundle:
        bundle.extractall(destination, filter="data")


def run(command: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> None:
    subprocess.run(command, cwd=cwd or ROOT, env=env, check=True)


def copy_runtime_sources(destination: Path) -> None:
    manifest = json.loads(RUNTIME_MANIFEST.read_text(encoding="utf-8"))
    source_files = set(manifest["source_files"])
    source_files.add("runtime-manifest.json")
    destination.mkdir(parents=True, exist_ok=True)
    for relative in sorted(source_files):
        shutil.copy2(ROOT / "python" / relative, destination / relative)


def install_cross_platform_wheels(platform_key: str, runtime_root: Path) -> None:
    if platform_key == "windows-x86_64":
        target = runtime_root / "Lib" / "site-packages"
        pip_platform = "win_amd64"
    elif platform_key == "macos-aarch64":
        target = runtime_root / "lib" / "python3.12" / "site-packages"
        pip_platform = "macosx_11_0_arm64"
    else:
        raise RuntimeError(f"unsupported package platform: {platform_key}")
    target.mkdir(parents=True, exist_ok=True)
    run(
        [
            sys.executable,
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--no-cache-dir",
            "--only-binary=:all:",
            "--platform",
            pip_platform,
            "--implementation",
            "cp",
            "--python-version",
            "3.12",
            "--abi",
            "cp312",
            "--target",
            str(target),
            "--requirement",
            str(REQUIREMENTS),
        ]
    )


def pdfium_url(manifest: dict[str, object], asset: str) -> str:
    tag = urllib.parse.quote(str(manifest["release_tag"]), safe="")
    return f"https://github.com/bblanchon/pdfium-binaries/releases/download/{tag}/{asset}"


def copy_pdfium(platform_key: str, executable_directory: Path, legal_directory: Path, scratch: Path) -> dict[str, str]:
    manifest = json.loads(PDFIUM_ARTIFACTS.read_text(encoding="utf-8"))
    record = manifest["artifacts"][platform_key]
    archive = scratch / record["asset"]
    download_verified(
        pdfium_url(manifest, record["asset"]),
        archive,
        record["archive_sha256"],
        int(record["size_bytes"]),
    )
    unpacked = scratch / "pdfium"
    extract_tar(archive, unpacked)
    source_library = unpacked / record["library_member"]
    if not source_library.is_file():
        raise RuntimeError(f"Pdfium library missing from archive: {record['library_member']}")
    actual = sha256_file(source_library)
    if actual != record["library_sha256"]:
        raise RuntimeError(f"Pdfium library hash mismatch: {actual} != {record['library_sha256']}")
    target_name = "pdfium.dll" if platform_key == "windows-x86_64" else "libpdfium.dylib"
    shutil.copy2(source_library, executable_directory / target_name)
    for candidate in [unpacked / "LICENSE", unpacked / "licenses"]:
        if candidate.is_file():
            shutil.copy2(candidate, legal_directory / "PDFIUM-LICENSE")
        elif candidate.is_dir():
            shutil.copytree(candidate, legal_directory / "pdfium-licenses", dirs_exist_ok=True)
    return {
        "source_release": str(manifest["release_tag"]),
        "archive_sha256": record["archive_sha256"],
        "library_sha256": actual,
        "library": target_name,
    }


def runtime_executable(platform_key: str, runtime_root: Path) -> Path:
    if platform_key == "windows-x86_64":
        return runtime_root / "python.exe"
    return runtime_root / "bin" / "python3"


def can_execute_target(platform_key: str) -> bool:
    machine = host_platform.machine().lower()
    if platform_key == "windows-x86_64":
        return os.name == "nt" and machine in {"amd64", "x86_64"}
    if platform_key == "macos-aarch64":
        return sys.platform == "darwin" and machine in {"arm64", "aarch64"}
    return False


def verify_worker(platform_key: str, python_root: Path) -> None:
    interpreter = runtime_executable(platform_key, python_root / "runtime")
    if not interpreter.is_file():
        raise RuntimeError(f"bundled interpreter missing: {interpreter}")
    if not can_execute_target(platform_key):
        return
    environment = os.environ.copy()
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONPATH"] = str(python_root)
    environment.pop("PYMUPDF_PRO_KEY", None)
    run(
        [str(interpreter), str(python_root / "verify_runtime_manifest.py"), "--tier", "pro"],
        cwd=python_root,
        env=environment,
    )
    run([str(interpreter), str(ROOT / "python" / "smoke_test.py")], env=environment)


def verify_application(
    platform_key: str,
    executable: Path,
    python_root: Path,
) -> None:
    if not can_execute_target(platform_key):
        return
    environment = os.environ.copy()
    environment["PYTHONNOUSERSITE"] = "1"
    environment["PYTHONPATH"] = str(python_root)
    environment["DUAL_CORE_PASSPHRASE"] = "portable-package-smoke-passphrase"
    environment.pop("PYMUPDF_PRO_KEY", None)
    with tempfile.TemporaryDirectory(prefix="portable-doctor-") as working_directory:
        result = subprocess.run(
            [str(executable), "doctor"],
            cwd=working_directory,
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
    output = result.stdout.decode("utf-8", errors="replace")
    if result.returncode not in (0, 2):
        raise RuntimeError(
            f"packaged first-run diagnostics failed ({result.returncode}):\n{output}"
        )
    if "Bank templates" not in output or "0 template(s) found" in output:
        raise RuntimeError(f"packaged templates are not discoverable:\n{output}")


def write_bundle_manifest(bundle_root: Path, platform_key: str, revision: str, pdfium: dict[str, str]) -> None:
    files = []
    for path in sorted(bundle_root.rglob("*")):
        if path.is_file() and path.name != "bundle-manifest.json":
            files.append(
                {
                    "path": path.relative_to(bundle_root).as_posix(),
                    "size_bytes": path.stat().st_size,
                    "sha256": sha256_file(path),
                }
            )
    manifest = {
        "schema_version": 1,
        "application": APP_NAME,
        "version": "1.1.1",
        "revision": revision,
        "platform": platform_key,
        "python": json.loads(PYTHON_ARTIFACTS.read_text(encoding="utf-8"))["python_version"],
        "pymupdf": "1.28.0",
        "pymupdf_pro": "1.28.0 (license required for Pro operations)",
        "local_llm": "unavailable",
        "pdfium": pdfium,
        "signature": "unsigned-build-artifact",
        "files": files,
    }
    (bundle_root / "bundle-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def build(args: argparse.Namespace) -> Path:
    platform_key = args.platform
    binary = args.binary.resolve()
    if not binary.is_file():
        raise RuntimeError(f"production binary missing: {binary}")
    output_root = args.output.resolve()
    if output_root.exists():
        shutil.rmtree(output_root)
    output_root.mkdir(parents=True)

    if platform_key == "windows-x86_64":
        bundle_root = output_root / APP_NAME
        executable_directory = bundle_root
        resources_directory = bundle_root / "resources"
        destination_binary = executable_directory / f"{APP_NAME}.exe"
    elif platform_key == "macos-aarch64":
        bundle_root = output_root / f"{APP_NAME}.app"
        executable_directory = bundle_root / "Contents" / "MacOS"
        resources_directory = bundle_root / "Contents" / "Resources"
        destination_binary = executable_directory / BINARY_NAME
    else:
        raise RuntimeError(f"unsupported platform: {platform_key}")

    executable_directory.mkdir(parents=True)
    resources_directory.mkdir(parents=True)
    legal_directory = resources_directory / "licenses"
    legal_directory.mkdir(parents=True)
    shutil.copy2(binary, destination_binary)
    destination_binary.chmod(destination_binary.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    shutil.copy2(ROOT / "LICENSE", legal_directory / "APPLICATION-LICENSE")
    shutil.copy2(PYTHON_ARTIFACTS, legal_directory / "python-runtime-artifacts.json")
    shutil.copy2(PDFIUM_ARTIFACTS, legal_directory / "pdfium-artifacts.json")
    shutil.copytree(ROOT / "bank_templates", resources_directory / "bank_templates")
    shutil.copytree(ROOT / "assets", resources_directory / "assets")
    (resources_directory / "capabilities.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "offline_core": True,
                "bundled_python": True,
                "bundled_pdfium": True,
                "pymupdf_pro": {
                    "bundled": True,
                    "license_required": True,
                },
                "local_llm": "unavailable",
                "font_substitution": "disabled",
                "typst_reconstruction": "disabled",
                "signed": False,
                "notarized": False,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    with tempfile.TemporaryDirectory(prefix="portable-bundle-") as temporary:
        scratch = Path(temporary)
        python_manifest = json.loads(PYTHON_ARTIFACTS.read_text(encoding="utf-8"))
        python_record = python_manifest["artifacts"][platform_key]
        python_archive = scratch / python_record["asset"]
        download_verified(
            python_record["url"],
            python_archive,
            python_record["archive_sha256"],
            int(python_record["size_bytes"]),
        )
        unpacked = scratch / "python-unpacked"
        extract_tar(python_archive, unpacked)
        source_runtime = unpacked / "python"
        if not source_runtime.is_dir():
            raise RuntimeError("standalone Python archive has no python/ root")
        python_root = resources_directory / "python"
        shutil.copytree(source_runtime, python_root / "runtime", symlinks=True)
        install_cross_platform_wheels(platform_key, python_root / "runtime")
        copy_runtime_sources(python_root)
        pdfium = copy_pdfium(platform_key, executable_directory, legal_directory, scratch)
        verify_worker(platform_key, python_root)

    if platform_key == "macos-aarch64":
        info = {
            "CFBundleDevelopmentRegion": "en",
            "CFBundleExecutable": BINARY_NAME,
            "CFBundleIdentifier": "io.github.flak3dd.bank-statement-fidelity-editor",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleName": APP_NAME,
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": "1.1.1",
            "CFBundleVersion": "1.1.1",
            "LSMinimumSystemVersion": "12.0",
            "NSHighResolutionCapable": True,
        }
        with (bundle_root / "Contents" / "Info.plist").open("wb") as stream:
            plistlib.dump(info, stream, sort_keys=True)

    (resources_directory / "README-FIRST.txt").write_text(
        "Bank Statement Fidelity Editor 1.1.1\n\n"
        "This portable CI artifact contains a pinned Python/PyMuPDF/PyMuPDF Pro runtime and pinned Pdfium. "
        "PyMuPDF Pro operations require a separately configured valid license key. "
        "This artifact is unsigned and not notarized; production distribution requires owner signing credentials.\n",
        encoding="utf-8",
    )
    write_bundle_manifest(bundle_root, platform_key, args.revision, pdfium)
    verify_application(platform_key, destination_binary, resources_directory / "python")
    print(bundle_root)
    return bundle_root


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", required=True, choices=["windows-x86_64", "macos-aarch64"])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    return parser.parse_args()


if __name__ == "__main__":
    build(parse_args())
