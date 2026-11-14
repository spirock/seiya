"""Build a Debian archive on any OS and a macOS installer on macOS.

No downloads, elevated privileges, install hooks or bundled model weights.
"""
import argparse
import gzip
import hashlib
import io
import json
import os
import plistlib
import re
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VERSION = '0.1.0'


def write(root, path, content, mode=0o644):
    target = root / path.lstrip('/')
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(content if isinstance(content, bytes) else content.encode())
    target.chmod(mode)


def stage(root, mac=False):
    lib = '/usr/local/lib/seiya' if mac else '/usr/lib/seiya'
    binary = '/usr/local/bin/seiya' if mac else '/usr/bin/seiya'
    for file in (ROOT / 'seiya').iterdir():
        if file.suffix in ('.py', '.html'):
            write(root, lib + '/seiya/' + file.name, file.read_bytes())
    wrapper = f'''#!/bin/sh
export PYTHONPATH="{lib}${{PYTHONPATH:+:$PYTHONPATH}}"
for python in "${{SEIYA_PYTHON:-}}" /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3; do
    if [ -n "$python" ] && [ -x "$python" ] && "$python" -c 'import sys;sys.exit(sys.version_info < (3,10))' 2>/dev/null; then
        exec "$python" -m seiya "$@"
    fi
done
echo "Seiya requires Python 3.10+. Set SEIYA_PYTHON to a compatible executable." >&2
exit 1
'''
    write(root, binary, wrapper, 0o755)
    write(root, '/usr/local/share/doc/seiya/README.md' if mac else '/usr/share/doc/seiya/README.md',
          (ROOT / 'README.md').read_bytes().replace(
              b'](../docs/TRABAJOS_EN_SEGUNDO_PLANO.md)', b'](docs/TRABAJOS_EN_SEGUNDO_PLANO.md)'))
    doc = '/usr/local/share/doc/seiya/' if mac else '/usr/share/doc/seiya/'
    write(root, doc + 'opencode-provider.json', (ROOT / 'opencode-provider.json').read_bytes())
    for name in ('CONTRATO_SEIYA.md', 'PROTOCOLO_V1.md', 'ENTREGA_Y_MIGRACION.md',
                 'MEMORIA_ADAPTATIVA.md', 'TRABAJOS_EN_SEGUNDO_PLANO.md'):
        write(root, doc + 'docs/' + name, (ROOT.parent / 'docs' / name).read_bytes())
    write(root, doc + 'docs/PAQUETES_PRUEBA.md', (ROOT.parent / 'docs/PAQUETES_PRUEBA.md').read_bytes())
    for file in (ROOT.parent / 'contracts' / 'v1').rglob('*'):
        if file.is_file():
            write(root, doc + 'contracts/v1/' + file.relative_to(ROOT.parent / 'contracts' / 'v1').as_posix(),
                   file.read_bytes())
    write(root, doc + 'contracts/openai-compat/v0.1.0/jobs.example.json',
          (ROOT.parent / 'contracts/openai-compat/v0.1.0/jobs.example.json').read_bytes())
    if mac:
        app = '/Applications/Seiya.app/Contents/'
        write(root, app + 'Info.plist', plistlib.dumps({
            'CFBundleIdentifier': 'local.seiya.app', 'CFBundleName': 'Seiya',
            'CFBundleExecutable': 'Seiya', 'CFBundlePackageType': 'APPL',
            'CFBundleShortVersionString': VERSION, 'CFBundleVersion': VERSION}))
        # Terminal shows startup errors and gives the user an explicit Ctrl+C
        # lifecycle. The installer itself never launches services or apps.
        write(root, app + 'MacOS/Seiya', '''#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
exec /usr/bin/open -a Terminal "$HERE/../Resources/Seiya.command"
''', 0o755)
        write(root, app + 'Resources/Seiya.command', '''#!/bin/sh
/usr/local/bin/seiya init
exec /usr/local/bin/seiya serve --open
''', 0o755)
    else:
        write(root, '/usr/share/applications/seiya.desktop', '''[Desktop Entry]
Type=Application
Name=Seiya
Comment=Local sequential model coordinator
Exec=seiya serve --open
Terminal=true
Categories=Development;
''')


