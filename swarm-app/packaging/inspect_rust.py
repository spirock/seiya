"""Inspect an extracted Rust payload without installing or starting anything."""
import argparse
from pathlib import Path
from build import verify_rust_stage


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--platform', choices=['macos', 'linux'], required=True)
    parser.add_argument('--arch', choices=['amd64', 'arm64'], required=True)
    args = parser.parse_args()
    verify_rust_stage(args.root, args.platform == 'macos', args.arch)
    print('Rust payload allowlist, binary target and hashes OK (no installation/inference).')


if __name__ == '__main__':
    main()
