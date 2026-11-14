"""Readme publishing fixtures: no network, push, artifact download or image generation."""
import copy
import importlib.util
import re
import json
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('readme_downloads', ROOT / '.github/scripts/update_downloads.py')
downloads = importlib.util.module_from_spec(spec); spec.loader.exec_module(downloads)


def metadata():
    return {'artifacts': [{'id': n + 1, 'name': name, 'expired': False,
                          'workflow_run': {'id': 123, 'head_sha': 'a' * 40}}
                         for n, name in enumerate(downloads.REQUIRED)]}


class ReadmeDownloadsTests(unittest.TestCase):
    def test_opencode_row_distinguishes_partial_api_from_launcher_and_real_platform_tests(self):
        readme = (ROOT / 'readmees.md').read_text(encoding='utf-8')
        self.assertIn('| Integración con OpenCode mediante API compatible parcial |', readme)
        self.assertIn('🧪 ensayos aislados anteriores | 🧪 contratos/mocks | 🧪 contratos/mocks |', readme)
        self.assertIn('No es un plugin incluido ni una configuración automática de OpenCode', readme)
    def test_update_changes_only_marked_block_and_is_idempotent(self):
        before = 'Keep banner\n' + downloads.START + '\nold\n' + downloads.END + '\nKeep docs\n'
        result = downloads.updated(before, metadata(), 'spirock/seiya', 123, 'a' * 40)
        self.assertTrue(result.startswith('Keep banner\n'))
        self.assertTrue(result.endswith('\nKeep docs\n'))
        self.assertIn('/actions/runs/123/artifacts/1', result)
        self.assertIn('Instalador .exe', result)
        self.assertEqual(downloads.updated(result, metadata(), 'spirock/seiya', 123, 'a' * 40), result)

    def test_missing_expired_duplicate_or_wrong_source_never_produces_links(self):
        body = downloads.START + downloads.END
        fixtures = []
        data = metadata(); data['artifacts'].pop(); fixtures.append(data)
        data = metadata(); data['artifacts'][0]['expired'] = True; fixtures.append(data)
        data = metadata(); data['artifacts'].append(copy.deepcopy(data['artifacts'][0])); fixtures.append(data)
        data = metadata(); data['artifacts'][0]['workflow_run']['head_sha'] = 'b' * 40; fixtures.append(data)
        data = metadata(); data['artifacts'][0]['workflow_run']['id'] = 999; fixtures.append(data)
        for data in fixtures:
            with self.assertRaises(ValueError): downloads.updated(body, data, 'spirock/seiya', 123, 'a' * 40)

    def test_markers_and_identifiers_cannot_inject_or_replace_other_content(self):
        for body in ['', downloads.START, downloads.START + downloads.END + downloads.END]:
            with self.assertRaises(ValueError): downloads.updated(body, metadata(), 'spirock/seiya', 123, 'a' * 40)
        with self.assertRaises(ValueError): downloads.updated(downloads.START + downloads.END, metadata(), 'x/y\ninjected', 123, 'a' * 40)

    def test_readme_graphics_are_local_original_svg_without_scripts_or_external_references(self):
        readme = (ROOT / 'README.md').read_text(encoding='utf-8')
        for name in ('seiya-banner.svg', 'seiya-arquitectura.svg'):
            self.assertIn('docs/assets/' + name, readme)
            tree = ET.parse(ROOT / 'docs/assets' / name)
            for node in tree.iter():
                self.assertNotIn(node.tag.rsplit('}', 1)[-1], ('script', 'foreignObject', 'image'))
                for key in node.attrib:
                    self.assertFalse(key.lower().startswith('on'))
                    self.assertFalse(key.rsplit('}', 1)[-1] in ('href', 'src'))

    def test_ci_only_writes_readme_after_success_on_current_main(self):
        workflow = (ROOT / '.github/workflows/seiya-packages.yml').read_text()
        self.assertIn('needs: [contracts, build, windows]', workflow)
        self.assertIn("github.event_name != 'pull_request'", workflow)
        self.assertIn('"$(git rev-parse HEAD)" != "$GITHUB_SHA"', workflow)
        self.assertIn('git add -- README.md', workflow)
        self.assertNotIn('git push --force', workflow)
        self.assertNotIn('pull_request_target:', workflow)

    def test_model_roles_match_native_tags_and_are_outside_generated_download_block(self):
        readme = (ROOT / 'readmees.md').read_text(encoding='utf-8')
        code = (ROOT / 'crates/seiya-server/src/prepared_web.rs').read_text()
        for tag in ('gemma4:e2b-mlx', 'qwen3.5:9b-mlx'):
            self.assertIn(tag, code)
            self.assertIn(tag, readme)
        result = downloads.updated(readme, metadata(), 'spirock/seiya', 123, 'a' * 40)
        for label in ('## Funciones por sistema operativo', '## Modelos y asignaciones', 'Qwen-Image-2.1-Turbo', 'Qwen3-VL-8B-Instruct', 'VAE Qwen-Image-2.1'):
            self.assertIn(label, result)
        english=(ROOT/'README.md').read_text(encoding='utf-8')
        self.assertIn('[Español](readmees.md)',english)
        self.assertIn('[English](README.md)',readme)
        for label in ('gemma4:e2b-mlx','qwen3.5:9b-mlx','## Platform status','## Models and compatibility'):
            self.assertIn(label,downloads.updated(english,metadata(),'spirock/seiya',123,'a'*40,'en'))

    def test_both_languages_use_same_artifacts_and_validate_before_writing(self):
        english=downloads.updated('English\n'+downloads.START+downloads.END,metadata(),'spirock/seiya',123,'a'*40,'en')
        spanish=downloads.updated('Español\n'+downloads.START+downloads.END,metadata(),'spirock/seiya',123,'a'*40,'es')
        self.assertEqual(re.findall(r'https://[^)]+',english),re.findall(r'https://[^)]+',spanish))
        self.assertIn('Optional PKG',english);self.assertIn('PKG opcional',spanish)
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);en=root/'README.md';es=root/'readmees.md';meta=root/'artifacts.json'
            en.write_text(english);es.write_text('invalid markers');meta.write_text(json.dumps(metadata()))
            result=subprocess.run([sys.executable,str(ROOT/'.github/scripts/update_downloads.py'),'--readme',str(en),'--language','en','--readme-es',str(es),'--artifacts',str(meta),'--repository','spirock/seiya','--run-id','123','--sha','a'*40],capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0);self.assertEqual(en.read_text(),english);self.assertEqual(es.read_text(),'invalid markers')

    def test_public_roadmap_has_no_conversation_attribution_or_private_plan_links(self):
        roadmap = (ROOT / 'ROADMAP.md').read_text(encoding='utf-8')
        for phrase in ('el usuario solicit', 'usuario autoriza', 'petición del usuario'):
            self.assertNotIn(phrase, roadmap.lower())
        documents = [ROOT / 'README.md', ROOT/'readmees.md', ROOT / 'ROADMAP.md', *list((ROOT / 'docs').rglob('*.md'))]
        for path in documents:
            text = path.read_text(encoding='utf-8')
            self.assertIsNone(re.search(r'\]\([^)]*(?:\.\./|\b)plans/', text), str(path))
