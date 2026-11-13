"""Offline invariants of a specified experiment, NOT tests of a sampler."""
import json
from pathlib import Path
import unittest


class BaselinePlanTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/memory-baseline/v0.1.0/plan.json'
        cls.plan = json.loads(path.read_text())

    def test_implemented_runner_does_not_claim_real_evidence(self):
        self.assertEqual(self.plan['status'], 'specified-not-executed')
        self.assertIs(self.plan['runner_implemented'], True)
        self.assertIs(self.plan['authorization_required'], True)
        self.assertEqual(self.plan['runtime_policy'], 'one-runtime-at-a-time')

    def test_budgets_and_runtime_parameters_are_equal_for_both_arms(self):
        comparison = self.plan['comparison']
        budget = comparison['request']['max_tokens']
        self.assertEqual(budget, comparison['runtime_settings']['max_output_tokens'])
        for arm in comparison['arms']:
            self.assertEqual(comparison['arms_config'][arm]['max_output_tokens'], budget)
        self.assertEqual(comparison['arms_config']['python']['mode'], 'fast')
        self.assertEqual(comparison['request']['swarm_mode'], 'fast')
        self.assertNotIn('max_completion_tokens', comparison['request'])
        self.assertFalse(comparison['auxiliary_requests_allowed'])

    def test_order_is_balanced_and_does_not_claim_cold_disk(self):
        comparison = self.plan['comparison']
        order = comparison['block_order'] * comparison['blocks']
        self.assertEqual(order.count('python'), order.count('rust'))
        self.assertEqual(comparison['cold_definition'], 'fresh-runtime-process-not-cold-disk')
        phases = [phase['phase'] for phase in self.plan['workload']]
        self.assertLess(phases.index('cold-text'), phases.index('warm-repeat'))
        self.assertLess(phases.index('idle'), phases.index('resume'))
        self.assertEqual(phases[-1], 'shutdown')

    def test_missing_memory_metrics_are_null_not_invented_zero(self):
        for metric in self.plan['metrics']:
            self.assertIn(metric['scope'], ['each-owned-pid', 'runtime', 'whole-host'])
            if metric['availability'] == 'unsupported':
                self.assertIsNone(metric['method'])
        self.assertIsNone(self.plan['unsupported_sample']['value'])
        self.assertEqual(self.plan['unsupported_sample']['availability'], 'unsupported')
        self.assertFalse(self.plan['aggregation']['sum_overlapping_metrics'])
        self.assertFalse(self.plan['aggregation']['sum_process_rss_as_physical_memory'])

    def test_process_roles_and_sampling_limits_are_explicit(self):
        self.assertIn('collector', self.plan['required_process_roles'])
        self.assertIn('runtime', self.plan['required_process_roles'])
        self.assertIn('opencode-private-server', self.plan['optional_process_roles'])
        self.assertEqual(self.plan['sampling']['clock'], 'monotonic')
        self.assertTrue(self.plan['sampling']['phase_boundary_snapshots'])
        self.assertTrue(self.plan['sampling']['record_missing_samples'])
        payloads = self.plan['payloads']
        self.assertEqual(payloads['synthetic_tool_result'], payloads['expected_final_text'])
        self.assertEqual(payloads['expected_tool_arguments'], {'path': 'fixture.txt'})
        self.assertIs(payloads['execute_host_tools'], False)

    def test_plan_cannot_authorize_optimization_or_destructive_cleanup(self):
        self.assertEqual(self.plan['ownership'], 'own-processes-only')
        for key, value in self.plan['safety'].items():
            if key != 'network': self.assertIs(value, False)
        self.assertEqual(self.plan['safety']['network'], 'loopback-only')
        for reason in ['unconfirmed-runtime-state', 'pid-ownership-mismatch', 'effective-budget-mismatch', 'user-stop']:
            self.assertIn(reason, self.plan['stop_conditions'])

    def test_recorded_calibration_is_not_a_complete_budget_or_ab_baseline(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/memory-baseline/v0.1.0/gemma-calibration-2026-11-10.json'
        report = json.loads(path.read_text())
        self.assertEqual(report['mode'], 'real_single_experimental_calibration')
        self.assertTrue(report['completed'])
        self.assertTrue(report['cleanup_confirmed_by_driver'])
        self.assertTrue(report['lease_reacquired_after_exit'])
        self.assertFalse(report['normal_admission_policy_changed'])
        for field in ('baseline_ab_comparison', 'budget_categories_validated', 'long_sessions_validated',
                      'effective_context_4096_verified', 'restricted_runtime_compatibility_validated', 'capacity_guaranteed'):
            self.assertIs(report[field], False)
        for metric in ('footprint_bytes', 'mlx_active_memory_bytes', 'metal_allocated_memory_bytes', 'ttft_millis'):
            self.assertIsNone(report[metric])
        phases = report['runtime_rss_by_phase']
        self.assertEqual(sum(p['samples'] for p in phases), report['runtime_rss_sample_count'])
        self.assertEqual(max(p['max_bytes'] for p in phases), report['runtime_rss_sampled_max_bytes'])
        self.assertLessEqual(report['runtime_elapsed_millis_observed'], report['runtime_limit_seconds'] * 1000)
        self.assertLessEqual(report['completion_tokens_reported'], report['output_limit_tokens'])

    def test_instrumented_calibration_keeps_overlapping_counters_separate(self):
        path = Path(__file__).resolve().parents[1] / 'contracts/benchmarks/memory-baseline/v0.1.0/gemma-calibration-instrumented-2026-11-11.json'
        report = json.loads(path.read_text())
        self.assertTrue(report['completed'])
        self.assertTrue(report['runtime_memory_enabled'])
        self.assertTrue(report['approval_consumed'])
        self.assertTrue(report['cleanup_confirmed_by_driver'])
        self.assertTrue(report['lease_reacquired_after_exit'])
        for field in ('reads_are_atomic', 'rss_and_allocator_counters_are_additive', 'budget_categories_validated',
                      'normal_admission_policy_changed', 'capacity_guaranteed', 'baseline_ab_comparison',
                      'long_sessions_validated', 'effective_context_4096_verified',
                      'restricted_runtime_compatibility_validated', 'speedup_against_previous_calibration_proven'):
            self.assertIs(report[field], False)
        for field in ('total_physical_memory_bytes', 'footprint_bytes', 'metal_allocated_memory_bytes', 'ttft_millis'):
            self.assertIsNone(report[field])
        self.assertEqual(report['peak_scope'], 'runtime_global_external_resets_unknown')
        self.assertEqual(report['runtime_memory_failures'], 0)
        self.assertEqual(report['runtime_memory_report_count'], report['runtime_memory_sequence_last'] + 1)
        self.assertEqual(report['runtime_memory_sequence_first'], 0)
        self.assertEqual(sum(p['samples'] for p in report['runtime_rss_by_phase']), report['runtime_rss_sample_count'])
        self.assertEqual(max(p['max_bytes'] for p in report['runtime_rss_by_phase']), report['runtime_rss_sampled_max_bytes'])
        for field in ('mlx_active_sampled_max_bytes', 'mlx_cache_sampled_max_bytes', 'mlx_allocator_peak_max_reported_bytes'):
            self.assertGreater(report[field], 0)
        self.assertLessEqual(report['completion_tokens_reported'], report['output_limit_tokens'])
        self.assertLessEqual(report['runtime_elapsed_millis_observed'], report['runtime_limit_seconds'] * 1000)
