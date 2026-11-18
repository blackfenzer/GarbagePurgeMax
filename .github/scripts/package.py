#!/usr/bin/env python3
"""Build an unsigned native package with the locked Cargo dependencies."""

import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import zipfile

from verify_package import payload_paths, sha256_stream, validate_binary_target, validate_version, verify_package


ROOT = Path(__file__).resolve().parents[2]


def host_target():
    machine = platform.machine().lower()
    if sys.platform == "darwin" and machine in ("arm64", "aarch64"):
        return "macos-arm64"
    if sys.platform == "win32" and machine in ("amd64", "x86_64"):
        return "windows-x64"
    raise ValueError("Packaging supports Apple Silicon macOS and Windows x64 hosts only")


def build_executable(root):
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=root, text=True,
    ))
    package = next(item for item in metadata["packages"] if item["name"] == "storage-clinic")
    # Use Cargo's reported artifact path, including any configured target directory/triple.
    process = subprocess.Popen(
        ["cargo", "build", "--locked", "--release", "-p", "storage-clinic",
         "--message-format=json-render-diagnostics"],
        cwd=root, stdout=subprocess.PIPE, text=True,
    )
    executable = None
    with process.stdout:
        for line in process.stdout:
            message = json.loads(line)
            if (message.get("reason") == "compiler-artifact"
                    and message.get("package_id") == package["id"]
                    and message.get("executable")):
                executable = Path(message["executable"])
    if process.wait() != 0:
        raise RuntimeError("The release build failed; no package was written")
    if executable is None or not executable.is_file():
        raise RuntimeError("Cargo did not produce the desktop executable")
    return executable, package["version"], Path(metadata["target_directory"])


def create_package(executable, version, target, output):
    files = payload_paths(target)
    validate_version(version)
    validate_binary_target(executable, target)
    output.mkdir(parents=True, exist_ok=True)
    destination = output / f"Storage-Clinic-{version}-{target}.zip"
    # Assemble only generated files. A failed build/archive leaves an existing ZIP intact.
    with tempfile.TemporaryDirectory(prefix="storage-clinic-package-", dir=output) as temporary:
        staging = Path(temporary)
        payload = staging / "payload"
        payload.mkdir()
        if target == "macos-arm64":
            contents = payload / "Storage Clinic.app" / "Contents"
            binary = contents / "MacOS" / "storage-clinic"
            binary.parent.mkdir(parents=True)
            shutil.copyfile(executable, binary)
            binary.chmod(0o755)
            info = {
                "CFBundleIdentifier": "com.storageclinic.desktop",
                "CFBundleName": "Storage Clinic",
                "CFBundleDisplayName": "Storage Clinic",
                "CFBundleExecutable": "storage-clinic",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": version,
                "CFBundleVersion": version,
                "LSMinimumSystemVersion": "15.0",
                "NSHighResolutionCapable": True,
            }
            with (contents / "Info.plist").open("wb") as file:
                plistlib.dump(info, file)
            launch = "Extract the ZIP and open Storage Clinic.app on an unlocked Mac."
        else:
            shutil.copyfile(executable, payload / "storage-clinic.exe")
            launch = "Extract the ZIP and run storage-clinic.exe on Windows 11 x64."
        (payload / "READ-ME.txt").write_text(
            f"Storage Clinic {version}\n\n{launch}\n\n"
            "This is an unsigned development package, not a signed release.\n"
            "macOS notarization and Windows code signing have not been performed.\n"
            "Your OS may block downloaded unsigned applications. If blocked, use the\n"
            "documented source build workflow instead of disabling security settings.\n\n"
            "Scans are local and read-only. Reviewed cleanup requires explicit confirmation.\n"
            "Cleanup moves eligible items to OS Trash / Recycle Bin; it never empties Trash.\n"
            "Recovery depends on retained OS trash contents and the local operation journal.\n"
            "Native cleanup/restore acceptance is pending; use disposable fixtures first.\n"
            "History, exclusions and recovery records use the normal app-data directory.\n",
            encoding="utf-8",
        )
        hashes = {}
        for name in sorted(files):
            with (payload / name).open("rb") as file:
                hashes[name] = sha256_stream(file)
        (payload / "manifest.json").write_text(json.dumps({
            "schema_version": 1, "version": version, "target": target, "sha256": hashes,
        }, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        archive = staging / "package.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zipped:
            for file in sorted(payload.rglob("*")):
                if file.is_file():
                    relative = file.relative_to(payload).as_posix()
                    zipped.write(file, relative)
                    if target == "macos-arm64" and file == binary:
                        # Preserve executable permissions even in cross-platform layout tests.
                        info = zipped.getinfo(relative)
                        info.create_system = 3
                        info.external_attr = 0o100755 << 16
        verify_package(archive, target, version)
        os.replace(archive, destination)
    return destination


def main():
    target = host_target()
    executable, version, target_directory = build_executable(ROOT)
    artifact = create_package(executable, version, target, target_directory / "packages")
    print(f"Unsigned package: {artifact}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        sys.exit(1)
