import copy
import importlib.util
import json
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

from seiya.config import example, load
from seiya.coordinator import Coordinator, BackendError, BusyError, UnloadError
from seiya.server import make_server, validate


def config():
    c = example('mlx')
    c['max_output_tokens'] = 256
    return c


class Fake:
    def __init__(self, malformed=False):
        self.calls = []
        self.malformed = malformed

    def __call__(self, url, body, timeout):
        self.calls.append(copy.deepcopy(body))
        if url.endswith('/api/generate'):
            return {'done': True}
        if 'Evaluate candidate' in body['messages'][0]['content']:
            if self.malformed:
                message = {'role': 'assistant', 'content': '```json\n{}\n```'}
            else:
                message = {'role': 'assistant', 'content': None, 'tool_calls': [
                    {'id': 'vote-1', 'type': 'function', 'function': {
                        'name': 'cast_vote', 'arguments': json.dumps({'choice': '1', 'reason': 'correct'})}}]}
        elif body.get('tools'):
            message = {'role': 'assistant', 'content': None, 'tool_calls': [
                {'id': 'call-1', 'type': 'function', 'function':
                 {'name': 'read_file', 'arguments': '{"path":"a.txt"}'}}]}
        elif body['messages'][-1]['role'] == 'tool':
            message = {'role': 'assistant', 'content': 'actual-file-content'}
        else:
            message = {'role': 'assistant', 'content': '4'}
        return {'choices': [{'message': message, 'finish_reason': 'tool_calls' if message.get('tool_calls') else 'stop'}],
                'usage': {'prompt_tokens': 10, 'completion_tokens': 1, 'total_tokens': 11}}


