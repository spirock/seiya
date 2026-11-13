"""Browser integration with synthetic models/readers only, never external I/O."""
import json
import threading
import time
import unittest
import tempfile
from pathlib import Path
import urllib.error
import urllib.request

from seiya.config import example
from seiya.config import load
from seiya.coordinator import Coordinator, BackendError
from seiya.reader_profile import BrowserReader
from seiya.server import make_server

PROFILE = {'base_url': 'http://127.0.0.1:8012/internal/reader/v0.1', 'api_key_env': 'SYNTHETIC_KEY',
           'context_tokens': 4096, 'output_tokens': 256, 'margin_tokens': 128}


class FakeClient:
    port = 8012
    def __init__(self):
        self.cancels = []
    def __call__(self, frame, deadline):
        return json.dumps({'version': frame['version'], 'run_id': frame['run_id'], 'results': [
            {'id': s['id'], 'status': 'ok', 'text': 'Synthetic article. ' * 6, 'text_truncated': False,
             'received_bytes': 128, 'decoded_bytes': 128, 'header_bytes': 64, 'get_count': 1} for s in frame['sources']]}).encode()
    def select(self, frame, raw, ids):
        return [{k: p[k] for k in ('id', 'text', 'text_truncated')} for p in json.loads(raw)['results']]
    def cancel(self, run_id, deadline):
        self.cancels.append(run_id)


