"""Archive/staging tests with synthetic binary headers, never executed as code."""
import hashlib
import importlib.util
import io
import json
import plistlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / 'packaging/build.py'
spec = importlib.util.spec_from_file_location('rust_packaging', SCRIPT)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


def binaries(root, mac=False, arch='amd64'):
    root.mkdir()
    for name in ('seiya-desktop', 'seiya-server'):
        header = bytearray(64)
        if mac:
            header[:4] = b'\xcf\xfa\xed\xfe'
            header[4:8] = {'amd64': 0x01000007, 'arm64': 0x0100000C}[arch].to_bytes(4, 'little')
            header[12:16] = (2).to_bytes(4, 'little')
        else:
            header[:7] = b'\x7fELF\x02\x01\x01'
            header[16:18] = (3).to_bytes(2, 'little')
            header[18:20] = {'amd64': 62, 'arm64': 183}[arch].to_bytes(2, 'little')
        (root / name).write_bytes(header + b'synthetic archive fixture, not executable code')
        (root / name).chmod(0o755)
    (root / 'not-selected.gguf').write_bytes(b'not a real model')
    return root


def members(deb):
    raw = deb.read_bytes()
    assert raw[:8] == b'!<arch>\n'
    offset, result = 8, {}
    while offset < len(raw):
        header = raw[offset:offset + 60]
        size = int(header[48:58])
        result[header[:16].decode().strip().rstrip('/')] = raw[offset + 60:offset + 60 + size]
        offset += 60 + size + size % 2
    return result


