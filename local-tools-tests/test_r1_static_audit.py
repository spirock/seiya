"""Synthetic audit regressions: no network, MLX, model weights or host probes."""
from pathlib import Path
import tempfile
import unittest

from seiya_r1_static_audit import small_json, validate_tensor_index


class R1AuditTests(unittest.TestCase):
    def test_matching_header_and_index_cover_exact_payload(self):
        header = {'a': {'data_offsets': [0, 4]}, 'b': {'data_offsets': [4, 8]}}
        index = {'weight_map': {'a': 'model.safetensors', 'b': 'model.safetensors'}}
        self.assertEqual(validate_tensor_index(header, 8, index), 2)

    def test_gaps_overlap_out_of_range_and_boolean_offsets_rejected(self):
        index = {'weight_map': {'a': 'model.safetensors', 'b': 'model.safetensors'}}
        for a, b in [([0, 3], [4, 8]), ([0, 5], [4, 8]), ([0, 4], [4, 9]),
                     ([False, 4], [4, 8]), ([0, 4], [-1, 8]), ([0, 4], [4, '8'])]:
            with self.subTest(a=a, b=b), self.assertRaises(ValueError):
                validate_tensor_index({'a': {'data_offsets': a}, 'b': {'data_offsets': b}}, 8, index)

    def test_empty_tensors_or_inconsistent_index_cannot_pass(self):
        with self.assertRaises(ValueError):
            validate_tensor_index({}, 0, {'weight_map': {}})
        header = {'a': {'data_offsets': [0, 4]}}
        for index in [{'weight_map': {}}, {'weight_map': {'b': 'model.safetensors'}},
                      {'weight_map': {'a': '../other.safetensors'}}]:
            with self.subTest(index=index), self.assertRaises(ValueError):
                validate_tensor_index(header, 4, index)

    def test_metadata_bounds_and_symlink_refusal(self):
        base = Path(__file__).resolve().parents[1] / 'target'
        base.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='r1-audit-', dir=base) as directory:
            path = Path(directory) / 'synthetic.json'
            path.write_text('{"synthetic":true}')
            self.assertEqual(small_json(path, 64), {'synthetic': True})
            with self.assertRaises(ValueError):
                small_json(path, 2)
            alias = Path(directory) / 'alias.json'
            alias.symlink_to(path)
            with self.assertRaises(ValueError):
                small_json(alias, 64)


if __name__ == '__main__':
    unittest.main()
