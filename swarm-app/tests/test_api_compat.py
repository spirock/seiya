"""S1-01: shared wire fixtures against the Python reference, no real inference."""
import copy
import http.client
import json
import math
import re
import threading
import unittest
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from seiya.coordinator import BackendError, Coordinator
from seiya.server import make_server, validate


FIXTURES = Path(__file__).resolve().parents[2] / 'contracts/openai-compat/v0.1.0'
SCENARIOS = json.loads((FIXTURES / 'scenarios.json').read_text())
VALIDATION = json.loads((FIXTURES / 'validation.json').read_text())
HTTP_POLICY = json.loads((FIXTURES / 'http-policy.json').read_text())
TOKEN = 'synthetic-fixture-token-not-a-real-secret'


class ScriptedRuntime:
    """Records the actual forwarded request; never opens any runtime endpoint."""
    def __init__(self):
        self.pending = deque()
        self.calls = []
        self.lock = threading.Lock()
        self.entered = threading.Event()
        self.release = threading.Event()
        self.release.set()
        self.active = 0
        self.peak_active = 0

    def enqueue(self, step):
        with self.lock:
            self.pending.append(copy.deepcopy(step))

    def __call__(self, url, body, timeout):
        with self.lock:
            self.calls.append({'url': url, 'body': copy.deepcopy(body), 'timeout': timeout})
            if not self.pending:
                raise BackendError('unexpected fixture runtime request')
            step = self.pending.popleft()
            self.active += 1
            self.peak_active = max(self.peak_active, self.active)
        self.entered.set()
        try:
            if not self.release.wait(5):
                raise BackendError('fixture runtime gate timed out')
            if 'runtime_error' in step:
                raise BackendError(step['runtime_error'])
            return copy.deepcopy(step['runtime_response'])
        finally:
            with self.lock:
                self.active -= 1


