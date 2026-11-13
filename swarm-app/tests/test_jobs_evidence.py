"""Recorded synthetic preview is not real-model or browser-visual evidence."""
import json
from pathlib import Path
import unittest


class JobsEvidenceTests(unittest.TestCase):
    def test_preview_does_not_promote_simulation_to_inference(self):
        path = Path(__file__).resolve().parents[2] / 'contracts/benchmarks/background-jobs/v0.1.0/preview-evidence.json'
        record = json.loads(path.read_text())
        self.assertEqual(record['mode'], 'owned-synthetic-preview-http')
        self.assertTrue(record['orchestrator_and_auxiliary_simulated'])
        self.assertTrue(record['real_local_http'])
        for field in ('model_inference', 'workspace_written', 'tests_executed_by_job',
                      'real_model_pair_validated', 'safari_visual_validated'):
            self.assertIs(record[field], False)
        for field in ('idempotent_submit_verified', 'host_formatted_status_verified',
                      'cancelled_state_verified', 'artifact_retrieved', 'frontend_auth_and_host_verified',
                      'v1_bypass_rejected'):
            self.assertIs(record[field], True)
