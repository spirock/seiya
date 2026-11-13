"""Static CI contract; verified upstream action runtimes are documented in plan."""
import re
import unittest
from pathlib import Path


class WorkflowRuntimeTests(unittest.TestCase):
    def test_selected_actions_and_ui_runtime_use_node24_without_force_override(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/seiya-packages.yml').read_text()
        selected = {'checkout': 'v7.0.1', 'setup-node': 'v7.1.0',
                    'setup-python': 'v7.1.0', 'upload-artifact': 'v7.0.2'}
        for action, version in selected.items():
            refs = re.findall(r'uses:\s*actions/' + re.escape(action) + r'@([^\s]+)', workflow)
            self.assertTrue(refs, action)
            self.assertEqual(set(refs), {version})
        versions = re.findall(r"node-version:\s*['\"]?([0-9]+)", workflow)
        self.assertTrue(versions)
        self.assertEqual(set(versions), {'24'})
        self.assertNotIn('FORCE_JAVASCRIPT_ACTIONS_TO_NODE', workflow)