class APICompatibilityTests(unittest.TestCase):
    def assert_json_equal(self, actual, expected):
        # Python equality treats True == 1; a wire contract must not do that.
        options = {'sort_keys': True, 'ensure_ascii': False, 'allow_nan': False}
        self.assertEqual(json.dumps(actual, **options), json.dumps(expected, **options))

    def setUp(self):
        self.runtime = ScriptedRuntime()
        self.config = copy.deepcopy(SCENARIOS['config'])
        self.coordinator = Coordinator(self.config, self.runtime, queue_timeout=0.03)
        self.server = make_server(self.coordinator, '127.0.0.1', 0, TOKEN)
        self.thread = threading.Thread(
            target=self.server.serve_forever, kwargs={'poll_interval': 0.01}, daemon=True)
        self.thread.start()
        self.addCleanup(self.close_server)

    def close_server(self):
        self.runtime.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(2)

    def exchange(self, method, path, body=None, headers=None, raw=None):
        connection = http.client.HTTPConnection('127.0.0.1', self.server.server_port, timeout=3)
        request_headers = {'Authorization': 'Bearer ' + TOKEN, 'Content-Type': 'application/json'}
        for key, value in (headers or {}).items():
            if value is None:
                request_headers.pop(key, None)
            else:
                request_headers[key] = value
        if raw is None and body is not None:
            raw = json.dumps(body, ensure_ascii=False).encode('utf-8')
        try:
            connection.request(method, path, body=raw, headers=request_headers)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read().decode('utf-8')
        finally:
            connection.close()

    def normalize_completion(self, response):
        result = copy.deepcopy(response)
        self.assertRegex(result['id'], re.compile(r'^chatcmpl-[0-9a-f]{32}$'))
        self.assertIs(type(result['created']), int)
        self.assertGreater(result['created'], 0)
        result['id'] = '<completion-id>'
        result['created'] = '<unix-seconds>'
        if 'swarm' in result:
            elapsed = result['swarm']['elapsed_seconds']
            self.assertIn(type(elapsed), (int, float))
            self.assertTrue(math.isfinite(elapsed))
            self.assertGreaterEqual(elapsed, 0)
            result['swarm']['elapsed_seconds'] = '<elapsed-seconds>'
        return result

    def assert_response(self, step, response):
        status, headers, text = response
        self.assertEqual(status, step['status'], text)
        self.assertEqual(headers['Cache-Control'], 'no-store')
        if 'expected_sse' in step:
            self.assertEqual(headers['Content-Type'], 'text/event-stream')
            self.assertEqual(headers['Connection'], 'close')
            self.assertTrue(text.endswith('data: [DONE]\n\n'))
            frames = text.split('\n\n')
            self.assertEqual(frames[-1], '')
            parsed = []
            ids, times = set(), set()
            for frame in frames[:-1]:
                self.assertTrue(frame.startswith('data: '))
                payload = frame[len('data: '):]
                if payload == '[DONE]':
                    parsed.append(payload)
                else:
                    chunk = json.loads(payload)
                    ids.add(chunk['id']); times.add(chunk['created'])
                    parsed.append(self.normalize_completion(chunk))
            self.assertEqual(len(ids), 1)
            self.assertEqual(len(times), 1)
            self.assert_json_equal(parsed, step['expected_sse'])
        else:
            self.assertEqual(headers['Content-Type'], 'application/json; charset=utf-8')
            parsed = json.loads(text)
            if parsed.get('object') == 'chat.completion':
                parsed = self.normalize_completion(parsed)
            self.assert_json_equal(parsed, step['expected'])

    def expected_runtime_call(self, step):
        body = step['body']
        expected = {
            'model': 'fixture-checkpoint', 'messages': body['messages'], 'stream': False,
            'max_tokens': 256, 'temperature': 0,
            'chat_template_kwargs': {'enable_thinking': False}}
        if body.get('tools'):
            expected['tools'] = body['tools']
        expected.update(step.get('runtime_request', {}))
        return {'url': 'http://127.0.0.1:65535/v1/chat/completions',
                'body': expected, 'timeout': 5}

    def run_scenario(self, scenario):
        for step in scenario['steps']:
            with self.subTest(step=step['path']):
                before = len(self.runtime.calls)
                uses_runtime = 'runtime_response' in step or 'runtime_error' in step
                if uses_runtime:
                    self.runtime.enqueue(step)
                response = self.exchange(step['method'], step['path'], step.get('body'))
                self.assert_response(step, response)
                expected = [self.expected_runtime_call(step)] if uses_runtime else []
                self.assert_json_equal(self.runtime.calls[before:], expected)
                self.assertFalse(self.runtime.pending)

    def assert_error(self, response, status, message):
        self.assert_response({'status': status, 'expected': {'error': {'message': message}}}, response)

    def test_http_auth_host_origin_and_unknown_post(self):
        for case in HTTP_POLICY['cases']:
            with self.subTest(case=case['id']):
                self.assert_error(self.exchange(case['method'], case['path'], {}, case['headers']),
                                  case['status'], case['error'])
        self.assertEqual(self.runtime.calls, [])

    def test_generated_message_tool_and_scalar_boundaries(self):
        limits = VALIDATION['limits']
        for count in (1, limits['messages']):
            validate(dict(VALIDATION['base_request'], messages=[{'role': 'user', 'content': 'x'}] * count))
        too_many = dict(VALIDATION['base_request'], messages=[{'role': 'user', 'content': 'x'}] * (limits['messages'] + 1))
        self.assert_error(self.exchange('POST', '/v1/chat/completions', too_many), 400,
                          'messages must contain 1–100 messages')
        tools = [{'type': 'function', 'function': {'name': f'tool_{i}', 'parameters': {}}}
                 for i in range(limits['tools'] + 1)]
        validate(dict(VALIDATION['base_request'], tools=tools[:-1]))
        self.assert_error(self.exchange('POST', '/v1/chat/completions', dict(VALIDATION['base_request'], tools=tools)),
                          400, 'tools must be a list of at most 64 function tools')
        for value in (limits['max_tokens_min'], limits['max_tokens_max']):
            validate(dict(VALIDATION['base_request'], max_tokens=value))
        for value in (limits['temperature_min'], limits['temperature_max']):
            validate(dict(VALIDATION['base_request'], temperature=value))
        self.assertEqual(self.runtime.calls, [])

    def test_raw_body_limit_and_parser_failures(self):
        limit = VALIDATION['limits']['body_bytes']
        step = copy.deepcopy(SCENARIOS['scenarios'][1]['steps'][0])
        encoded = json.dumps(step['body']).encode()
        self.runtime.enqueue(step)
        response = self.exchange('POST', '/v1/chat/completions', raw=encoded + b' ' * (limit - len(encoded)))
        self.assert_response(step, response)
        self.assertEqual(len(self.runtime.calls), 1)
        for declared_size in (0, limit + 1):
            # The server rejects the header before reading. Do not race a large
            # socket write against that early close (platform-dependent RST).
            self.assert_error(self.exchange('POST', '/v1/chat/completions', raw=b'',
                                            headers={'Content-Length': str(declared_size)}), 400,
                              'body must be 1–262144 bytes; chunked requests unsupported')
        self.assert_error(self.exchange('POST', '/v1/chat/completions', VALIDATION['base_request'],
                                        headers={'Transfer-Encoding': 'chunked'}),
                          400, 'body must be 1–262144 bytes; chunked requests unsupported')
        for raw in (b'{', b'\xff'):
            status, headers, text = self.exchange('POST', '/v1/chat/completions', raw=raw)
            self.assertEqual(status, 400)
            self.assertIsInstance(json.loads(text)['error']['message'], str)
        self.assertEqual(len(self.runtime.calls), 1)

    def test_queue_full_timeout_and_capacity_recovery(self):
        self.coordinator.acquire_slot()
        try:
            self.coordinator.slots.acquire(); self.coordinator.slots.acquire()
            try:
                self.assert_error(self.exchange('POST', '/v1/chat/completions', VALIDATION['base_request']),
                                  429, VALIDATION['queue_errors']['full'])
            finally:
                self.coordinator.slots.release(); self.coordinator.slots.release()
            self.assert_error(self.exchange('POST', '/v1/chat/completions', VALIDATION['base_request']),
                              429, VALIDATION['queue_errors']['timeout'])
        finally:
            self.coordinator.release_slot()
        self.assertEqual(self.runtime.calls, [])
        self.run_scenario(SCENARIOS['scenarios'][1])

    def test_concurrent_http_clients_are_serialized(self):
        step = copy.deepcopy(SCENARIOS['scenarios'][1]['steps'][0])
        self.runtime.enqueue(step); self.runtime.enqueue(step)
        self.runtime.release.clear()
        self.coordinator.queue_timeout = 2
        waiting = threading.Event()
        original = self.coordinator.acquire_slot
        def acquire_slot():
            if self.coordinator.lock.locked():
                waiting.set()
            original()
        self.coordinator.acquire_slot = acquire_slot
        def request():
            return self.exchange(step['method'], step['path'], step['body'])
        with ThreadPoolExecutor(max_workers=2) as pool:
            first = pool.submit(request)
            try:
                self.assertTrue(self.runtime.entered.wait(2))
                second = pool.submit(request)
                self.assertTrue(waiting.wait(2))
                health = self.exchange('GET', '/health')
                self.assertTrue(json.loads(health[2])['busy'])
                self.assertFalse(second.done())
            finally:
                self.runtime.release.set()
            for future in (first, second):
                self.assert_response(step, future.result(timeout=3))
        self.assertEqual(len(self.runtime.calls), 2)
        self.assertEqual(self.runtime.peak_active, 1)
        self.assertFalse(self.coordinator.lock.locked())


