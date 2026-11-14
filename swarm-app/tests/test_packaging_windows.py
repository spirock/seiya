"""Synthetic PE fixtures only. Never execute a fixture or install Windows tools."""
import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

PACKAGING = Path(__file__).resolve().parents[1] / 'packaging'
spec = importlib.util.spec_from_file_location('build', PACKAGING / 'build.py')
builder = importlib.util.module_from_spec(spec); spec.loader.exec_module(builder)
spec = importlib.util.spec_from_file_location('windows_packaging', PACKAGING / 'windows.py')
windows = importlib.util.module_from_spec(spec)
with mock.patch.dict(sys.modules, {'build': builder}):
    spec.loader.exec_module(windows)


def pe(root):
    root.mkdir()
    for name in ('seiya-desktop.exe', 'seiya-server.exe'):
        data = bytearray(512)
        data[:2] = b'MZ'; data[60:64] = (64).to_bytes(4, 'little')
        data[64:68] = b'PE\0\0'; data[68:70] = (0x8664).to_bytes(2, 'little')
        data[84:86] = (112).to_bytes(2, 'little'); data[86:88] = (2).to_bytes(2, 'little')
        data[88:90] = (0x20B).to_bytes(2, 'little'); data[156:158] = (3).to_bytes(2, 'little')
        (root / name).write_bytes(data)
    (root / 'never-selected.gguf').write_bytes(b'not a model')
    return root


class WindowsPackagingTests(unittest.TestCase):
    def test_pe_x64_rejects_other_platform_arch_dll_truncation_and_unbounded_offsets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = pe(root / 'bin'); file = source / 'seiya-desktop.exe'
            windows.windows_binary(file)
            original = file.read_bytes()
            for start, replacement in [(0, b'#!'), (60, (2**31).to_bytes(4, 'little')),
                                       (68, (0x14C).to_bytes(2, 'little')), (68, (0xAA64).to_bytes(2, 'little')),
                                       (86, (0x2002).to_bytes(2, 'little')), (88, (0x10B).to_bytes(2, 'little'))]:
                data = bytearray(original); data[start:start + len(replacement)] = replacement
                file.write_bytes(data)
                with self.assertRaises(SystemExit): windows.windows_binary(file)
            file.write_bytes(original[:128])
            with self.assertRaises(SystemExit): windows.windows_binary(file)
            file.write_bytes(original)
            with mock.patch.object(Path, 'is_symlink', return_value=True):
                with self.assertRaises(SystemExit): windows.windows_binary(file)

    def test_zip_allowlist_hashes_and_discovery_no_credentials_or_python(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = pe(root / 'bin'); payload = root / 'payload'
            windows.stage_payload(payload, source, 'a' * 40)
            archive = root / 'portable.zip'; windows.portable_zip(payload, archive)
            with zipfile.ZipFile(archive) as zipped:
                self.assertEqual(set(zipped.namelist()), windows.FILES)
                self.assertFalse(any(n.endswith(('.py', '.gguf', '.der')) for n in zipped.namelist()))
                info = json.loads(zipped.read('BUILD-INFO.json'))
                for name in ('seiya-desktop.exe', 'seiya-server.exe'):
                    self.assertEqual(zipped.read(name), (source / name).read_bytes())
                    self.assertEqual(hashlib.sha256(zipped.read(name)).hexdigest(), info['binaries'][name])
                config = json.loads(zipped.read('server.discovery.example.json'))
                self.assertIsNone(config['runtime']); self.assertIsNone(config['api_key_env'])
                help_cmd = zipped.read('Seiya-Rust-Help.cmd').decode()
                self.assertIn('"%~dp0seiya-desktop.exe" --help', help_cmd)
                self.assertNotIn('--serve', help_cmd)
                launch_cmd = zipped.read('Seiya-Rust.cmd').decode()
                catalogue = json.loads(zipped.read('local-models.example.json'))
                self.assertEqual(catalogue['models'][0]['runtime']['backend'], 'openai_local')
                self.assertIn('Catálogo', zipped.read('LOCAL_MODELS.md').decode())
                self.assertIn('"%~dp0seiya-desktop.exe" --launch', launch_cmd)
                self.assertNotIn('--serve', launch_cmd)
                extracted = root / 'extracted'; zipped.extractall(extracted)
            windows.inspect_payload(extracted)
            (extracted / 'seiya-server.exe').write_bytes((source / 'seiya-server.exe').read_bytes() + b'tampered')
            with self.assertRaises(SystemExit): windows.inspect_payload(extracted)

    def test_installer_compiles_inspected_zip_payload_without_installing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = pe(root / 'bin'); payload = root / 'payload'
            windows.stage_payload(payload, source)
            compiler = root / 'ISCC.exe'; compiler.write_bytes(b'not executed fixture')
            output = root / 'out'; output.mkdir()
            def compile_only(command, **kwargs):
                self.assertEqual(command[0], str(compiler))
                self.assertIn(f'/DStageDir={payload.resolve()}', command)
                self.assertEqual(command[-1], str(PACKAGING / 'windows.iss'))
                (output / f'Seiya-Rust-{windows.VERSION}-windows-x64-setup.exe').write_bytes(b'MZ' + b'fixture' * 20)
            with mock.patch.object(windows.subprocess, 'run', side_effect=compile_only) as run:
                windows.setup(payload, output, compiler)
                self.assertEqual(run.call_count, 1)
            text = (PACKAGING / 'windows.iss').read_text()
            self.assertIn('PrivilegesRequired=lowest', text)
            self.assertIn('CloseApplications=no', text)
            self.assertIn('Parameters: "--launch"', text)
            for forbidden in ('[Run]', '[Code]', '[Registry]', '[UninstallDelete]', 'recursesubdirs', '*'):
                self.assertNotIn(forbidden, text)
            sources = {line.split('\\', 1)[1].split('"', 1)[0] for line in text.splitlines() if line.startswith('Source:')}
            self.assertEqual(sources, windows.FILES)

    def test_missing_compiler_and_extra_payload_fail_without_fallback(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = pe(root / 'bin'); payload = root / 'payload'
            windows.stage_payload(payload, source)
            with mock.patch.object(windows.subprocess, 'run') as run:
                with self.assertRaises(SystemExit): windows.setup(payload, root, root / 'missing.exe')
                self.assertFalse(run.called)
            (payload / 'extra.py').write_text('not executed')
            with self.assertRaises(SystemExit): windows.inspect_payload(payload)

    def test_cli_build_and_inspect_do_not_execute_inputs_or_hash_stale_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); source = pe(root / 'bin'); output = root / 'out'; output.mkdir()
            (output / 'stale.exe').write_bytes(b'preserved')
            result = subprocess.run([sys.executable, str(PACKAGING / 'windows.py'), 'zip', '--bin-dir', str(source), '--output', str(output)], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            sums = (output / 'SHA256SUMS').read_text()
            self.assertNotIn('stale.exe', sums)
            with zipfile.ZipFile(output / f'Seiya-Rust-{windows.VERSION}-windows-x64.zip') as archive:
                archive.extractall(root / 'extracted')
            result = subprocess.run([sys.executable, str(PACKAGING / 'windows.py'), 'inspect', '--payload-dir', str(root / 'extracted')], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
