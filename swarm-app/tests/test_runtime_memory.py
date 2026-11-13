"""Synthetic counters only; no MLX import, tensors, model or native probes."""
import builtins
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from seiya.runtime_memory import MAX_BYTES, MAX_REPORT_BYTES, RuntimeMemorySampler


FIXTURE = Path(__file__).resolve().parents[2] / 'contracts/benchmarks/memory-baseline/v0.1.0/runtime-memory.fixture.json'


class RuntimeMemoryTests(unittest.TestCase):
    def sample(self, core):
        return RuntimeMemorySampler(core, clock=iter([1000, 1100, 1125]).__next__).sample()

    def test_shared_fixture_and_zero(self):
        core = SimpleNamespace(get_active_memory=lambda: 1024,
                               get_cache_memory=lambda: 0,
                               get_peak_memory=lambda: 2048)
        with patch('seiya.runtime_memory.os.getpid', return_value=42):
            report = self.sample(core)
        self.assertEqual(report, json.loads(FIXTURE.read_text()))
        self.assertLessEqual(len(json.dumps(report).encode()), MAX_REPORT_BYTES)

    def test_no_runtime_import_or_mutation(self):
        original = builtins.__import__

        def guarded(name, *args, **kwargs):
            if name.split('.')[0] in ('mlx', 'mlx_lm'):
                self.fail('must not import a runtime')
            return original(name, *args, **kwargs)

        path = Path(__file__).resolve().parents[1] / 'seiya/runtime_memory.py'
        spec = importlib.util.spec_from_file_location('offline_memory_probe', path)
        module = importlib.util.module_from_spec(spec)
        with patch('builtins.__import__', side_effect=guarded):
            spec.loader.exec_module(module)
            # Every attribute except the three allowlisted reads fails loudly.
            class Core:
                def __getattr__(self, name):
                    self_test.assertIn(name, ('get_active_memory', 'get_cache_memory', 'get_peak_memory'))
                    return lambda: 0
            self_test = self
            module.RuntimeMemorySampler(Core()).sample()

    def test_missing_api_and_no_legacy_fallback(self):
        report = self.sample(SimpleNamespace(metal=object()))
        for field in ('mlx_active_bytes', 'mlx_cache_bytes', 'mlx_peak_bytes'):
            self.assertEqual(report[field], {'value': None, 'reason': 'missing_api'})

    def test_failure_redacts_exception_and_continues_other_reads(self):
        def failed():
            raise RuntimeError('/private/path/secret')
        report = self.sample(SimpleNamespace(get_active_memory=failed, get_cache_memory=lambda: 12))
        self.assertEqual(report['mlx_active_bytes'], {'value': None, 'reason': 'probe_failed'})
        self.assertEqual(report['mlx_cache_bytes']['value'], 12)
        self.assertNotIn('secret', json.dumps(report))

    def test_invalid_counter_values(self):
        for value in (-1, True, 1.0, '12', None, MAX_BYTES + 1):
            with self.subTest(value=value):
                report = self.sample(SimpleNamespace(get_active_memory=lambda: value))
                self.assertEqual(report['mlx_active_bytes'], {'value': None, 'reason': 'invalid_value'})
        self.assertEqual(self.sample(SimpleNamespace(get_active_memory=lambda: MAX_BYTES))['mlx_active_bytes']['value'], MAX_BYTES)

    def test_clock_and_sequence_are_relative(self):
        sampler = RuntimeMemorySampler(object(), clock=iter([100, 110, 120, 120, 125]).__next__)
        first, second = sampler.sample(), sampler.sample()
        self.assertEqual((first['sequence'], second['sequence']), (0, 1))
        self.assertEqual((second['read_started_ns'], second['read_finished_ns']), (20, 25))

    def test_clock_regression_overflow_and_bad_epoch(self):
        for times in ([100, 110, 109], [0, 0, MAX_BYTES + 1], [True], [-1]):
            with self.subTest(times=times), self.assertRaises(ValueError):
                RuntimeMemorySampler(object(), clock=iter(times).__next__).sample()

    def test_forked_sampler_rejected_before_counter_reads(self):
        sampler = RuntimeMemorySampler(object())
        with patch('seiya.runtime_memory.os.getpid', return_value=sampler._pid + 1):
            with self.assertRaises(ValueError):
                sampler.sample()

    def test_exhausted_sequence(self):
        sampler = RuntimeMemorySampler(object())
        sampler._sequence = MAX_BYTES + 1
        with self.assertRaises(ValueError):
            sampler.sample()
