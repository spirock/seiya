import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "release_audit", Path(__file__).resolve().parents[1] / "packaging/release_audit.py"
)
release_audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_audit)

NAMES = {
    "macos-dmg": "Seiya-Rust-0.1.0-arm64.dmg",
    "macos-pkg": "Seiya-Rust-0.1.0-arm64.pkg",
    "linux-deb": "seiya-rust-experimental_0.1.0_amd64.deb",
    "windows-zip": "Seiya-Rust-0.1.0-windows-x64.zip",
    "windows-setup": "Seiya-Rust-0.1.0-windows-x64-setup.exe",
}


class ReleaseAuditTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for role, name in NAMES.items():
            directory = self.root / role
            directory.mkdir()
            (directory / name).write_bytes(b"synthetic package")
            digest = hashlib.sha256(b"synthetic package").hexdigest()
            (directory / "SHA256SUMS").write_text(f"{digest}  {name}\n")

    def test_all_five_matching_hashes(self):
        self.assertEqual(set(release_audit.audit(self.root)), set(NAMES))

    def test_tampering_and_bad_manifests_fail_closed(self):
        payload = self.root / "windows-zip" / NAMES["windows-zip"]
        payload.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum_mismatch"):
            release_audit.audit(self.root)
        payload.write_bytes(b"synthetic package")
        (payload.parent / "SHA256SUMS").write_text("../private\n")
        with self.assertRaisesRegex(ValueError, "invalid_checksum"):
            release_audit.audit(self.root)

    def test_missing_extra_and_symlink_are_rejected(self):
        extra = self.root / "extra"
        extra.mkdir()
        with self.assertRaises(ValueError):
            release_audit.audit(self.root)
        extra.rmdir()
        payload = self.root / "linux-deb" / NAMES["linux-deb"]
        payload.unlink()
        with self.assertRaises(ValueError):
            release_audit.audit(self.root)
        payload.symlink_to(self.root / "windows-zip" / NAMES["windows-zip"])
        with self.assertRaises(ValueError):
            release_audit.audit(self.root)

    def test_mixed_nominal_versions_are_rejected_even_with_matching_hashes(self):
        directory = self.root / "macos-pkg"
        old = directory / NAMES["macos-pkg"]
        name = "Seiya-Rust-0.2.0-arm64.pkg"
        old.rename(directory / name)
        digest = hashlib.sha256(b"synthetic package").hexdigest()
        (directory / "SHA256SUMS").write_text(f"{digest}  {name}\n")
        with self.assertRaisesRegex(ValueError, "asset_versions_differ"):
            release_audit.audit(self.root)


if __name__ == "__main__":
    unittest.main()
