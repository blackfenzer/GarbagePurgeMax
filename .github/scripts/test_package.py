import plistlib
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from package import create_package, host_target, validate_binary_target


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "test-executable"
        self.binary.write_bytes(b"synthetic executable\x00\xff")
        self.output = self.root / "packages"

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

    def test_windows_package_contains_the_executable_without_a_macos_bundle(self):
        artifact = create_package(self.binary, "0.1.0", "windows-x64", self.output)
        with zipfile.ZipFile(artifact) as zipped:
            self.assertEqual(set(zipped.namelist()), {"storage-clinic.exe", "READ-ME.txt"})
            self.assertEqual(zipped.read("storage-clinic.exe"), self.binary.read_bytes())

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
        header = bytearray(64)
        header[:2] = b"MZ"
        header[60:64] = (64).to_bytes(4, "little")
        self.binary.write_bytes(header + b"PE\x00\x00\x64\x86")
        validate_binary_target(self.binary, "windows-x64")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "macos-arm64")
        self.binary.write_bytes(b"truncated")
        with self.assertRaises(ValueError):
            validate_binary_target(self.binary, "windows-x64")


if __name__ == "__main__":
    unittest.main()
