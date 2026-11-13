"""Synthetic adapter tests only: never launch Ollama, sandbox or native probes."""
import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import seiya_ollama_private as probe


class OllamaPrivateTests(unittest.TestCase):
    def test_fixed_one_response_budgets_no_tools_and_no_remote_tag(self):
        for tag in probe.TAGS:
            body = probe.request_body(tag)
            self.assertIs(body['think'], False)
            self.assertIs(body['stream'], False)
            self.assertEqual(body['keep_alive'], 0)
            self.assertEqual(body['options']['num_ctx'], 4096)
            self.assertEqual(body['options']['num_predict'], 256)
            self.assertNotIn('tools', body)
        with self.assertRaises(ValueError):
            probe.request_body('remote:cloud')

    def test_profile_denies_original_writes_and_escapes_operator_paths(self):
        profile = probe.profile(Path('/synthetic/a"b'))
        self.assertIn('(deny file-write* (subpath ', profile)
        self.assertIn('a\\"b', profile)
        self.assertNotIn('file-write* (literal', profile)

    def test_staging_is_allowlisted_and_never_copies_or_modifies_weights(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, store = root / 'source', root / 'store'
            (source / 'blobs').mkdir(parents=True)
            data = b'synthetic weights'
            digest = hashlib.sha256(data).hexdigest()
            blob = source / 'blobs' / ('sha256-' + digest)
            blob.write_bytes(data)
            raw = json.dumps({'config': {'digest': 'sha256:' + digest, 'size': len(data)},
                              'layers': []}).encode()
            sha = hashlib.sha256(raw).hexdigest()
            (source / 'blobs' / ('sha256-' + sha)).write_bytes(raw)
            manifest = source / 'manifests-v2/ollama.com/library/gemma4/e2b-mlx'
            manifest.parent.mkdir(parents=True)
            manifest.write_bytes(raw)
            (source / 'unrelated').write_text('do not read or stage')
            probe.stage(store, source, probe.TAGS[0], sha)
            staged = store / 'blobs' / blob.name
            self.assertTrue(staged.is_symlink())
            self.assertEqual(staged.resolve(), blob.resolve())
            self.assertEqual(blob.read_bytes(), data)
            self.assertFalse((store / 'unrelated').exists())
            with self.assertRaisesRegex(RuntimeError, 'identity mismatch'):
                probe.stage(root / 'other', source, probe.TAGS[0], '0' * 64)

    def tree(self, infos):
        tree = probe.OwnedTree.__new__(probe.OwnedTree)
        tree.child = SimpleNamespace(pid=10)
        tree.root = (10, 1, 1, 9, 10, 2)
        tree.known = {10: tree.root}
        tree.native = SimpleNamespace(info=lambda pid: infos.get(pid))
        return tree

    def test_owned_scan_rejects_foreign_group_member_and_recycled_pid(self):
        tree = self.tree({10: (10, 1, 1, 9, 10, 2), 11: (11, 2, 1, 99, 10, 2)})
        with patch.object(probe, 'fixed', return_value='10 9 10\n11 99 10\n'), patch.object(probe.os, 'getpid', return_value=9):
            with self.assertRaisesRegex(RuntimeError, 'unwitnessed'):
                tree.scan()
        tree = self.tree({10: (10, 9, 1, 9, 10, 2)})
        with self.assertRaisesRegex(RuntimeError, 'identity changed'):
            tree.scan()

    def test_emergency_never_signals_recycled_or_escaped_pid(self):
        tree = self.tree({10: (10, 8, 1, 9, 10, 2), 11: (11, 2, 1, 10, 99, 2)})
        tree.known[11] = (11, 2, 1, 10, 10, 2)
        tree.child.wait = Mock()
        with patch.object(probe.os, 'kill') as kill:
            tree.emergency_stop_known()
            kill.assert_not_called()

    def test_native_abi_is_explicit_without_importing_runtime_or_libproc(self):
        self.assertEqual(probe.ctypes.sizeof(probe.BsdInfo), 136)

    def test_warning_window_keeps_host_state_and_rounds_intervals_up(self):
        window = probe.WarningWindow('synthetic-checker', 123, 0)
        with patch.object(probe, 'policy', side_effect=[
                {'allowed': True, 'warning_used_ms': 2},
                {'allowed': True, 'warning_used_ms': 4}]) as checker:
            window.observe({'pressure': 2, 'swap_bytes': 123}, .0011)
            self.assertEqual(checker.call_args.args[1]['delta_ms'], 2)
            window.observe({'pressure': 1, 'swap_bytes': 123}, .0022)
            body = checker.call_args.args[1]
            self.assertEqual(body['previous_pressure'], 2)
            self.assertEqual(body['warning_used_ms'], 2)
            self.assertEqual(body['baseline_swap_bytes'], 123)
            self.assertEqual(window.warning_ms, 4)
        with self.assertRaisesRegex(RuntimeError, 'clock regressed'):
            window.observe({'pressure': 1, 'swap_bytes': 123}, 0)

    def test_warning_calibration_cannot_load_qwen(self):
        with self.assertRaisesRegex(ValueError, 'only for Gemma E2B'):
            probe.run_one(None, None, probe.TAGS[1], calibration=True)

    def test_qwen_mode_selects_only_qwen_with_its_own_approval_and_policy(self):
        self.assertEqual(probe.tags_for_mode('qwen-calibration'), (probe.TAGS[1],))
        self.assertEqual(probe.tags_for_mode('gemma-calibration'), (probe.TAGS[0],))
        self.assertNotEqual(probe.approval_for_mode('qwen-calibration'),
                            probe.approval_for_mode('gemma-calibration'))
        with self.assertRaisesRegex(ValueError, 'only for selected Qwen'):
            probe.run_one(None, None, probe.TAGS[0], qwen_calibration=True)
        with self.assertRaisesRegex(ValueError, 'cannot be combined'):
            probe.run_one(None, None, probe.TAGS[1], calibration=True, qwen_calibration=True)
        window = probe.WarningWindow('synthetic-checker', 123, 0, qwen=True)
        with patch.object(probe, 'policy', return_value={'allowed': True, 'warning_used_ms': 0}) as checker:
            window.observe({'pressure': 1, 'swap_bytes': 123}, .1)
            self.assertEqual(checker.call_args.args[1]['operation'], 'qwen_calibration_snapshot')

    def test_critical_mode_is_separate_and_still_only_qwen(self):
        self.assertEqual(probe.tags_for_mode('qwen-critical-calibration'), (probe.TAGS[1],))
        self.assertNotEqual(probe.approval_for_mode('qwen-critical-calibration'),
                            probe.approval_for_mode('qwen-calibration'))
        with self.assertRaisesRegex(ValueError, 'requires its Qwen selector'):
            probe.run_one(None, None, probe.TAGS[1], critical_calibration=True)
        with self.assertRaisesRegex(ValueError, 'only for selected Qwen'):
            probe.run_one(None, None, probe.TAGS[0], qwen_calibration=True, critical_calibration=True)
        with self.assertRaisesRegex(ValueError, 'Qwen-only'):
            probe.WarningWindow(None, 0, 0, critical=True)

    def test_warning_and_critical_share_one_accumulator_in_critical_mode(self):
        window = probe.WarningWindow('synthetic-checker', 123, 0, qwen=True, critical=True)
        with patch.object(probe, 'policy', side_effect=[
                {'allowed': True, 'high_pressure_used_ms': 100},
                {'allowed': True, 'high_pressure_used_ms': 200}]) as checker:
            window.observe({'pressure': 2, 'swap_bytes': 123}, .1)
            window.observe({'pressure': 4, 'swap_bytes': 123}, .2)
            body = checker.call_args.args[1]
            self.assertEqual(body['operation'], 'qwen_critical_calibration_snapshot')
            self.assertEqual(body['high_pressure_used_ms'], 100)
            self.assertNotIn('warning_used_ms', body)
            self.assertEqual(body['previous_pressure'], 2)

    def test_observe_mode_keeps_separate_approval_and_only_the_selected_qwen(self):
        self.assertEqual(probe.tags_for_mode('qwen-observe-calibration'), (probe.TAGS[1],))
        self.assertNotEqual(probe.approval_for_mode('qwen-observe-calibration'),
                            probe.approval_for_mode('qwen-critical-calibration'))
        with self.assertRaisesRegex(ValueError, 'separate Qwen selector'):
            probe.run_one(None, None, probe.TAGS[1], observe_memory=True)
        with self.assertRaisesRegex(ValueError, 'separate Qwen selector'):
            probe.run_one(None, None, probe.TAGS[1], qwen_calibration=True,
                          critical_calibration=True, observe_memory=True)
        with self.assertRaisesRegex(ValueError, 'only for selected Qwen'):
            probe.run_one(None, None, probe.TAGS[0], qwen_calibration=True, observe_memory=True)

    def test_memory_probe_failure_is_missing_not_zero_only_in_observe_mode(self):
        with patch.object(probe, 'snapshot', side_effect=RuntimeError('synthetic probe error')):
            with self.assertRaisesRegex(RuntimeError, 'synthetic probe error'):
                probe.memory_sample()
            sample = probe.memory_sample(observe=True)
            self.assertIsNone(sample['pressure'])
            self.assertIsNone(sample['swap_bytes'])
            self.assertIn('synthetic probe error', sample['memory_probe_error'])
        with patch.object(probe, 'policy', return_value={'continue': True, 'not_admission': True}) as checker:
            probe.observation('fake-checker', sample, None)
            self.assertEqual(checker.call_args.args[1]['operation'], 'qwen_observe_snapshot')
            self.assertIsNone(checker.call_args.args[1]['baseline_swap_bytes'])

    def test_aborted_real_attempt_does_not_claim_response_or_pair_validation(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/background-jobs/v0.1.0/sequential-attempt.json'
        record = json.loads(path.read_text())
        self.assertTrue(record['inference_request_sent'])
        self.assertEqual(record['stopped_reason'], 'memory_pressure_warning')
        self.assertEqual(record['last_pressure'], 2)
        for field in ('generation_completed', 'response_validated', 'effective_context_validated',
                      'qwen_attempted', 'retry_attempted', 'concurrent_models_validated', 'cache_purged'):
            self.assertIs(record[field], False)
        for field in ('confirmed_owned_tree_stopped', 'confirmed_server_and_runner_ports_closed', 'lease_reacquired'):
            self.assertIs(record[field], True)
        self.assertIsNone(record['generation_started'])
        self.assertIsNone(record['mlx_active_bytes'])

    def test_completed_e2b_calibration_keeps_warning_and_missing_total_memory_visible(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/background-jobs/v0.1.0/e2b-calibration.json'
        r = json.loads(path.read_text())
        self.assertEqual(r['model'], probe.TAGS[0])
        self.assertEqual(r['policy'], 'ADR0040')
        self.assertTrue(r['generation_completed'])
        self.assertTrue(r['response_validated'])
        self.assertEqual(r['response_content'], 'SEIYA_OK')
        self.assertEqual(r['finish_reason'], 'stop')
        self.assertEqual(r['maximum_sampled_pressure'], 2)
        self.assertGreater(r['conservatively_accounted_warning_ms'], 0)
        self.assertLessEqual(r['conservatively_accounted_warning_ms'], 10000)
        self.assertLessEqual(r['maximum_sampled_swap_growth_bytes'], 512 * 1024 ** 2)
        for key in ('qwen_attempted', 'retry_attempted', 'concurrent_models_validated',
                    'total_model_memory_validated', 'sustained_session_validated', 'tools_validated'):
            self.assertIs(r[key], False)
        for key in ('mlx_active_bytes', 'metal_total_bytes', 'footprint_bytes'):
            self.assertIsNone(r[key])

    def test_qwen_critical_stop_is_not_warning_exhaustion_or_model_validation(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/background-jobs/v0.1.0/qwen-calibration-attempt.json'
        r = json.loads(path.read_text())
        self.assertEqual(r['model'], probe.TAGS[1])
        self.assertEqual(r['policy'], 'ADR0041')
        self.assertEqual(r['stopped_reason'], 'memory_pressure_critical')
        self.assertEqual(r['last_pressure'], 4)
        self.assertLess(r['conservatively_accounted_warning_ms'], r['warning_limit_ms'])
        self.assertLess(r['maximum_sampled_swap_growth_bytes'], r['swap_growth_limit_bytes'])
        for key in ('generation_completed', 'response_validated', 'effective_context_validated',
                    'gemma_attempted_in_this_run', 'retry_attempted', 'concurrent_models_validated',
                    'total_model_memory_validated', 'sustained_session_validated', 'tools_validated'):
            self.assertIs(r[key], False)
        for key in ('confirmed_owned_tree_stopped', 'confirmed_server_and_runner_ports_closed', 'lease_reacquired'):
            self.assertIs(r[key], True)
        self.assertIsNone(r['generation_started'])

    def test_qwen_critical_exception_stopped_on_swap_not_critical_or_time_budget(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/background-jobs/v0.1.0/qwen-critical-calibration-attempt.json'
        r = json.loads(path.read_text())
        self.assertEqual(r['policy'], 'ADR0042')
        self.assertEqual(r['model'], probe.TAGS[1])
        self.assertEqual(r['stopped_reason'], 'swap_growth_limit_exceeded')
        self.assertTrue(r['critical_sample_allowed_before_stop'])
        self.assertLess(r['conservatively_accounted_high_pressure_ms'], r['high_pressure_limit_ms'])
        self.assertGreater(r['maximum_sampled_swap_growth_bytes'], r['swap_growth_limit_bytes'])
        for key in ('generation_completed', 'response_validated', 'automatic_retry_attempted',
                    'gemma_attempted_in_this_run', 'total_model_memory_validated', 'concurrent_models_validated'):
            self.assertIs(r[key], False)
        for key in ('confirmed_owned_tree_stopped', 'confirmed_server_and_runner_ports_closed', 'lease_reacquired'):
            self.assertIs(r[key], True)

    def test_qwen_observation_completion_is_not_global_limit_removal_or_pair_validation(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/background-jobs/v0.1.0/qwen-observe-calibration.json'
        r = json.loads(path.read_text())
        self.assertEqual(r['policy'], 'ADR0043')
        self.assertEqual(r['model'], probe.TAGS[1])
        self.assertTrue(r['generation_completed'])
        self.assertTrue(r['response_validated'])
        self.assertEqual(r['response_content'], 'SEIYA_OK')
        self.assertEqual(r['finish_reason'], 'stop')
        self.assertEqual(r['deadline_seconds'], 120)
        for key in ('memory_stop_enabled', 'general_supervisor_limits_changed',
                    'automatic_retry_attempted', 'gemma_attempted_in_this_run',
                    'total_model_memory_validated', 'concurrent_models_validated',
                    'sustained_session_validated', 'tools_validated', 'effective_context_validated'):
            self.assertIs(r[key], False)
        self.assertIsNone(r['mlx_active_bytes'])
