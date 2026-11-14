"""Frontend is retained verbatim; backend work must not replace or edit its assets."""
import hashlib
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


class FrozenFrontendTests(unittest.TestCase):
    def test_frontend_assets_keep_their_approved_bytes(self):
        manifest = json.loads((ROOT / 'contracts/frontend/frozen/v1.json').read_text())
        self.assertEqual(manifest['version'], 1)
        for name, digest in manifest['assets'].items():
            with self.subTest(asset=name):
                self.assertEqual(hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), digest)
