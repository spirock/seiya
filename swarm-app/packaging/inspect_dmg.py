"""Inspect a mounted app DMG read-only. Never opens or installs its application."""
import argparse
import hashlib
import json
from pathlib import Path
from build import rust_binary


def inspect(root, arch, revision=None):
    if {p.name for p in root.iterdir()} != {'Seiya Rust.app', 'Applications', 'README.md', 'SHA256SUMS'}:
        raise SystemExit('Unexpected DMG root payload.')
    link = root / 'Applications'
    if not link.is_symlink() or link.readlink() != Path('/Applications'):
        raise SystemExit('Expected Applications shortcut only.')
    app = root / 'Seiya Rust.app'
    if app.is_symlink() or not app.is_dir():
        raise SystemExit('Regular app bundle required.')
    files = set()
    for path in app.rglob('*'):
        if path.is_symlink():
            raise SystemExit('Unexpected symlink inside app bundle.')
        if path.is_file():
            files.add(path.relative_to(root).as_posix())
    expected = {'Seiya Rust.app/Contents/' + name for name in (
        'Info.plist', 'MacOS/Seiya', 'MacOS/seiya-desktop', 'MacOS/seiya-server',
        'Resources/Seiya.command', *('Resources/' + n for n in (
            'README.md','APP_RUST.md','PARIDAD_RUST.md','BACKENDS_LOCALES.md',
            'IMAGENES_DESDE_CHAT.md','RESIDENCIA_NATIVA.md','PRUEBA_INSTALADOR.md',
            'desktop-chat-images.example.json','local-models-images.example.json',
            'LICENSE','THIRD_PARTY_NOTICES.md','desktop.example.json',
            'local-models.example.json','LOCAL_MODELS.md','BUILD-INFO.json')))}
    if files != expected:
        raise SystemExit('DMG app differs from explicit allowlist.')
    resources = app / 'Contents/Resources'
    info = json.loads((resources / 'BUILD-INFO.json').read_text())
    if info['platform'] != 'macos' or info['arch'] != arch or (revision and info['revision'] != revision):
        raise SystemExit('DMG app build target/revision mismatch.')
    for name in ('seiya-desktop','seiya-server'):
        file = app / 'Contents/MacOS' / name
        rust_binary(file, True, arch)
        if hashlib.sha256(file.read_bytes()).hexdigest() != info['binaries'][name]:
            raise SystemExit('DMG binary hash mismatch.')
    observed = set()
    for line in (root / 'SHA256SUMS').read_text().splitlines():
        digest, name = line.split('  ', 1)
        if name not in files | {'README.md'} or name in observed:
            raise SystemExit('Invalid DMG manifest path.')
        observed.add(name)
        if hashlib.sha256((root / name).read_bytes()).hexdigest() != digest:
            raise SystemExit('DMG manifest hash mismatch.')
    if observed != files | {'README.md'}:
        raise SystemExit('Incomplete DMG manifest.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('root', type=Path)
    parser.add_argument('--arch', required=True, choices=('amd64','arm64'))
    parser.add_argument('--revision')
    args = parser.parse_args()
    inspect(args.root, args.arch, args.revision)
    print('App DMG allowlist, Applications link, target/revision and hashes OK; no installation.')
