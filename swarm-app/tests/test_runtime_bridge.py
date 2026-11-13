"""Only ephemeral loopback fixtures: no runtime/model, service or external web."""
import copy
import gc
import http.client
import io
import json
import os
from pathlib import Path
import socket
import threading
import unittest
import urllib.error
import warnings
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch, Mock

from seiya.coordinator import Coordinator
from seiya.runtime_bridge import BridgeError, RuntimeBridge, validate_bridge
from seiya.server import make_server
from seiya.config import load


HEALTH = {'status': 'ok', 'model': 'pegassus', 'runtime_bridge': {'version': 1, 'model_binding': 'hex-utf8-v1'}, 'runtime_status': {
    'version': 1, 'model_residency': 'unknown', 'admission': 'may_attempt',
    'endpoint_observation': 'not_checked', 'memory_admission': 'not_integrated'}}
REPLY = {'model': 'pegassus', 'choices': [{'message': {'role': 'assistant', 'content': 'fixture'},
                                        'finish_reason': 'stop'}]}
BODY = {'messages': [{'role': 'user', 'content': 'fixture'}], 'max_tokens': 32, 'temperature': 0}


class BridgeTests(unittest.TestCase):
    def test_response_and_connection_close_on_every_exit_path(self):
        class Reply:
            def __init__(self, raw, status=200, declared=None):
                self.fp = io.BytesIO(raw)
                self.length = len(raw) if declared is None else declared
                self.status, self.headers, self.closed = status, {}, False
            def read1(self, n):
                value = self.fp.read(n)
                self.length -= len(value)
                return value
            def isclosed(self):
                return self.length == 0
            def getheader(self, name):
                return 'chat-buffered-sse'
            def close(self):
                self.closed = True
                self.fp.close()
        for reply, error in [(Reply(b'{}'), None), (Reply(b'!'), ValueError),
                             (Reply(b'{}', status=503), urllib.error.HTTPError),
                             (Reply(b'{}', declared=65537), BridgeError),
                             (Reply(b'{}', declared=3), http.client.IncompleteRead)]:
            connection = Mock()
            connection.getresponse.return_value = reply
            with patch('seiya.runtime_bridge.http.client.HTTPConnection', return_value=connection):
                if error:
                    with self.assertRaises(error) as caught:
                        self.bridge._request('/health')
                    if isinstance(caught.exception, urllib.error.HTTPError):
                        caught.exception.close()
                else:
                    self.bridge._request('/health')
            self.assertTrue(reply.closed)
            self.assertTrue(reply.fp.closed)
            connection.close.assert_called_once()

    def test_coded_http_failure_does_not_leave_socket_for_garbage_collection(self):
        gc.collect()
        with warnings.catch_warnings(record=True) as observed:
            warnings.simplefilter('always', ResourceWarning)
            self.reply, self.code = {'error': {'code': 'runtime_unavailable', 'message': 'synthetic error'}}, 503
            try:
                self.bridge.complete(BODY, 1)
            except BridgeError:
                pass
            else:
                self.fail('expected runtime failure')
            gc.collect()
        self.assertEqual([str(w.message) for w in observed if issubclass(w.category, ResourceWarning)], [])

    def setUp(self):
        self.calls = []
        self.health = copy.deepcopy(HEALTH)
        self.reply, self.code, self.header = copy.deepcopy(REPLY), 200, 'chat-buffered-sse'
        self.disconnect = False
        self.declared_extra = 0
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                owner.calls.append((self.path, dict(self.headers), None))
                self.send(owner.health)

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                owner.calls.append((self.path, dict(self.headers), body))
                if owner.disconnect:
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.connection.close()
                    return
                self.send(owner.reply, owner.code)

            def send(self, body, code=200):
                raw = json.dumps(body).encode()
                self.send_response(code)
                self.send_header('Content-Length', str(len(raw) + (owner.declared_extra if self.command == 'POST' else 0)))
                self.send_header('X-Seiya-Experimental', owner.header)
                self.send_header('Location', '/must-not-follow')
                self.end_headers()
                self.wfile.write(raw)

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': .01}, daemon=True)
        self.thread.start()
        self.addCleanup(self.close, self.server, self.thread)
        self.config = {'mode': 'fast', 'worker': 'fixture', 'timeout_seconds': 1, 'max_output_tokens': 32,
                       'models': [{'id': 'fixture', 'model': 'synthetic/ñ', 'backend': 'mlx',
                                   'base_url': 'http://127.0.0.1:65535/v1'}],
                       'runtime_bridge': {'base_url': f'http://127.0.0.1:{self.server.server_port}/v1'}}
        self.bridge = RuntimeBridge(self.config['runtime_bridge'], 'synthetic/ñ')

    def close(self, server, thread):
        server.shutdown()
        server.server_close()
        thread.join(2)

    def test_closed_config_and_environment_credentials(self):
        for config in [None, {}, {'base_url': None}, {'base_url': []},
                       {'base_url': 'http://localhost:1234/v1'}, {'base_url': 'https://127.0.0.1:1234/v1'},
                       {'base_url': 'http://127.0.0.1:1234/v1/'}, {'base_url': 'http://127.0.0.1:1234/v1#x'},
                       {**self.config['runtime_bridge'], 'api_key': 'must-not-store'},
                       {**self.config['runtime_bridge'], 'api_key_env': 'BAD\nNAME'}]:
            with self.subTest(config=config), self.assertRaises(ValueError):
                validate_bridge(config)
        cfg = {**self.config['runtime_bridge'], 'api_key_env': 'SEIYA_FIXTURE_KEY'}
        with patch.dict(os.environ, {'SEIYA_FIXTURE_KEY': ''}), self.assertRaises(ValueError):
            RuntimeBridge(cfg, 'synthetic')
        with patch.dict(os.environ, {'SEIYA_FIXTURE_KEY': 'fixture-only-secret', 'http_proxy': 'http://127.0.0.1:1'}):
            RuntimeBridge(cfg, 'synthetic').health()
        self.assertEqual(self.calls[0][1]['Authorization'], 'Bearer fixture-only-secret')

    def test_versioned_config_example_and_config_loader(self):
        path = Path(__file__).resolve().parents[2] / 'contracts/openai-compat/v0.1.0/runtime-bridge.json'
        fixture = json.loads(path.read_text())
        config = fixture['python_config']
        with patch('seiya.config.Path.read_text', return_value=json.dumps(config)):
            self.assertEqual(load('synthetic-config')['runtime_bridge'], config['runtime_bridge'])
        bridge = RuntimeBridge({'base_url': config['runtime_bridge']['base_url']}, config['models'][0]['model'])
        self.assertEqual(bridge.binding, fixture['backend_model_header']['value'])
        for changed in [{**config, 'mode': 'consensus'}, {**config, 'models': [{**config['models'][0], 'backend': 'ollama'}]}]:
            with patch('seiya.config.Path.read_text', return_value=json.dumps(changed)), self.assertRaises(ValueError):
                load('synthetic-config')

    def test_health_only_closed_metadata_and_never_inference(self):
        self.health['runtime_status']['private'] = 'do-not-forward'
        status = self.bridge.health()
        self.assertNotIn('private', status)
        self.assertEqual(status['memory_admission'], 'not_integrated')
        self.assertEqual([c[0] for c in self.calls], ['/health'])
        for mutation in [{'version': 3}, {'version': True}, {'model_residency': 'loaded'},
                         {'endpoint_observation': '<script>'}, {'memory_admission': {}}, {'admission': 'ready'}]:
            self.health = copy.deepcopy(HEALTH)
            self.health['runtime_status'].update(mutation)
            with self.subTest(mutation=mutation), self.assertRaises(BridgeError):
                self.bridge.health()
        self.health = copy.deepcopy(HEALTH)
        self.health['runtime_status'].update(version=2, memory_admission={
            'version': 1, 'scope': 'stop_only_pressure', 'state': 'stopped', 'private': 'hidden'})
        self.assertEqual(self.bridge.health()['memory_admission'], {'scope': 'stop_only_pressure', 'state': 'stopped'})

    def test_coordinator_forwards_tools_without_direct_runtime_or_execution(self):
        coordinator = Coordinator(self.config, transport=lambda *a: self.fail('direct runtime used'))
        tools = [{'type': 'function', 'function': {'name': 'read_fixture', 'parameters': {'type': 'object'}}}]
        call = {'id': 'fixture-call', 'type': 'function', 'function': {'name': 'read_fixture', 'arguments': '{}'}}
        self.reply['choices'][0]['message'] = {'role': 'assistant', 'content': None, 'tool_calls': [call]}
        answer = coordinator.complete({**BODY, 'tools': tools, 'swarm_mode': 'fast'})
        self.assertEqual(answer['choices'][0]['message']['tool_calls'], [call])
        forwarded = self.calls[-1]
        self.assertEqual(forwarded[0], '/v1/chat/completions')
        self.assertEqual(forwarded[1]['X-Seiya-Model-Binding'], 'synthetic/ñ'.encode().hex())
        self.assertEqual(forwarded[2]['tools'], tools)
        self.assertEqual(forwarded[2]['model'], 'pegassus')
        self.assertNotIn('chat_template_kwargs', forwarded[2])
        with self.assertRaises(ValueError):
            coordinator.complete({**BODY, 'swarm_mode': 'consensus'})

    def test_runtime_errors_survive_without_retry(self):
        for code in ('runtime_unavailable', 'runtime_state_unconfirmed', 'runtime_memory_blocked', 'memory_observation_unavailable'):
            self.bridge = RuntimeBridge(self.config['runtime_bridge'], 'synthetic/ñ')
            self.reply, self.code = {'error': {'code': code, 'message': 'synthetic error'}}, 503
            with self.subTest(code=code), self.assertRaises(BridgeError) as caught:
                self.bridge.complete(BODY, 1)
            self.assertEqual(caught.exception.code, code)
            if code != 'memory_observation_unavailable':
                with self.assertRaises(BridgeError) as blocked:
                    self.bridge.complete(BODY, 1)
                self.assertEqual(blocked.exception.code, code)
        self.assertEqual(len([c for c in self.calls if c[0] != '/health']), 4)

    def test_disconnect_blocks_even_after_health_recovers(self):
        self.disconnect = True
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.disconnect = False
        self.assertEqual(self.bridge.health()['admission'], 'blocked')
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertEqual(len([c for c in self.calls if c[0] != '/health']), 1)

    def test_redirect_is_not_followed_and_blocks_reentry(self):
        self.code = 302
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertEqual([c[0] for c in self.calls], ['/health', '/v1/chat/completions'])

    def test_incompatible_or_blocked_health_never_posts(self):
        self.header = 'discovery-only'
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.header = 'chat-buffered-sse'
        self.health['runtime_status']['admission'] = 'closed'
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertEqual([c[0] for c in self.calls], ['/health', '/health'])

    def test_response_limit_and_malformed_json_shapes(self):
        self.reply = {'model': 'pegassus', 'large': 'x' * 200}
        with self.assertRaises(BridgeError):
            self.bridge._request('/v1/chat/completions', BODY, limit=64)
        self.reply = []
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertTrue(self.bridge.failed)

    def test_local_input_limit_does_not_poison_or_dispatch(self):
        with self.assertRaises(BridgeError) as error:
            self.bridge.complete({**BODY, 'messages': [{'role': 'user', 'content': 'x' * (256 * 1024)}]}, 1)
        self.assertEqual(error.exception.code, 'bridge_request_rejected')
        self.assertFalse(self.bridge.failed)
        self.assertEqual([c[0] for c in self.calls], ['/health'])

    def test_premature_eof_is_uncertain_even_when_partial_body_is_valid_json(self):
        self.declared_extra = 10
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertTrue(self.bridge.failed)
        with self.assertRaises(BridgeError):
            self.bridge.complete(BODY, 1)
        self.assertEqual([c[0] for c in self.calls], ['/health', '/v1/chat/completions'])

    def test_frontend_health_and_coded_error_do_not_change_default_profile(self):
        coordinator = Coordinator(self.config)
        frontend = make_server(coordinator, '127.0.0.1', 0, 'fixture-key')
        thread = threading.Thread(target=frontend.serve_forever, kwargs={'poll_interval': .01}, daemon=True)
        thread.start()
        self.addCleanup(self.close, frontend, thread)
        def request(method, path, body=None):
            conn = http.client.HTTPConnection('127.0.0.1', frontend.server_port, timeout=3)
            try:
                conn.request(method, path, json.dumps(body) if body else None,
                             {'Authorization': 'Bearer fixture-key', 'Content-Type': 'application/json'})
                response = conn.getresponse()
                return response.status, json.loads(response.read())
            finally:
                conn.close()
        self.assertEqual(request('GET', '/health')[1]['runtime_status']['model_residency'], 'unknown')
        self.reply, self.code = {'error': {'code': 'runtime_unavailable', 'message': 'synthetic offline'}}, 503
        status, body = request('POST', '/v1/chat/completions', BODY)
        self.assertEqual((status, body['error']['code']), (503, 'runtime_unavailable'))
        coordinator.bridge = None
        self.assertNotIn('runtime_status', request('GET', '/health')[1])