class Tests(unittest.TestCase):
    def test_config_and_no_overcommit_endpoints(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / 'config.json'
            c = config(); p.write_text(json.dumps(c)); self.assertEqual(load(p)['worker'], 'gemma')
            c['models'][1]['base_url'] = 'http://127.0.0.1:9000/v1'
            p.write_text(json.dumps(c))
            with self.assertRaises(ValueError): load(p)
            c = config(); c['models'][0]['base_url'] = 'https://remote.test/v1'
            p.write_text(json.dumps(c))
            with self.assertRaises(ValueError): load(p)

    def test_consensus_identical_text_not_split(self):
        f = Fake(); r = Coordinator(config(), f).complete({'messages': [{'role': 'user', 'content': '2+2'}]})
        self.assertEqual(r['model'], 'pegassus')
        self.assertEqual(r['choices'][0]['message']['content'], '4')
        self.assertEqual(sum(r['swarm']['votes'].values()), 3)
        self.assertFalse(r['swarm']['tests_executed'])
        self.assertEqual(len(f.calls), 6)
        for request in f.calls[:3]:
            self.assertEqual(request['messages'], [{'role': 'user', 'content': '2+2'}])

    def test_bad_votes_fail_closed(self):
        with self.assertRaisesRegex(BackendError, 'insufficient'):
            Coordinator(config(), Fake(malformed=True)).complete({'messages': [{'role': 'user', 'content': 'x'}]})

    def test_tools_forward_and_result(self):
        f = Fake(); c = Coordinator(config(), f)
        tools = [{'type': 'function', 'function': {'name': 'read_file', 'parameters': {'type': 'object'}}}]
        messages = [{'role': 'user', 'content': 'read a.txt'}]
        result = c.complete({'messages': messages, 'tools': tools})
        self.assertEqual(result['swarm']['mode'], 'tools')
        self.assertEqual(len(f.calls), 1)
        call = result['choices'][0]['message']['tool_calls'][0]
        messages += [result['choices'][0]['message'], {'role': 'tool', 'tool_call_id': call['id'], 'content': 'actual-file-content'}]
        result = c.complete({'messages': messages})
        self.assertEqual(result['choices'][0]['message']['content'], 'actual-file-content')
        self.assertEqual(len(f.calls), 2)

    def test_opencode_profile_uses_one_worker_and_larger_output(self):
        profile = json.loads((Path(__file__).resolve().parents[1] / 'opencode-provider.json').read_text())
        model = profile['providers']['seiya']['models']['pegassus']
        self.assertEqual(model['limit'], {'context': 32768, 'output': 2048})
        self.assertEqual(model['body'], {'swarm_mode': 'fast'})
        c = example('mlx'); f = Fake()
        Coordinator(c, f).complete(dict(model['body'], max_tokens=model['limit']['output'],
            messages=[{'role': 'user', 'content': 'Summarize this session'}]))
        self.assertEqual(len(f.calls), 1)
        self.assertEqual(f.calls[0]['max_tokens'], 2048)

    def test_bad_tool_never_execute(self):
        with self.assertRaises(BackendError):
            Coordinator._check_tools({'tool_calls': [{'id': 'a', 'type': 'function', 'function':
                {'name': 'delete_everything', 'arguments': '{}'}}]}, [])

    def test_ollama_unloads(self):
        c = config()
        for m in c['models']: m['backend'] = 'ollama'
        f = Fake(); Coordinator(c, f).complete({'messages': [{'role': 'user', 'content': 'x'}], 'swarm_mode': 'fast'})
        self.assertEqual(f.calls[-1]['keep_alive'], 0)

    def test_busy(self):
        c = Coordinator(config(), Fake(), queue_timeout=0.01); c.lock.acquire()
        with self.assertRaises(BusyError): c.complete({})
        c.lock.release()

    def test_waiting_request_runs_after_active_request(self):
        c = Coordinator(config(), Fake(), queue_timeout=2)
        c.acquire_slot()
        started, finished = threading.Event(), threading.Event()
        results, errors = [], []
        def waiting():
            started.set()
            try:
                results.append(c.complete({'messages': [{'role': 'user', 'content': 'hi'}], 'swarm_mode': 'fast'}))
            except Exception as error:
                errors.append(error)
            finally:
                finished.set()
        thread = threading.Thread(target=waiting)
        thread.start()
        try:
            self.assertTrue(started.wait(1))
            self.assertFalse(finished.wait(0.05))
        finally:
            c.release_slot()
            thread.join(3)
        self.assertFalse(thread.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(len(results), 1)

    def test_queue_bound_and_timeout_release_capacity(self):
        c = Coordinator(config(), Fake(), queue_timeout=0.01)
        c.acquire_slot()
        c.slots.acquire(); c.slots.acquire()
        try:
            with self.assertRaisesRegex(BusyError, 'queue full'): c.complete({})
        finally:
            c.slots.release(); c.slots.release()
        with self.assertRaisesRegex(BusyError, 'timed out'): c.complete({})
        c.release_slot()
        # Timeout must not leak capacity.
        for _ in range(3): self.assertTrue(c.slots.acquire(blocking=False))
        for _ in range(3): c.slots.release()

    def test_unload_failure_stops_before_next_model(self):
        c = config()
        for m in c['models']: m['backend'] = 'ollama'
        fake = Fake()
        def broken(url, body, timeout):
            if url.endswith('/api/generate'):
                raise BackendError('unload failed')
            return fake(url, body, timeout)
        with self.assertRaises(UnloadError):
            Coordinator(c, broken).complete({'messages': [{'role': 'user', 'content': 'x'}]})
        self.assertEqual(len(fake.calls), 1)

    def test_split_ballots_do_not_claim_majority(self):
        count = 0
        def divergent(url, body, timeout):
            nonlocal count
            count += 1
            if count <= 3:
                return {'choices': [{'message': {'role': 'assistant', 'content': f'candidate {count}'}}]}
            return {'choices': [{'message': {'role': 'assistant', 'tool_calls': [
                {'id': f'vote{count}', 'type': 'function', 'function': {
                    'name': 'cast_vote', 'arguments': '{"choice":"1","reason":"best"}'}}]}}]}
        with self.assertRaisesRegex(BackendError, 'no absolute majority'):
            Coordinator(config(), divergent).complete({'messages': [{'role': 'user', 'content': 'x'}]})

    def test_validation(self):
        for payload in [[], {}, {'messages': []}, {'messages': [{'role': 'user', 'content': []}]},
                        {'messages': [{'role': 'user', 'content': 'hi'}], 'tool_choice': 'required'}]:
            with self.assertRaises(ValueError): validate(payload)

    def test_http_models_auth_stream(self):
        c = Coordinator(config(), Fake())
        server = make_server(c, '127.0.0.1', 0, 'secret')
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        base = f'http://127.0.0.1:{server.server_port}'
        try:
            with self.assertRaises(urllib.error.HTTPError) as error: urllib.request.urlopen(base+'/v1/models')
            error.exception.close()
            request = urllib.request.Request(base+'/v1/models', headers={'Authorization': 'Bearer secret'})
            with urllib.request.urlopen(request) as r: self.assertEqual(json.load(r)['data'][0]['id'], 'pegassus')
            body = {'model': 'pegassus', 'messages': [{'role': 'user', 'content': 'x'}], 'stream': True, 'swarm_mode': 'fast'}
            request = urllib.request.Request(base+'/v1/chat/completions', data=json.dumps(body).encode(),
                                             headers={'Authorization': 'Bearer secret', 'Content-Type': 'application/json'})
            with urllib.request.urlopen(request) as r:
                text = r.read().decode(); self.assertIn('data: [DONE]', text); self.assertIn('"model": "pegassus"', text)
            request = urllib.request.Request(base+'/health', headers={'Origin': 'https://evil.test', 'Authorization': 'Bearer secret'})
            with self.assertRaises(urllib.error.HTTPError) as error: urllib.request.urlopen(request)
            self.assertEqual(error.exception.code, 403)
            error.exception.close()
        finally:
            server.shutdown(); server.server_close(); thread.join()


if __name__ == '__main__': unittest.main()
