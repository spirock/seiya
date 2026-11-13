import shutil
import subprocess
import unittest
from pathlib import Path


class UITests(unittest.TestCase):
    @unittest.skipUnless(shutil.which('node'), 'Node is required for UI regression tests')
    def test_background_jobs_and_capabilities(self):
        root = Path(__file__).resolve().parents[1]
        subprocess.run(['node', str(root / 'tests/jobs_ui.cjs')], cwd=root,
                       check=True, timeout=15)

    @unittest.skipUnless(shutil.which('node'), 'Node is required for UI regression tests')
    def test_history_and_request_lifecycle(self):
        root = Path(__file__).resolve().parents[1]
        subprocess.run(['node', str(root / 'tests/ui_history.cjs')], cwd=root,
                       check=True, timeout=15)
