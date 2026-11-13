"""Synthetic authenticated loopback HTTP only; never Rust DNS/TLS or model I/O."""
import json
import io
import os
from pathlib import Path
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch

from seiya.reader_client import ReaderClient, ReaderError, _DeadlineFile
from seiya.reader_frame import parse_request

FRAME = (Path(__file__).resolve().parents[2] / 'contracts/reader-ipc/v0.1-draft.1/request.json').read_bytes()
TOKEN = 'synthetic-reader-fixture-token'


class ReaderClientTests(unittest.TestCase):
    def setUp(self):
        self.mode, self.calls = 'ok', []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def log_message(self, *args):
                pass

            def do_POST(self):
                frame = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                owner.calls.append((self.path, frame, self.headers['Authorization']))
                if self.path.endswith('/cancel'):
                    raw, status = b'{"status":"cancellation_requested"}', 202
                else:
                    raw = json.dumps({'version': frame['version'], 'run_id': frame['run_id'], 'results': [
                        {'id': s['id'], 'status': 'policy_blocked', 'text': None, 'text_truncated': False,
                         'received_bytes': 0, 'decoded_bytes': 0, 'header_bytes': 0, 'get_count': 0}
                        for s in frame['sources']]}).encode()
                    status = 200
                if owner.mode == 'foreign':
                    raw = raw.replace(b'run-1', b'other-run')
                if owner.mode == 'redirect':
                    status = 302
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(raw) + (1 if owner.mode == 'truncated' else 0)))
                self.send_header('Connection', 'close')
                self.end_headers()
                self.wfile.write(raw)
                self.close_connection = True

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.environment = patch.dict(os.environ, {'SEIYA_TEST_READER_KEY': TOKEN})
        self.environment.start()
        self.config = {'base_url': 'http://127.0.0.1:%d/internal/reader/v0.1' % self.server.server_port,
                       'api_key_env': 'SEIYA_TEST_READER_KEY'}

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.environment.stop()

    def test_roundtrip_and_cancel_use_key_without_model_binding_or_tool_execution(self):
        client = ReaderClient(self.config)
        raw = client(parse_request(FRAME), time.monotonic() + 1)
        self.assertEqual(json.loads(raw)['results'][0]['status'], 'policy_blocked')
        self.assertEqual(client.cancel('run-1', time.monotonic() + 1),
                         {'status': 'cancellation_requested', 'cleanup_confirmed': False})
        self.assertEqual(len(self.calls), 2)
        self.assertTrue(all(call[2] == 'Bearer ' + TOKEN for call in self.calls))

    def test_ambiguous_responses_block_new_reads_without_retry(self):
        for mode in ('foreign', 'redirect', 'truncated'):
            self.mode = mode
            client = ReaderClient(self.config)
            before = len(self.calls)
            with self.assertRaises(ReaderError):
                client(parse_request(FRAME), time.monotonic() + 1)
            with self.assertRaises(ReaderError):
                client(parse_request(FRAME), time.monotonic() + 1)
            self.assertEqual(len(self.calls), before + 1)

    def test_config_cannot_select_remote_or_proxy_and_expiry_never_dispatches(self):
        for base in ('https://example.org', 'http://localhost:123/internal/reader/v0.1',
                     self.config['base_url'] + '?key=x'):
            with self.assertRaises(ValueError):
                ReaderClient(dict(self.config, base_url=base))
        client = ReaderClient(self.config)
        with self.assertRaises(ReaderError):
            client(parse_request(FRAME), time.monotonic() - 1)
        self.assertEqual(self.calls, [])

    def test_absolute_header_deadline_is_not_reset_by_each_byte(self):
        class Socket:
            def settimeout(self, value):
                self.timeout = value
        ticks = iter([0, .01, .02, .03, .04, .05, .06])
        file = _DeadlineFile(io.BytesIO(b'HTTP/1.1 200 OK\r\n'), Socket(), .05, lambda: next(ticks))
        with self.assertRaises(TimeoutError):
            file.readline()
        file.close()
