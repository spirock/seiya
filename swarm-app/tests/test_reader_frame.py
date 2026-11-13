"""Shared synthetic fixtures only; no model, subprocess, or network."""
import json
from pathlib import Path
import unittest

from seiya.reader_frame import parse_request, validate_response

FIXTURES = Path(__file__).resolve().parents[2] / 'contracts/reader-ipc/v0.1-draft.1'


class ReaderFrameTests(unittest.TestCase):
    def setUp(self):
        self.request = (FIXTURES / 'request.json').read_bytes()
        self.response = (FIXTURES / 'response.json').read_bytes()

    def test_shared_roundtrip(self):
        request = parse_request(self.request)
        self.assertEqual(validate_response(request, self.response)['results'][0]['status'], 'ok')

    def test_shared_adversarial_corpus(self):
        req = parse_request(self.request)
        for case in json.loads((FIXTURES / 'invalid-frames.json').read_bytes()):
            with self.subTest(kind=case['kind'], frame=case['frame']):
                with self.assertRaises(ValueError):
                    if case['kind'] == 'request':
                        parse_request(case['frame'].encode())
                    else:
                        validate_response(req, case['frame'].encode())

    def test_duplicate_trailing_eof_and_encoding(self):
        for data in [self.request + b'{}', b'\xff', self.request.replace(b'"run_id":', b'"run_id":"x","run_id":'),
                     b' ' * 16385, self.request.replace(b'12000', b'true'), self.request.replace(b'12000', b'1.0')]:
            with self.assertRaises(ValueError):
                parse_request(data)
        with self.assertRaises(ValueError):
            parse_request(self.request, eof=False)

    def test_response_binding_and_shapes(self):
        request = parse_request(self.request)
        for data in [self.response.replace(b'run-1', b'other'), self.response + b'{}',
                     self.response.replace(b'"get_count":1', b'"get_count":true'),
                     self.response.replace(b'"header_bytes":64', b'"header_bytes":65537')]:
            with self.assertRaises(ValueError):
                validate_response(request, data)

    def test_error_does_not_return_text(self):
        req = parse_request(self.request)
        result = json.loads(self.response)
        result['results'][0].update(status='timeout', text=None)
        validate_response(req, json.dumps(result).encode())
        result['results'][0]['text_truncated'] = True
        with self.assertRaises(ValueError):
            validate_response(req, json.dumps(result).encode())
