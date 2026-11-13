"""Windows Rust payload/ZIP/setup. Never installs or starts a bundled executable."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
import zipfile
from pathlib import Path

from build import ROOT, VERSION, write


FILES = {'seiya-desktop.exe', 'seiya-server.exe', 'README.md', 'LICENSE',
         'THIRD_PARTY_NOTICES.md', 'server.discovery.example.json', 'BUILD-INFO.json',
         'Seiya-Rust-Help.cmd'}


def windows_binary(path):
    if path.is_symlink() or not path.is_file() or not 200 <= path.stat().st_size <= 256 * 1024 * 1024:
        raise SystemExit('Regular bounded Windows x64 executable required.')
    with path.open('rb') as file:
        dos = file.read(64)
        offset = int.from_bytes(dos[60:64], 'little')
        if dos[:2] != b'MZ' or not 64 <= offset <= min(path.stat().st_size - 136, 1024 * 1024):
            raise SystemExit('Invalid bounded DOS/PE header.')
        file.seek(offset)
        pe = file.read(136)
    characteristics = int.from_bytes(pe[22:24], 'little')
    if (pe[:4] != b'PE\0\0' or int.from_bytes(pe[4:6], 'little') != 0x8664
            or int.from_bytes(pe[20:22], 'little') < 112
            or offset + 24 + int.from_bytes(pe[20:22], 'little') > path.stat().st_size
            or int.from_bytes(pe[24:26], 'little') != 0x20B
            or not characteristics & 2 or characteristics & 0x2000
            or int.from_bytes(pe[24 + 68:24 + 70], 'little') not in (2, 3)):
        raise SystemExit('Expected PE32+ Windows x64 executable, not DLL/x86/ARM64.')


def stage_payload(root, binaries, revision=None):
    if revision is not None and not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise SystemExit('Revision must be an explicit full Git SHA, or omitted.')
    info = {'version': VERSION, 'platform': 'windows', 'arch': 'x64', 'revision': revision, 'binaries': {}}
    for name in ('seiya-desktop.exe', 'seiya-server.exe'):
        source = binaries / name
        windows_binary(source)
        root.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, root / name)
        windows_binary(root / name)
        info['binaries'][name] = hashlib.sha256((root / name).read_bytes()).hexdigest()
    for source, name in [('docs/WINDOWS_RUST.md', 'README.md'), ('LICENSE', 'LICENSE'),
                         ('THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_NOTICES.md'),
                         ('contracts/desktop/windows/server.discovery.example.json', 'server.discovery.example.json')]:
        write(root, name, (ROOT.parent / source).read_bytes())
    write(root, 'BUILD-INFO.json', json.dumps(info, indent=2) + '\n')
    write(root, 'Seiya-Rust-Help.cmd', '@echo off\r\n"%~dp0seiya-desktop.exe" --help\r\npause\r\n')
    inspect_payload(root)


def inspect_payload(root):
    paths = list(root.rglob('*'))
    if any(p.is_symlink() or not p.is_file() for p in paths) or {p.relative_to(root).as_posix() for p in paths if p.is_file()} != FILES:
        raise SystemExit('Windows payload differs from explicit flat allowlist.')
    info = json.loads((root / 'BUILD-INFO.json').read_text())
    if info['platform'] != 'windows' or info['arch'] != 'x64':
        raise SystemExit('Windows payload metadata mismatch.')
    for name in ('seiya-desktop.exe', 'seiya-server.exe'):
        windows_binary(root / name)
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != info['binaries'][name]:
            raise SystemExit('Windows binary hash mismatch.')


def portable_zip(root, output):
    inspect_payload(root)
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(FILES):
            archive.write(root / name, name)


def setup(root, output, compiler):
    inspect_payload(root)
    if not compiler.is_file():
        raise SystemExit('Preinstalled InnoSetup compiler required; no automatic install/download.')
    for path in (root.resolve(), output.resolve()):
        if any(char in str(path) for char in '\r\n"{}'):
            raise SystemExit('Installer staging/output paths contain unsupported macro characters.')
    subprocess.run([str(compiler), f'/DStageDir={root.resolve()}', f'/DOutputDir={output.resolve()}',
                    f'/DPackageVersion={VERSION}', str(ROOT / 'packaging/windows.iss')], check=True)
    result = output / f'Seiya-Rust-{VERSION}-windows-x64-setup.exe'
    if not result.is_file() or result.is_symlink() or result.stat().st_size < 64:
        raise SystemExit('InnoSetup did not produce a regular setup executable.')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('target', choices=['zip', 'setup', 'inspect'])
    parser.add_argument('--bin-dir', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--payload-dir', type=Path)
    parser.add_argument('--compiler', type=Path)
    parser.add_argument('--revision')
    args = parser.parse_args()
    if args.target == 'inspect':
        if not args.payload_dir:
            parser.error('inspect requires --payload-dir')
        inspect_payload(args.payload_dir)
        print('Windows PE x64, allowlist and hashes OK (no installation/inference).')
        return
    if not args.output or args.target == 'setup' and not args.compiler:
        parser.error('build requires --output; setup also requires --compiler')
    if args.payload_dir and (args.target != 'setup' or args.bin_dir or args.revision):
        parser.error('existing payload is only for setup, without --bin-dir/--revision')
    if not args.payload_dir and not args.bin_dir:
        parser.error('build requires --bin-dir or an existing setup --payload-dir')
    args.output.mkdir(parents=True, exist_ok=True)
    if args.payload_dir:
        result = setup(args.payload_dir, args.output, args.compiler)
    else:
        with tempfile.TemporaryDirectory(prefix='seiya-windows-') as tmp:
            root = Path(tmp) / 'payload'
            stage_payload(root, args.bin_dir, args.revision)
            if args.target == 'zip':
                result = args.output / f'Seiya-Rust-{VERSION}-windows-x64.zip'
                portable_zip(root, result)
            else:
                result = setup(root, args.output, args.compiler)
    (args.output / 'SHA256SUMS').write_text(f'{hashlib.sha256(result.read_bytes()).hexdigest()}  {result.name}\n')
    print(result)


if __name__ == '__main__':
    main()
