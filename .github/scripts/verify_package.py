#!/usr/bin/env python3
"""Check a development ZIP without extracting or executing its contents."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import re
import stat
import sys
import zipfile


MAC_BINARY = "Storage Clinic.app/Contents/MacOS/storage-clinic"
MAC_PLIST = "Storage Clinic.app/Contents/Info.plist"


def validate_version(version):
    if not isinstance(version, str) or not re.fullmatch(r"[0-9A-Za-z.+_-]+", version):
        raise ValueError("Invalid package version")


def payload_paths(target):
    if target == "macos-arm64":
        return {MAC_BINARY, MAC_PLIST, "READ-ME.txt"}
    if target == "windows-x64":
        return {"storage-clinic.exe", "READ-ME.txt"}
    raise ValueError("Unsupported package target")


def sha256_stream(file):
    digest = hashlib.sha256()
    for chunk in iter(lambda: file.read(1024 * 1024), b""):
        digest.update(chunk)
    return digest.hexdigest()


def validate_binary_stream(file, target):
    header = file.read(64)
    valid = False
    if len(header) == 64 and target == "macos-arm64":
        valid = (header[:4] == b"\xcf\xfa\xed\xfe"
                 and int.from_bytes(header[4:8], "little") == 0x0100000C)
    elif len(header) == 64 and target == "windows-x64" and header[:2] == b"MZ":
        offset = int.from_bytes(header[60:64], "little")
        # Bound seeking in compressed or malformed inputs; native PE headers are near the start.
        if 64 <= offset <= 1024 * 1024:
            file.seek(offset)
            valid = file.read(6) == b"PE\x00\x00\x64\x86"
    if not valid:
        raise ValueError(f"Executable does not match {target}; check Cargo cross-compilation settings")


def validate_binary_target(executable, target):
    with executable.open("rb") as file:
        validate_binary_stream(file, target)


def read_small(zipped, name):
    if zipped.getinfo(name).file_size > 64 * 1024:
        raise ValueError(f"Package metadata is too large: {name}")
    return zipped.read(name)


def verify_package(archive, expected_target=None, expected_version=None):
    with zipfile.ZipFile(archive) as zipped:
        names = zipped.namelist()
        if len(names) != len(set(names)) or "manifest.json" not in names:
            raise ValueError("Duplicate archive entries or missing manifest")
        manifest = json.loads(read_small(zipped, "manifest.json"))
        if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
            raise ValueError("Unsupported package manifest")
        target = manifest.get("target")
        version = manifest.get("version")
        validate_version(version)
        expected_files = payload_paths(target)
        if ((expected_target is not None and target != expected_target)
                or (expected_version is not None and version != expected_version)):
            raise ValueError("Package target or version does not match the requested build")
        hashes = manifest.get("sha256")
        if (set(names) != expected_files | {"manifest.json"}
                or not isinstance(hashes, dict) or set(hashes) != expected_files):
            raise ValueError("Package layout does not match its target")
        for name in names:
            mode = zipped.getinfo(name).external_attr >> 16
            if stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                raise ValueError(f"Package entry is not a regular file: {name}")
        for name, expected_hash in hashes.items():
            with zipped.open(name) as file:
                if sha256_stream(file) != expected_hash:
                    raise ValueError(f"Package checksum mismatch: {name}")
        binary = MAC_BINARY if target == "macos-arm64" else "storage-clinic.exe"
        with zipped.open(binary) as file:
            validate_binary_stream(file, target)
        if target == "macos-arm64":
            info = plistlib.loads(read_small(zipped, MAC_PLIST))
            required = {
                "CFBundleIdentifier": "com.storageclinic.desktop",
                "CFBundleExecutable": "storage-clinic",
                "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": version,
                "CFBundleVersion": version,
                "LSMinimumSystemVersion": "15.0",
            }
            if not isinstance(info, dict) or any(info.get(k) != v for k, v in required.items()):
                raise ValueError("Invalid macOS launch metadata")
            if not (zipped.getinfo(binary).external_attr >> 16) & stat.S_IXUSR:
                raise ValueError("macOS executable permission is missing")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--target", choices=("macos-arm64", "windows-x64"))
    parser.add_argument("--version")
    args = parser.parse_args()
    manifest = verify_package(args.archive, args.target, args.version)
    print(f"Verified {manifest['version']} {manifest['target']}: {args.archive}")
    print("Checksums verify consistency, not publisher identity; this package is unsigned.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, zipfile.BadZipFile, plistlib.InvalidFileException, RuntimeError) as error:
        print(f"Package verification failed: {error}", file=sys.stderr)
        sys.exit(1)
