import importlib.util
import io
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
from pathlib import Path


class PackagingTests(unittest.TestCase):
    def test_dmg_contains_only_package_note_and_manifest_without_installing(self):
        spec = importlib.util.spec_from_file_location('build', Path(__file__).resolve().parents[1] / 'packaging/build.py')
        build = importlib.util.module_from_spec(spec); spec.loader.exec_module(build)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / 'Seiya-0.1.0.pkg'; package.write_bytes(b'package fixture, not executable')
            image = root / 'Seiya-0.1.0.dmg'
            def create(command, **kwargs):
                self.assertEqual(command[:2], ['hdiutil', 'create'])
                self.assertIn('UDZO', command)
                staged = Path(command[command.index('-srcfolder') + 1])
                self.assertEqual({p.name for p in staged.iterdir()}, {package.name, 'README.md', 'SHA256SUMS'})
                self.assertEqual((staged / package.name).read_bytes(), package.read_bytes())
                self.assertIn('Python', (staged / 'README.md').read_text())
                self.assertIn(package.name, (staged / 'SHA256SUMS').read_text())
                image.write_bytes(b'disk-image fixture')
            with mock.patch.object(build.shutil, 'which', return_value='/usr/bin/hdiutil'), mock.patch.object(build.subprocess, 'run', side_effect=create) as run:
                build.dmg(image, package)
                self.assertEqual(run.call_count, 1)
            self.assertTrue(image.is_file())

    def test_deb_archive_and_extracted_cli(self):
        spec = importlib.util.spec_from_file_location('build', Path(__file__).resolve().parents[1] / 'packaging/build.py')
        build = importlib.util.module_from_spec(spec); spec.loader.exec_module(build)
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            target = tmp / 'seiya.deb'
            build.deb(target)
            raw = target.read_bytes()
            self.assertEqual(raw[:8], b'!<arch>\n')
            members = {}; offset = 8
            while offset < len(raw):
                header = raw[offset:offset+60]
                self.assertEqual(header[-2:], b'`\n')
                name = header[:16].decode().strip().rstrip('/')
                size = int(header[48:58])
                members[name] = raw[offset+60:offset+60+size]
                offset += 60 + size + size % 2
            self.assertEqual(set(members), {'debian-binary', 'control.tar.gz', 'data.tar.gz'})
            self.assertEqual(members['debian-binary'], b'2.0\n')
            with tarfile.open(fileobj=io.BytesIO(members['control.tar.gz']), mode='r:gz') as archive:
                control = archive.extractfile('./control').read().decode()
                self.assertIn('Package: seiya\n', control)
                self.assertIn('Depends: python3 (>= 3.10)', control)
                self.assertEqual(archive.getnames(), ['./control'])
            with tarfile.open(fileobj=io.BytesIO(members['data.tar.gz']), mode='r:gz') as archive:
                self.assertEqual(archive.getmember('./usr/bin/seiya').mode, 0o755)
                for item in archive:
                    self.assertTrue(item.isfile())
                    path = tmp / item.name
                    self.assertTrue(path.resolve().is_relative_to(tmp.resolve()))
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(archive.extractfile(item).read())
            package = tmp / 'usr/lib/seiya'
            self.assertTrue((package / 'seiya/runtime_bridge.py').is_file())
            self.assertTrue((package / 'seiya/jobs_profile.py').is_file())
            docs = tmp / 'usr/share/doc/seiya'
            self.assertTrue((docs / 'docs/CONTRATO_SEIYA.md').is_file())
            self.assertTrue((docs / 'docs/MEMORIA_ADAPTATIVA.md').is_file())
            self.assertTrue((docs / 'contracts/v1/session-request.schema.json').is_file())
            self.assertTrue((docs / 'docs/TRABAJOS_EN_SEGUNDO_PLANO.md').is_file())
            self.assertTrue((docs / 'contracts/openai-compat/v0.1.0/jobs.example.json').is_file())
            self.assertIn('](docs/TRABAJOS_EN_SEGUNDO_PLANO.md)', (docs / 'README.md').read_text())
            self.assertFalse(any(p.suffix in ('.safetensors', '.gguf') for p in tmp.rglob('*')))
            self.assertIn('pegassus', control)
            run = subprocess.run([sys.executable, '-m', 'seiya', '--version'], cwd=package,
                                 capture_output=True, text=True, check=True)
            self.assertEqual(run.stdout.strip(), '0.1.0')


if __name__ == '__main__': unittest.main()
