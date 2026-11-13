"""Peer protocol tested in memory, with counters that never import MLX."""
import io
import json
import unittest
from unittest.mock import patch

from seiya import runtime_memory_bootstrap as peer
from seiya.runtime_memory import MAX_REPORT_BYTES, RuntimeMemorySampler


class Buffer(io.BytesIO):
    def close(self):
        self.closed_by_peer = True


class RuntimeMemoryPeerTests(unittest.TestCase):
    def run_peer(self, request, sampler=None):
        reader, writer = Buffer(request), Buffer()
        with patch.object(peer, 'MAX_REPORT_BYTES', MAX_REPORT_BYTES, create=True):
            peer._memory_peer(reader, writer, sampler or RuntimeMemorySampler(object()))
        self.assertTrue(reader.closed_by_peer)
        self.assertTrue(writer.closed_by_peer)
        return writer.getvalue()

    def test_multiple_samples_eof_and_missing_core(self):
        data = self.run_peer(b'sample\nsample\n')
        reports = [json.loads(line) for line in data.splitlines()]
        self.assertEqual([r['sequence'] for r in reports], [0, 1])
        self.assertEqual(reports[0]['mlx_active_bytes']['reason'], 'missing_api')

    def test_only_fixed_command_and_no_arguments(self):
        for request in (b'', b'SAMPLE\n', b'sample x\n', b'sample', b'x' * 100):
            with self.subTest(request=request):
                self.assertEqual(self.run_peer(request), b'')

    def test_sample_limit(self):
        self.assertEqual(len(self.run_peer(b'sample\n' * 1025).splitlines()), 1024)

    def test_oversize_report_never_written(self):
        class Sampler:
            def sample(self):
                return {'data': 'x' * MAX_REPORT_BYTES}
        self.assertEqual(self.run_peer(b'sample\n', Sampler()), b'')

    def test_loaded_core_proxy_does_not_import_any_runtime(self):
        with patch.dict(peer.sys.modules, {'mlx.core': None}):
            self.assertIsNone(peer._LoadedCore().get_active_memory)