def tgz(root):
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode='wb', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            for file in sorted(root.rglob('*')):
                if not file.is_file():
                    continue
                info = archive.gettarinfo(str(file), arcname='./' + file.relative_to(root).as_posix())
                info.uid = info.gid = 0
                info.uname = info.gname = 'root'
                info.mtime = 0
                with file.open('rb') as data:
                    archive.addfile(info, data)
    return buffer.getvalue()


def deb(output):
    with tempfile.TemporaryDirectory(prefix='seiya-build-') as tmp:
        root = Path(tmp) / 'data'
        stage(root)
        control = Path(tmp) / 'control'
        write(control, 'control', f'''Package: seiya
Version: {VERSION}
Section: utils
Priority: optional
Architecture: all
Depends: python3 (>= 3.10)
Recommends: curl
Maintainer: Seiya Local Packaging <seiya@localhost>
Installed-Size: {sum(f.stat().st_size for f in root.rglob('*') if f.is_file()) // 1024 + 1}
Description: Sequential local multi-model coordinator
  Exposes one local OpenAI-compatible model named pegassus by Seiya.
 Requires a separately installed and configured MLX or Ollama runtime.
''')
        deb_archive(output, root, control)


def deb_archive(output, root, control):
    with output.open('wb') as out:
        out.write(b'!<arch>\n')
        for name, content in [('debian-binary', b'2.0\n'),
                              ('control.tar.gz', tgz(control)), ('data.tar.gz', tgz(root))]:
            header = f'{name + "/":<16}{0:<12}{0:<6}{0:<6}{"100644":<8}{len(content):<10}`\n'
            out.write(header.encode('ascii'))
            out.write(content)
            if len(content) % 2:
                out.write(b'\n')


def rust_binary(path, mac, arch):
    """Metadata inspection only; never execute a binary supplied to packaging."""
    if arch not in ('amd64', 'arm64'):
        raise SystemExit('Rust architecture must be amd64 or arm64.')
    if path.is_symlink() or not path.is_file():
        raise SystemExit('Regular Rust release binaries required (no symlinks).')
    meta = path.stat()
    if not 64 <= meta.st_size <= 256 * 1024 * 1024 or not meta.st_mode & 0o111:
        raise SystemExit('Bounded executable Rust release binary required.')
    with path.open('rb') as file:
        header = file.read(64)
    if mac:
        cpu = {'amd64': 0x01000007, 'arm64': 0x0100000C}[arch]
        valid = header[:4] == b'\xcf\xfa\xed\xfe' and int.from_bytes(header[4:8], 'little') == cpu and int.from_bytes(header[12:16], 'little') == 2
    else:
        cpu = {'amd64': 62, 'arm64': 183}[arch]
        valid = header[:7] == b'\x7fELF\x02\x01\x01' and int.from_bytes(header[18:20], 'little') == cpu and int.from_bytes(header[16:18], 'little') in (2, 3)
    if not valid:
        raise SystemExit('Rust binary format/architecture does not match package target.')