class RustPackagingTests(unittest.TestCase):
    def test_debian_arch_payload_hashes_no_python_or_install_hooks(self):
        for arch in ('amd64', 'arm64'):
            with self.subTest(arch=arch), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                source = binaries(root / 'bin', arch=arch)
                output = root / 'test.deb'
                build.rust_deb(output, source, arch, '2.39')
                parts = members(output)
                with tarfile.open(fileobj=io.BytesIO(parts['control.tar.gz'])) as archive:
                    self.assertEqual(archive.getnames(), ['./control'])
                    control = archive.extractfile('./control').read().decode()
                    self.assertIn('Package: seiya-rust-experimental\n', control)
                    self.assertIn(f'Architecture: {arch}\n', control)
                    self.assertIn('Depends: libc6 (>= 2.39), libgcc-s1\n', control)
                    self.assertNotIn('Depends: python', control)
                with tarfile.open(fileobj=io.BytesIO(parts['data.tar.gz'])) as archive:
                    names = archive.getnames()
                    self.assertNotIn('./usr/bin/seiya', names)
                    self.assertFalse(any(n.endswith(('.py', '.gguf', '.pem', '.der')) for n in names))
                    info = json.load(archive.extractfile('./usr/share/doc/seiya-rust-experimental/BUILD-INFO.json'))
                    for name in ('seiya-desktop', 'seiya-server'):
                        member = archive.getmember('./usr/lib/seiya-rust/bin/' + name)
                        self.assertEqual(member.mode, 0o755)
                        payload = archive.extractfile(member).read()
                        self.assertEqual(payload, (source / name).read_bytes())
                        self.assertEqual(hashlib.sha256(payload).hexdigest(), info['binaries'][name])
                    wrapper = archive.extractfile('./usr/bin/seiya-rust').read().decode()
                    self.assertIn('exec "/usr/lib/seiya-rust/bin/seiya-desktop" "$@"', wrapper)
                    self.assertEqual(info['arch'], arch)

    def test_mac_bundle_keeps_legacy_separate_and_uses_explicit_native_launcher(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin', mac=True, arch='arm64')
            stage = root / 'stage'
            build.rust_stage(stage, source, True, 'arm64')
            app = stage / 'Applications/Seiya Rust.app/Contents'
            plist = plistlib.loads((app / 'Info.plist').read_bytes())
            self.assertEqual(plist['CFBundleIdentifier'], 'local.seiya.rust.experimental')
            self.assertFalse((stage / 'Applications/Seiya.app').exists())
            self.assertFalse((stage / 'usr/local/bin/seiya').exists())
            launcher = (app / 'Resources/Seiya.command').read_text()
            self.assertIn('"$HERE/../MacOS/seiya-desktop" --launch', launcher)
            self.assertIn('"$HERE/seiya-desktop" --launch', (app / 'MacOS/Seiya').read_text())
            self.assertNotIn('--serve', launcher)
            self.assertNotIn('python', launcher.lower())
            self.assertFalse(any(p.suffix in ('.py', '.gguf', '.der') for p in stage.rglob('*')))
            self.assertEqual((app / 'MacOS/seiya-desktop').read_bytes(), (source / 'seiya-desktop').read_bytes())
            self.assertTrue((app / 'Resources/desktop.example.json').is_file())

    def test_wrong_os_arch_symlinks_and_nonexecutables_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin')
            file = source / 'seiya-desktop'
            for mac, arch in [(True, 'amd64'), (False, 'arm64'), (False, 'all')]:
                with self.assertRaises(SystemExit): build.rust_binary(file, mac, arch)
            file.chmod(0o644)
            with self.assertRaises(SystemExit): build.rust_binary(file, False, 'amd64')
            file.chmod(0o755)
            link = root / 'link'; link.symlink_to(file)
            with self.assertRaises(SystemExit): build.rust_binary(link, False, 'amd64')
            file.write_bytes(b'#!/bin/sh\nexit 99\n')
            with self.assertRaises(SystemExit): build.rust_binary(file, False, 'amd64')

    def test_pkg_stages_only_and_never_invokes_installer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin', mac=True)
            calls = []
            def run(command, **kwargs):
                calls.append(command)
                self.assertNotIn('installer', command)
                if command[0] == 'pkgbuild':
                    staged = Path(command[command.index('--root') + 1])
                    self.assertTrue((staged / 'Applications/Seiya Rust.app/Contents/MacOS/seiya-desktop').is_file())
                    self.assertEqual(command[command.index('--identifier') + 1], 'local.seiya.rust.experimental')
                    self.assertNotIn('--scripts', command)
            with mock.patch.object(build.shutil, 'which', return_value='/usr/bin/pkgbuild'), mock.patch.object(build.subprocess, 'run', side_effect=run):
                build.rust_pkg(root / 'test.pkg', source, 'amd64')
            self.assertEqual([c[0] for c in calls], ['/usr/bin/xattr', 'pkgbuild'])

    def test_cli_requires_explicit_inputs_and_only_hashes_new_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin')
            out = root / 'out'; out.mkdir()
            (out / 'old.pkg').write_bytes(b'untouched')
            command = [sys.executable, str(SCRIPT), 'deb', '--output', str(out)]
            result = subprocess.run(command, capture_output=True)
            self.assertEqual(result.returncode, 2)
            result = subprocess.run(command + ['--bin-dir', str(source), '--arch', 'amd64', '--libc-min', '2.39'], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            sums = (out / 'SHA256SUMS').read_text()
            self.assertIn('seiya-rust-experimental_0.1.0_amd64.deb', sums)
            self.assertNotIn('old.pkg', sums)
            self.assertEqual((out / 'old.pkg').read_bytes(), b'untouched')

    def test_abi_and_revision_are_explicit_not_guessed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin')
            for minimum in (None, '0', '2.39\nDepends: injected'):
                with self.assertRaises(SystemExit): build.rust_deb(root / 'test.deb', source, 'amd64', minimum)
            with self.assertRaises(SystemExit): build.rust_stage(root / 'stage', source, False, 'amd64', revision='HEAD')

    def test_inspector_detects_unexpected_files_and_binary_tampering(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = binaries(root / 'bin')
            stage = root / 'stage'
            build.rust_stage(stage, source, False, 'amd64', '2.39')
            build.verify_rust_stage(stage, False, 'amd64')
            unexpected = stage / 'extra.py'; unexpected.write_text('not executed')
            with self.assertRaises(SystemExit): build.verify_rust_stage(stage, False, 'amd64')
            unexpected.unlink()
            with (stage / 'usr/lib/seiya-rust/bin/seiya-desktop').open('ab') as file:
                file.write(b'tampered')
            with self.assertRaises(SystemExit): build.verify_rust_stage(stage, False, 'amd64')