def scenario_test(scenario):
    def test(self):
        self.run_scenario(scenario)
    return test


def validation_test(case):
    def test(self):
        body = copy.deepcopy(case['body'] if 'body' in case else VALIDATION['base_request'])
        if 'patch' in case:
            body.update(copy.deepcopy(case['patch']))
        self.assert_error(self.exchange('POST', '/v1/chat/completions', body), 400, case['error'])
        self.assertEqual(self.runtime.calls, [])
    return test


def backend_call_test(case):
    def test(self):
        step = {
            'method': 'POST', 'path': '/v1/chat/completions',
            'body': dict(copy.deepcopy(VALIDATION['base_request']), tools=[{
                'type': 'function', 'function': {'name': 'read', 'parameters': {'type': 'object'}}}]),
            'runtime_response': {'choices': [{'message': {'role': 'assistant', 'content': None,
                                                         'tool_calls': case['calls']}}]},
            'status': 502, 'expected': {'error': {'message': case.get(
                'error', 'invalid structured tool call; nothing executed')}}}
        self.run_scenario({'steps': [step]})
    return test


for scenario in SCENARIOS['scenarios']:
    setattr(APICompatibilityTests, 'test_fixture_' + scenario['id'], scenario_test(scenario))
for case in VALIDATION['invalid_requests']:
    setattr(APICompatibilityTests, 'test_invalid_request_' + case['id'], validation_test(case))
for case in VALIDATION['invalid_backend_calls']:
    setattr(APICompatibilityTests, 'test_invalid_backend_' + case['id'], backend_call_test(case))