def rust_stage(root, binaries, mac, arch, libc_min=None, revision=None):
    if revision is not None and not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise SystemExit('Revision must be an explicit full Git SHA, or omitted.')
    selected = {name: binaries / name for name in ('seiya-desktop', 'seiya-server')}
    for file in selected.values():
        rust_binary(file, mac, arch)
    if mac:
        app = '/Applications/Seiya Rust.app/Contents/'
        lib, doc = app + 'MacOS/', app + 'Resources/'
    else:
        lib, doc = '/usr/lib/seiya-rust/bin/', '/usr/share/doc/seiya-rust-experimental/'
    info = {'version': VERSION, 'implementation': 'rust-experimental', 'platform': 'macos' if mac else 'linux',
            'arch': arch, 'revision': revision, 'libc_min': libc_min, 'binaries': {}}
    for name, source in selected.items():
        target = root / (lib + name).lstrip('/')
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        target.chmod(0o755)
        rust_binary(target, mac, arch)
        # Hash the actual staged payload, not an earlier view of a source file.
        info['binaries'][name] = hashlib.sha256(target.read_bytes()).hexdigest()
    for source, dest in [('docs/PAQUETES_PRUEBA.md', 'README.md'), ('docs/APP_RUST.md', 'APP_RUST.md'), ('docs/PARIDAD_RUST.md', 'PARIDAD_RUST.md'), ('docs/BACKENDS_LOCALES.md', 'BACKENDS_LOCALES.md'),
                         ('docs/IMAGENES_DESDE_CHAT.md', 'IMAGENES_DESDE_CHAT.md'),
                         ('docs/RESIDENCIA_NATIVA.md', 'RESIDENCIA_NATIVA.md'),
                         ('docs/PRUEBA_INSTALADOR.md', 'PRUEBA_INSTALADOR.md'),
                         ('contracts/desktop/v1/config.chat-images.example.json', 'desktop-chat-images.example.json'),
                         ('contracts/local-models/v1/images.example.json', 'local-models-images.example.json'),
                         ('LICENSE', 'LICENSE'), ('THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_NOTICES.md'),
                         ('contracts/desktop/v1/config.example.json', 'desktop.example.json'),
                         ('contracts/local-models/v1/catalog.example.json', 'local-models.example.json'),
                         ('contracts/local-models/v1/README.md', 'LOCAL_MODELS.md')]:
        write(root, doc + dest, (ROOT.parent / source).read_bytes())
    write(root, doc + 'BUILD-INFO.json', json.dumps(info, indent=2) + '\n')
    binary = lib + 'seiya-desktop'
    prefix = '/usr/local/bin/' if mac else '/usr/bin/'
    for command, target in [('seiya-rust', binary), ('seiya-rust-server', lib + 'seiya-server')]:
        write(root, prefix + command, f'#!/bin/sh\nexec "{target}" "$@"\n', 0o755)
    if mac:
        write(root, app + 'Info.plist', plistlib.dumps({
            'CFBundleIdentifier': 'local.seiya.rust.experimental', 'CFBundleName': 'Seiya Rust',
            'CFBundleExecutable': 'Seiya', 'CFBundlePackageType': 'APPL',
            'CFBundleShortVersionString': VERSION, 'CFBundleVersion': VERSION,
            'LSMinimumSystemVersion': '13.0'}))
        write(root, app + 'MacOS/Seiya', '''#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "$HERE/seiya-desktop" --launch
''', 0o755)
        write(root, app + 'Resources/Seiya.command', '''#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "$HERE/../MacOS/seiya-desktop" --launch
''', 0o755)
    else:
        write(root, '/usr/share/applications/seiya-rust.desktop', '''[Desktop Entry]
Type=Application
Name=Seiya Rust (experimental)
Comment=Local web setup; external model engines configured explicitly
Exec=seiya-rust --launch
Terminal=true
Categories=Development;
''')
    verify_rust_stage(root, mac, arch)


