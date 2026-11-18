import hashlib
import json
import plistlib
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from package import create_package, host_target, validate_binary_target
from verify_package import MAC_BINARY, MAC_PLIST, verify_package


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "test-executable"
        self.binary.write_bytes(b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01" + bytes(56))
        self.output = self.root / "packages"

    def windows_binary(self):
        header = bytearray(64)
        header[:2] = b"MZ"
        header[60:64] = (64).to_bytes(4, "little")
        self.binary.write_bytes(header + b"PE\x00\x00\x64\x86")

    def rewrite_archive(self, artifact, change):
        with zipfile.ZipFile(artifact) as zipped:
            entries = [(info, zipped.read(info.filename)) for info in zipped.infolist()]
        with zipfile.ZipFile(artifact, "w") as zipped:
            for info, data in entries:
                info, data = change(info, data)
                zipped.writestr(info, data)

    def test_macos_bundle_has_matching_executable_and_launch_metadata(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        with zipfile.ZipFile(artifact) as zipped:
            prefix = "Storage Clinic.app/Contents/"
            info = plistlib.loads(zipped.read(prefix + "Info.plist"))
            binary = prefix + "MacOS/" + info["CFBundleExecutable"]
            self.assertEqual(zipped.read(binary), self.binary.read_bytes())
            self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
            self.assertEqual(info["CFBundlePackageType"], "APPL")
            self.assertEqual(info["LSMinimumSystemVersion"], "15.0")
            self.assertTrue((zipped.getinfo(binary).external_attr >> 16) & stat.S_IXUSR)
            self.assertIn(b"unsigned development", zipped.read("READ-ME.txt"))
        self.assertEqual(list(self.output.iterdir()), [artifact])
        manifest = verify_package(artifact, "macos-arm64", "0.1.0")
        self.assertEqual(manifest["sha256"][MAC_BINARY], hashlib.sha256(self.binary.read_bytes()).hexdigest())

    def test_windows_package_contains_the_executable_without_a_macos_bundle(self):
        self.windows_binary()
        artifact = create_package(self.binary, "0.1.0", "windows-x64", self.output)
        with zipfile.ZipFile(artifact) as zipped:
            self.assertEqual(set(zipped.namelist()), {"storage-clinic.exe", "READ-ME.txt", "manifest.json"})
            self.assertEqual(zipped.read("storage-clinic.exe"), self.binary.read_bytes())
        verify_package(artifact, "windows-x64", "0.1.0")

    def test_validation_failure_preserves_previous_artifact(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        original = artifact.read_bytes()
        with patch("package.verify_package", side_effect=ValueError("invalid archive")):
            with self.assertRaises(ValueError):
                create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        self.assertEqual(artifact.read_bytes(), original)
        self.assertEqual(list(self.output.iterdir()), [artifact])

    def test_wrong_architecture_cannot_replace_existing_package(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        original = artifact.read_bytes()
        self.windows_binary()
        with self.assertRaisesRegex(ValueError, "does not match"):
            create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        self.assertEqual(artifact.read_bytes(), original)
        self.assertEqual(list(self.output.iterdir()), [artifact])

    def test_verifier_rejects_changed_payload(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        self.rewrite_archive(artifact, lambda info, data: (info, b"changed" if info.filename == MAC_BINARY else data))
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            verify_package(artifact)

    def test_verifier_rejects_unexpected_files_and_duplicate_entries(self):
        for extra in ("../outside", "READ-ME.txt"):
            artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
            with zipfile.ZipFile(artifact, "a") as zipped:
                if extra == "READ-ME.txt":
                    with self.assertWarns(UserWarning):
                        zipped.writestr(extra, b"extra")
                else:
                    zipped.writestr(extra, b"extra")
            with self.assertRaises(ValueError):
                verify_package(artifact)

    def test_verifier_checks_requested_target_and_version(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        for target, version in (("windows-x64", "0.1.0"), ("macos-arm64", "9.0.0")):
            with self.assertRaisesRegex(ValueError, "requested build"):
                verify_package(artifact, target, version)

    def test_verifier_rejects_missing_execute_permission_and_symlinks(self):
        for mode in (0o100644, 0o120755):
            artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)

            def change(info, data):
                if info.filename == MAC_BINARY:
                    info.external_attr = mode << 16
                return info, data

            self.rewrite_archive(artifact, change)
            with self.assertRaises(ValueError):
                verify_package(artifact)

    def test_verifier_checks_launch_metadata_even_with_matching_hash(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        with zipfile.ZipFile(artifact) as zipped:
            plist = plistlib.loads(zipped.read(MAC_PLIST))
        plist["CFBundleExecutable"] = "wrong-name"
        replacement = plistlib.dumps(plist)

        def change(info, data):
            if info.filename == MAC_PLIST:
                data = replacement
            elif info.filename == "manifest.json":
                manifest = json.loads(data)
                manifest["sha256"][MAC_PLIST] = hashlib.sha256(replacement).hexdigest()
                data = json.dumps(manifest).encode()
            return info, data

        self.rewrite_archive(artifact, change)
        with self.assertRaisesRegex(ValueError, "launch metadata"):
            verify_package(artifact)

    def test_verifier_rejects_invalid_manifest_and_zip(self):
        for manifest in (b"[]", b"{}", b"{broken", b"x" * 65537):
            artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
            self.rewrite_archive(artifact, lambda info, data: (info, manifest if info.filename == "manifest.json" else data))
            with self.assertRaises(ValueError):
                verify_package(artifact)
        artifact.write_bytes(b"not a zip")
        with self.assertRaises(zipfile.BadZipFile):
            verify_package(artifact)

    def test_failed_packaging_preserves_previous_artifact_and_removes_staging(self):
        artifact = create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        original = artifact.read_bytes()
        with patch("package.zipfile.ZipFile.write", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                create_package(self.binary, "0.1.0", "macos-arm64", self.output)
        self.assertEqual(artifact.read_bytes(), original)
        self.assertEqual(list(self.output.iterdir()), [artifact])

    def test_rejects_unsupported_target_and_unsafe_version(self):
        for version, target in [("../elsewhere", "macos-arm64"), ("0.1.0", "linux-x64")]:
            with self.assertRaises(ValueError):
                create_package(self.binary, version, target, self.output)
        self.assertFalse(self.output.exists())

    def test_supported_hosts_have_explicit_architectures(self):
        for system, machine, expected in [("darwin", "arm64", "macos-arm64"), ("win32", "AMD64", "windows-x64")]:
            with patch("package.sys.platform", system), patch("package.platform.machine", return_value=machine):
                self.assertEqual(host_target(), expected)
        with patch("package.sys.platform", "linux"):
            with self.assertRaises(ValueError):
                host_target()

    def test_binary_architecture_must_match_the_package_target(self):
        self.binary.write_bytes(b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01" + bytes(56))
        validate_binary_target(self.binary, "macos-arm64")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "windows-x64")
        self.windows_binary()
        validate_binary_target(self.binary, "windows-x64")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "macos-arm64")
        self.binary.write_bytes(b"truncated")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "windows-x64")
        self.binary.write_bytes(b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "macos-arm64")


if __name__ == "__main__":
    unittest.main()
