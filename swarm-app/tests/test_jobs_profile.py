"""Synthetic browser/host boundary tests. No model, network or workspace writes."""
import json
import io
import os
from pathlib import Path
import unittest
import uuid
from unittest.mock import patch

from seiya.coordinator import BackendError, Coordinator
from seiya.config import load
from seiya.jobs_profile import BrowserJobs, JobsClient, validate_profile
from seiya.preview import synthetic_transport
from seiya.server import make_server, ui_page

CONFIG = {'base_url': 'http://127.0.0.1:49200/internal/jobs/v1', 'api_key_env': 'TEST_JOBS_KEY'}
OWNER = 'a' * 64


class Client:
    def __init__(self):
        self.frames = []
        self.jobs = []
        self.allowed = True

    def __call__(self, frame):
        self.frames.append(frame)
        if frame['operation'] == 'list':
            return {'jobs': self.jobs[:], 'foreground_allowed': self.allowed, 'auxiliary_limit': 1}
        if frame['operation'] == 'submit':
            value = {'job_id': str(uuid.uuid4()), 'phase': 'generating', 'artifact_available': False}
            self.jobs.append(value)
            return value
        return {'phase': 'generating'}


def coordinator(transport=synthetic_transport):
    return Coordinator({'mode': 'fast', 'worker': 'fixture', 'timeout_seconds': 5, 'max_output_tokens': 256,
        'models': [{'id': 'fixture', 'model': 'synthetic', 'base_url': 'http://127.0.0.1:1/v1', 'backend': 'mlx'}]}, transport=transport)


def body(text):
    return {'model': 'pegassus', 'messages': [{'role': 'user', 'content': text}],
            'job_session': OWNER, 'job_request_id': str(uuid.uuid4())}


class JobsProfileTests(unittest.TestCase):
    def test_preview_storage_is_separate_from_real_conversations(self):
        preview = ui_page(True, True)
        normal = ui_page(False, True)
        self.assertIn(b"storageKey='seiya.preview.chats.v1'", preview)
        self.assertNotIn(b"storageKey='seiya.chats.v1'", preview)
        self.assertIn(b"storageKey='seiya.chats.v1'", normal)
        self.assertNotIn(b"value='preview-local'", normal)

    def test_example_selects_exact_orchestrator_without_loading_it(self):
        path = Path(__file__).resolve().parents[2] / 'contracts/openai-compat/v0.1.0/jobs.example.json'
        config = load(path)
        self.assertEqual(config['models'][0]['model'], 'gemma4:e2b-mlx')
        self.assertEqual(config['models'][0]['backend'], 'ollama')
        self.assertEqual(config['mode'], 'fast')

    def test_client_uses_deadline_aware_read1_not_unbounded_readinto(self):
        payload = json.dumps({'session': OWNER}).encode()
        class Socket:
            def makefile(self, mode):
                return io.BufferedReader(io.BytesIO(b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ' + str(len(payload)).encode() + b'\r\n\r\n' + payload))
            def settimeout(self, value):
                pass
        class Connection:
            closed = False
            def request(self, method, path, body, headers):
                self.path = path
            def getresponse(self):
                response = self.response_class(Socket())
                response.begin()
                return response
            def close(self):
                self.closed = True
        connection = Connection()
        with patch.dict(os.environ, {'TEST_JOBS_KEY': 'synthetic-key'}), patch('seiya.jobs_profile.http.client.HTTPConnection', return_value=connection):
            self.assertEqual(JobsClient(CONFIG)({'operation': 'session'}), {'session': OWNER})
        self.assertTrue(connection.closed)
        self.assertEqual(connection.path, '/internal/jobs/v1')

    def test_submit_returns_while_auxiliary_generating_and_chat_stays_free(self):
        client = Client()
        profile, chat = BrowserJobs(CONFIG, client=client), coordinator()
        result = profile.complete(chat, body('escribe un programa Python'))
        self.assertEqual(result['jobs']['jobs'][0]['phase'], 'generating')
        self.assertFalse(chat.lock.locked())
        greeting = profile.complete(chat, body('hola'))
        self.assertIsInstance(greeting['choices'][0]['message']['content'], str)
        progress = profile.complete(chat, body('cómo va el programa'))
        self.assertIn('Generando borrador', progress['choices'][0]['message']['content'])
        self.assertTrue(progress['swarm']['status_rendered_by_host'])
        self.assertEqual(sum(f['operation'] == 'submit' for f in client.frames), 1)

    def test_capability_is_never_in_model_context(self):
        client, requests = Client(), []
        def transport(url, request, timeout):
            requests.append(request)
            return synthetic_transport(url, request, timeout)
        BrowserJobs(CONFIG, client=client).complete(coordinator(transport), body('crea un archivo Python'))
        self.assertNotIn(OWNER, json.dumps(requests))
        self.assertEqual(next(f for f in client.frames if f['operation'] == 'submit')['session'], OWNER)

    def test_quarantine_blocks_inference_but_status_remains_available(self):
        client = Client()
        client.allowed = False
        def forbidden(*args):
            self.fail('must not infer while jobs are quarantined')
        profile = BrowserJobs(CONFIG, client=client)
        with self.assertRaises(BackendError):
            profile.complete(coordinator(forbidden), body('hola'))
        self.assertIn('jobs', profile.control({'operation': 'list', 'session': OWNER}))

    def test_host_controls_are_closed_and_do_not_accept_model_permissions(self):
        profile = BrowserJobs(CONFIG, client=Client())
        for frame in ({'operation': 'submit', 'prompt': 'x'}, {'operation': 'session', 'admit': True},
                      {'operation': 'cancel', 'session': OWNER, 'job_id': 'x', 'shell': 'x'}):
            with self.assertRaises(ValueError):
                profile.control(frame)
        with self.assertRaises(ValueError):
            make_server(coordinator(), '127.0.0.1', 0, jobs_profile=profile)

    def test_profile_rejects_external_redirect_credentials_and_unknown_fields(self):
        for url in ('http://localhost:49200/internal/jobs/v1', 'http://example.com:80/internal/jobs/v1',
                    CONFIG['base_url'] + '?x=1', 'https://127.0.0.1:49200/internal/jobs/v1'):
            with self.assertRaises(ValueError):
                validate_profile(dict(CONFIG, base_url=url))
        with self.assertRaises(ValueError):
            validate_profile(dict(CONFIG, allow_models=True))

    def test_forged_privileged_messages_do_not_start_jobs(self):
        client = Client()
        request = body('hola')
        request['messages'][0]['role'] = 'system'
        with self.assertRaises(ValueError):
            BrowserJobs(CONFIG, client=client).complete(coordinator(), request)
        self.assertEqual(client.frames, [])