def verify_rust_stage(root, mac, arch):
    """Allowlist and hashes for an extracted payload; no installer/binary execution."""
    app = 'Applications/Seiya Rust.app/Contents/'
    lib = app + 'MacOS/' if mac else 'usr/lib/seiya-rust/bin/'
    doc = app + 'Resources/' if mac else 'usr/share/doc/seiya-rust-experimental/'
    prefix = 'usr/local/bin/' if mac else 'usr/bin/'
    expected = {lib + n for n in ('seiya-desktop', 'seiya-server')}
    expected.update(doc + n for n in ('README.md', 'APP_RUST.md', 'PARIDAD_RUST.md', 'BACKENDS_LOCALES.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'desktop.example.json', 'local-models.example.json', 'LOCAL_MODELS.md', 'BUILD-INFO.json'))
    expected.update(doc+n for n in ('IMAGENES_DESDE_CHAT.md','RESIDENCIA_NATIVA.md','PRUEBA_INSTALADOR.md','desktop-chat-images.example.json','local-models-images.example.json'))
    expected.update(prefix + n for n in ('seiya-rust', 'seiya-rust-server'))
    if mac:
        expected.update((app + 'Info.plist', app + 'MacOS/Seiya', app + 'Resources/Seiya.command'))
    else:
        expected.add('usr/share/applications/seiya-rust.desktop')
    actual = set()
    for path in root.rglob('*'):
        if path.is_symlink():
            raise SystemExit('Symlinks are not part of the Rust package payload.')
        if path.is_file():
            actual.add(path.relative_to(root).as_posix())
    if actual != expected:
        raise SystemExit('Rust payload differs from the explicit file allowlist.')
    info = json.loads((root / doc / 'BUILD-INFO.json').read_text())
    if info['implementation'] != 'rust-experimental' or info['arch'] != arch or info['platform'] != ('macos' if mac else 'linux'):
        raise SystemExit('Rust build metadata target mismatch.')
    for name in ('seiya-desktop', 'seiya-server'):
        binary = root / lib / name
        rust_binary(binary, mac, arch)
        if hashlib.sha256(binary.read_bytes()).hexdigest() != info['binaries'][name]:
            raise SystemExit('Rust payload binary hash mismatch.')


def rust_deb(output, binaries, arch, libc_min, revision=None):
    if not libc_min or not re.fullmatch(r'2\.[0-9]+', libc_min):
        raise SystemExit('Explicit build-runner glibc minimum required for GNU/Linux binaries.')
    with tempfile.TemporaryDirectory(prefix='seiya-rust-deb-') as tmp:
        root, control = Path(tmp) / 'root', Path(tmp) / 'control'
        rust_stage(root, binaries, False, arch, libc_min, revision)
        write(control, 'control', f'''Package: seiya-rust-experimental
Version: {VERSION}
Section: utils
Priority: optional
Architecture: {arch}
Depends: libc6 (>= {libc_min}), libgcc-s1
Maintainer: Seiya Local Packaging <seiya@localhost>
Installed-Size: {sum(f.stat().st_size for f in root.rglob('*') if f.is_file()) // 1024 + 1}
Description: Experimental Rust CLI and local API for Seiya
 No Python server or bundled models. macOS native image host is not available
 on Linux. Requires explicitly configured compatible external inference runtime.
''')
        deb_archive(output, root, control)


def rust_pkg(output, binaries, arch, revision=None):
    if not shutil.which('pkgbuild'):
        raise SystemExit('Build macOS packages on macOS with pkgbuild installed.')
    with tempfile.TemporaryDirectory(prefix='seiya-rust-pkg-') as tmp:
        root = Path(tmp) / 'root'
        rust_stage(root, binaries, True, arch, revision=revision)
        subprocess.run(['/usr/bin/xattr', '-cr', str(root)], check=True)
        subprocess.run(['pkgbuild', '--root', str(root), '--identifier', 'local.seiya.rust.experimental',
                        '--version', VERSION, '--install-location', '/', '--ownership', 'recommended',
                        '--filter', r'/\._', str(output)], check=True,
                       env={**os.environ, 'COPYFILE_DISABLE': '1'})


def pkg(output):
    if not shutil.which('pkgbuild'):
        raise SystemExit('Build macOS packages on macOS with pkgbuild installed.')
    with tempfile.TemporaryDirectory(prefix='seiya-pkg-') as tmp:
        root = Path(tmp) / 'root'
        stage(root, mac=True)
        # Python's os.*xattr helpers are not available on macOS. Strip only
        # staging metadata, never source or user files.
        subprocess.run(['/usr/bin/xattr', '-cr', str(root)], check=True)
        subprocess.run(['pkgbuild', '--root', str(root), '--identifier', 'local.seiya.installer',
                        '--version', VERSION, '--install-location', '/', '--ownership', 'recommended',
                        '--filter', r'/\._', str(output)], check=True,
                       env={**os.environ, 'COPYFILE_DISABLE': '1'})


def dmg(output, package, volume=None):
    """Distribution container, not an installer invocation or bundled runtime."""
    if not shutil.which('hdiutil'):
        raise SystemExit('Build disk images on macOS with hdiutil installed.')
    if not package.is_file() or package.is_symlink():
        raise SystemExit('A regular freshly built .pkg is required.')
    with tempfile.TemporaryDirectory(prefix='seiya-dmg-') as tmp:
        root = Path(tmp) / 'Seiya'
        root.mkdir()
        shutil.copyfile(package, root / package.name)
        shutil.copyfile(ROOT.parent / 'docs/PAQUETES_PRUEBA.md', root / 'README.md')
        manifest = ''.join(f'{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n'
                           for file in sorted(root.iterdir()))
        (root / 'SHA256SUMS').write_text(manifest)
        subprocess.run(['hdiutil', 'create', '-volname', volume or f'Seiya-{VERSION}',
                        '-srcfolder', str(root), '-format', 'UDZO', '-ov', str(output)],
                       check=True)


def rust_dmg(output, binaries, arch, revision=None):
    """Finder-style app distribution. PKG/CLI wrappers are separate outputs."""
    if not shutil.which('hdiutil'):
        raise SystemExit('Build disk images on macOS with hdiutil installed.')
    with tempfile.TemporaryDirectory(prefix='seiya-app-dmg-') as tmp:
        staged = Path(tmp) / 'payload'
        rust_stage(staged, binaries, True, arch, revision=revision)
        root = Path(tmp) / 'Seiya'
        root.mkdir()
        shutil.copytree(staged / 'Applications/Seiya Rust.app', root / 'Seiya Rust.app')
        (root / 'Applications').symlink_to('/Applications', target_is_directory=True)
        (root / 'README.md').write_text(
            'Seiya Rust · macOS\n\n'
            'Drag Seiya Rust.app to Applications. Do not drag the DMG file.\n'
            'Arrastra Seiya Rust.app a Aplicaciones; no el archivo DMG.\n\n'
            'Open the app from Applications, then review its web setup.\n'
            'No models or engines are included or downloaded automatically.\n'
            'This app is unsigned/not notarized. Do not disable system protections.\n'
            'The optional separate PKG also installs command-line wrappers.\n'
            'See the app Resources/PRUEBA_INSTALADOR.md for limits and feedback.\n', encoding='utf-8')
        manifest = ''.join(
            f'{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.relative_to(root).as_posix()}\n'
            for file in sorted(root.rglob('*')) if file.is_file() and not file.is_symlink())
        (root / 'SHA256SUMS').write_text(manifest, encoding='utf-8')
        subprocess.run(['/usr/bin/xattr', '-cr', str(root / 'Seiya Rust.app')], check=True)
        subprocess.run(['hdiutil', 'create', '-volname', f'Seiya-Rust-{VERSION}-{arch}',
                        '-srcfolder', str(root), '-format', 'UDZO', '-ov', str(output)], check=True,
                       env={**os.environ, 'COPYFILE_DISABLE': '1'})


def main():
    p = argparse.ArgumentParser()
    p.add_argument('target', choices=['deb', 'mac', 'dmg', 'all'])
    p.add_argument('--output', type=Path, default=ROOT / 'dist')
    p.add_argument('--implementation', choices=['rust', 'python'], default='rust')
    p.add_argument('--bin-dir', type=Path)
    p.add_argument('--arch', choices=['amd64', 'arm64'])
    p.add_argument('--libc-min', help='GNU/Linux build runner glibc version, e.g. 2.39')
    p.add_argument('--revision', help='Explicit source snapshot full Git SHA (optional)')
    args = p.parse_args()
    if args.implementation == 'rust' and (not args.bin_dir or not args.arch or args.target == 'all'):
        p.error('Rust requires --bin-dir, --arch and a specific target: deb, mac or dmg (not all).')
    args.output.mkdir(parents=True, exist_ok=True)
    built = []
    if args.implementation == 'rust':
        if args.target == 'deb':
            output = args.output.resolve() / f'seiya-rust-experimental_{VERSION}_{args.arch}.deb'
            rust_deb(output, args.bin_dir, args.arch, args.libc_min, args.revision)
        else:
            output = args.output.resolve() / f'Seiya-Rust-{VERSION}-{args.arch}.pkg'
            rust_pkg(output, args.bin_dir, args.arch, args.revision)
        built.append(output)
        if args.target == 'dmg':
            image = args.output.resolve() / f'Seiya-Rust-{VERSION}-{args.arch}.dmg'
            rust_dmg(image, args.bin_dir, args.arch, args.revision)
            built.append(image)
    else:
        built = legacy_packages(args)
    for file in built:
        print(file)
    (args.output / 'SHA256SUMS').write_text(''.join(
        f'{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n' for file in built))


def legacy_packages(args):
    built = []
    if args.target in ('deb', 'all'):
        output = args.output.resolve() / f'seiya_{VERSION}_all.deb'
        deb(output)
        built.append(output)
    if args.target in ('mac', 'dmg', 'all'):
        output = args.output.resolve() / f'Seiya-{VERSION}.pkg'
        pkg(output)
        built.append(output)
        if args.target in ('dmg', 'all'):
            image = args.output.resolve() / f'Seiya-{VERSION}.dmg'
            dmg(image, output)
            built.append(image)
    return built


if __name__ == '__main__':
    main()
