"""Offline release asset checksum audit; does not install or publish packages."""

import argparse
import hashlib
from pathlib import Path
import re
import sys


EXPECTED = {
    "macos-dmg": re.compile(r"Seiya-Rust-(\d+\.\d+\.\d+)-arm64\.dmg", re.ASCII),
    "macos-pkg": re.compile(r"Seiya-Rust-(\d+\.\d+\.\d+)-arm64\.pkg", re.ASCII),
    "linux-deb": re.compile(r"seiya-rust-experimental_(\d+\.\d+\.\d+)_amd64\.deb", re.ASCII),
    "windows-zip": re.compile(r"Seiya-Rust-(\d+\.\d+\.\d+)-windows-x64\.zip", re.ASCII),
    "windows-setup": re.compile(r"Seiya-Rust-(\d+\.\d+\.\d+)-windows-x64-setup\.exe", re.ASCII),
}
CHECKSUM = re.compile(r"([0-9a-f]{64}) [ *]([^\r\n/\\]+)\n?")


def audit(root):
    """Return hashes after strict directory/name checks; no provenance claim."""
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        raise ValueError("invalid_root")
    if {p.name for p in root.iterdir()} != set(EXPECTED):
        raise ValueError("unexpected_asset_directories")
    result = {}
    versions = set()
    for role, pattern in EXPECTED.items():
        directory = root / role
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError("invalid_asset_directory")
        entries = list(directory.iterdir())
        if len(entries) != 2 or any(p.is_symlink() or not p.is_file() for p in entries):
            raise ValueError("invalid_asset_files")
        manifest = directory / "SHA256SUMS"
        payloads = [p for p in entries if p.name != "SHA256SUMS"]
        if not manifest.is_file() or len(payloads) != 1:
            raise ValueError("missing_checksum_or_payload")
        payload = payloads[0]
        name_match = pattern.fullmatch(payload.name)
        if not name_match or manifest.stat().st_size > 512:
            raise ValueError("invalid_asset_name_or_checksum")
        versions.add(name_match.group(1))
        try:
            match = CHECKSUM.fullmatch(manifest.read_text(encoding="ascii"))
        except (OSError, UnicodeError) as exc:
            raise ValueError("invalid_checksum") from exc
        if not match or match.group(2) != payload.name:
            raise ValueError("invalid_checksum")
        digest = hashlib.sha256()
        with payload.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        if digest.hexdigest() != match.group(1):
            raise ValueError("checksum_mismatch")
        result[role] = (payload.name, digest.hexdigest())
    if len(versions) != 1:
        raise ValueError("asset_versions_differ")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    try:
        for role, (name, digest) in audit(args.directory).items():
            print(f"{role} {digest}  {name}")
    except (OSError, ValueError) as exc:
        print(f"release audit failed: {exc}", file=sys.stderr)
        return 1
    print("Names and checksums only: verify run, commit, payload and signatures separately.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