class ReaderProfileTests(unittest.TestCase):
    def setUp(self):
        self.client = FakeClient()
        self.profile = BrowserReader(PROFILE, client=self.client,
            search=lambda *_: [{'title': '<b>untrusted</b>', 'url': 'https://example.org/s1', 'snippet': 'Snippet'}])
        self.requests = []
        def transport(url, body, timeout):
            self.requests.append(body)
            action = [('web_search', {'query': 'synthetic'}), ('web_read', {'ids': ['s1']}),
                      ('web_answer', {'summary': 'Synthetic summary.', 'citations': ['s1']})][(len(self.requests)-1) % 3]
            return {'choices': [{'message': {'role': 'assistant', 'tool_calls': [{'id': 'action', 'type': 'function',
                'function': {'name': action[0], 'arguments': json.dumps(action[1])}}]}}]}
        config = example('mlx')
        config['mode'], config['models'] = 'fast', config['models'][:1]
        self.coordinator = Coordinator(config, transport)

    def test_host_profile_admits_estimate_and_consumes_run_once(self):
        ref = self.profile.create()
        body = {'messages': [{'role': 'user', 'content': 'Summarize'}], 'reader_run': ref}
        result = self.profile.complete(self.coordinator, body)
        self.assertEqual(result['web']['citations'], ['s1'])
        self.assertGreater(result['web']['reads'][0]['supplied_characters'], 0)
        self.assertFalse(result['web']['context_budget']['tokenizer_verified'])
        self.assertEqual(self.profile.status(ref)['phase'], 'finished')
        with self.assertRaises(ValueError):
            self.profile.complete(self.coordinator, body)
        self.assertFalse(self.coordinator.lock.locked())

    def test_selection_truncation_is_visible_even_if_full_page_was_not_truncated(self):
        def select(frame, raw, ids):
            original = json.loads(raw)['results'][0]
            return [{'id': original['id'], 'text': original['text'][:80], 'text_truncated': True}]
        self.client.select = select
        ref = self.profile.create()
        result = self.profile.complete(self.coordinator, {'messages': [{'role': 'user', 'content': 'x'}], 'reader_run': ref})
        self.assertTrue(result['web']['reads'][0]['text_truncated'])
        self.assertEqual(result['web']['reads'][0]['supplied_characters'], 80)
        self.assertEqual(result['web']['pipeline_outcome'], 'partial')

    def test_cancel_before_upload_never_calls_model(self):
        ref = self.profile.create()
        self.profile.cancel(ref)
        with self.assertRaises(BackendError):
            self.profile.complete(self.coordinator, {'messages': [{'role': 'user', 'content': 'x'}], 'reader_run': ref})
        self.assertEqual(self.requests, [])

    def test_example_profile_is_opt_in_and_registry_is_bounded(self):
        path = Path(__file__).resolve().parents[2] / 'contracts/reader-ipc/v0.1-draft.1/python-profile.example.json'
        data = json.loads(path.read_bytes())
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / 'profile.json'
            config.write_text(json.dumps(data))
            self.assertIn('web_reader', load(config))
            data['mode'] = 'consensus'
            config.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                load(config)
        refs = [self.profile.create() for _ in range(128)]
        from seiya.coordinator import BusyError
        with self.assertRaises(BusyError):
            self.profile.create()
        with self.assertRaises(ValueError):
            self.profile.status(dict(refs[0], token='a' * 64))

    def test_slow_search_uses_original_deadline_and_host_run_id(self):
        now = [100.0]
        deadlines = []
        def search(query, deadline):
            deadlines.append(deadline)
            now[0] += 44.0
            return [{'title': 'x', 'url': 'https://example.org', 'snippet': 'x'}]
        profile = BrowserReader(PROFILE, client=self.client, search=search, clock=lambda: now[0])
        ref = profile.create()
        result = profile.complete(self.coordinator, {'messages': [{'role': 'user', 'content': 'Summarize'}], 'reader_run': ref})
        self.assertEqual(deadlines, [145.0])
        self.assertEqual(result['web']['run_id'], ref['run_id'])

    def test_context_failure_does_not_truncate_history_or_dispatch(self):
        ref = self.profile.create()
        body = {'messages': [{'role': 'user', 'content': 'a' * 5000}], 'reader_run': ref}
        with self.assertRaises(BackendError):
            self.profile.complete(self.coordinator, body)
        self.assertEqual(len(body['messages'][0]['content']), 5000)
        self.assertEqual(self.requests, [])

    def test_frontend_controls_auth_and_v1_isolation(self):
        with self.assertRaises(ValueError):
            make_server(self.coordinator, '127.0.0.1', 0, reader_profile=self.profile)
        server = make_server(self.coordinator, '127.0.0.1', 0, 'synthetic-token', self.profile)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        def post(path, data, token='synthetic-token'):
            req = urllib.request.Request('http://127.0.0.1:%s%s' % (server.server_port, path),
                data=json.dumps(data).encode(), headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + token})
            with urllib.request.urlopen(req, timeout=3) as response:
                return json.load(response)
        try:
            with self.assertRaises(urllib.error.HTTPError) as error:
                post('/api/reader/runs', {}, 'wrong')
            self.assertEqual(error.exception.code, 401)
            error.exception.close()
            ref = post('/api/reader/runs', {})
            result = post('/api/chat', {'messages': [{'role': 'user', 'content': 'Summarize'}], 'reader_run': ref})
            self.assertEqual(result['web']['citations'], ['s1'])
            self.assertEqual(post('/api/reader/status', ref)['phase'], 'finished')
            with self.assertRaises(urllib.error.HTTPError) as error:
                post('/v1/chat/completions', {'messages': [{'role': 'user', 'content': 'x'}], 'reader_run': ref})
            self.assertEqual(error.exception.code, 400)
            error.exception.close()
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def test_cancel_during_inference_keeps_slot_until_runtime_reply(self):
        entered, release = threading.Event(), threading.Event()
        def blocked(url, body, timeout):
            entered.set(); release.wait(3)
            return {'choices': [{'message': {'role': 'assistant', 'content': 'late'}}]}
        self.coordinator.transport = blocked
        ref = self.profile.create()
        errors = []
        def work():
            try:
                self.profile.complete(self.coordinator, {'messages': [{'role': 'user', 'content': 'x'}], 'reader_run': ref})
            except BackendError as error:
                errors.append(str(error))
        thread = threading.Thread(target=work)
        thread.start()
        try:
            self.assertTrue(entered.wait(2))
            self.profile.cancel(ref)
            self.assertTrue(self.coordinator.lock.locked())
            self.assertEqual(self.profile.status(ref)['phase'], 'cancelling')
        finally:
            release.set(); thread.join()
        self.assertTrue(errors[0].startswith('reader_cancelled:'))
        self.assertFalse(self.coordinator.lock.locked())
