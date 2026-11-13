"""Explicit browser-only host adapters; no installation or service startup."""
import json
import re
import secrets
import threading
import time

from .coordinator import BackendError, BusyError
from .reader_client import ReaderClient
from .reader_flow import run
from .web import web_search


def validate_profile(config):
    if (type(config) is not dict or set(config) != {'base_url', 'api_key_env', 'context_tokens', 'output_tokens', 'margin_tokens'}
            or type(config['context_tokens']) is not int or not 1024 <= config['context_tokens'] <= 4096
            or type(config['output_tokens']) is not int or not 32 <= config['output_tokens'] <= 256
            or type(config['margin_tokens']) is not int or not 128 <= config['margin_tokens'] <= 1024):
        raise ValueError('invalid web_reader profile')
    from .runtime_bridge import validate_bridge
    base = config['base_url']
    if not isinstance(base, str) or not base.endswith('/internal/reader/v0.1'):
        raise ValueError('invalid web_reader endpoint')
    validate_bridge({'base_url': base[:-len('/internal/reader/v0.1')] + '/v1', 'api_key_env': config['api_key_env']})
    if not config['api_key_env']:
        raise ValueError('web_reader requires key environment name')


class BrowserReader:
    def __init__(self, config, *, client=None, search=web_search, clock=time.monotonic):
        validate_profile(config)
        self.config, self.clock, self.search = config, clock, search
        self.client = client or ReaderClient({k: config[k] for k in ('base_url', 'api_key_env')})
        self.lock, self.runs = threading.Lock(), {}

    def create(self):
        with self.lock:
            now = self.clock()
            self.runs = {i: v for i, v in self.runs.items() if v['active'] or now - v['created'] < 600}
            if len(self.runs) >= 128:
                raise BusyError('browser run registry full')
            run_id, token = secrets.token_hex(16), secrets.token_hex(32)
            self.runs[run_id] = {'token': token, 'created': now, 'active': False, 'consumed': False,
                                 'cancelled': threading.Event(), 'phase': 'ready', 'reading': False}
            return {'run_id': run_id, 'token': token}

    def _find(self, ref):
        if type(ref) is not dict or set(ref) != {'run_id', 'token'}:
            raise ValueError('invalid browser run reference')
        if not all(isinstance(v, str) and re.fullmatch('[a-f0-9]{32,64}', v) for v in ref.values()):
            raise ValueError('invalid browser run reference')
        item = self.runs.get(ref['run_id'])
        if not item or not secrets.compare_digest(item['token'], ref['token']):
            raise ValueError('unknown browser run')
        if not item['active'] and self.clock() - item['created'] >= 600:
            raise ValueError('browser run expired')
        return item

    def status(self, ref):
        with self.lock:
            item = self._find(ref)
            return {'phase': item['phase'], 'cancel_requested': item['cancelled'].is_set(),
                    'active': item['active'], 'cleanup_confirmed': False}

    def cancel(self, ref):
        with self.lock:
            item = self._find(ref)
            item['cancelled'].set()
            if item['active']:
                item['phase'] = 'cancelling'
            elif not item['consumed']:
                item['phase'] = 'cancelled'
            reading = item['reading']
        if reading:
            self.client.cancel(ref['run_id'], self.clock() + 3)
        return {'status': 'cancellation_requested', 'cleanup_confirmed': False}

    def complete(self, coordinator, body):
        ref = body.get('reader_run')
        with self.lock:
            item = self._find(ref)
            if item['consumed']:
                raise ValueError('browser run already consumed; no retry')
            item['active'], item['consumed'] = True, True

        def check():
            if item['cancelled'].is_set():
                raise BackendError('reader_cancelled: cancellation requested; no late answer accepted')

        def phase(name):
            check()
            with self.lock:
                item['phase'] = name

        def admit(request):
            phase('inference')
            # Deliberately an ESTIMATE, not tokenization or a universal upper bound.
            # Include full history, system, tool schemas/results, output and margin.
            size = len(json.dumps(request, ensure_ascii=False).encode('utf-8'))
            overhead = 64 * (len(request['messages']) + len(request.get('tools', [])))
            return size + overhead + self.config['output_tokens'] + self.config['margin_tokens'] <= self.config['context_tokens']

        def search(query, deadline):
            phase('search')
            value = self.search(query, deadline)
            check()
            return value

        def read(frame, deadline):
            phase('reading')
            with self.lock:
                check()
                item['reading'] = True
            try:
                value = self.client(frame, deadline)
                check()
                return value
            finally:
                with self.lock:
                    item['reading'] = False

        request = {k: v for k, v in body.items() if k != 'reader_run'}
        request['max_tokens'] = min(body.get('max_tokens', self.config['output_tokens']), self.config['output_tokens'])
        succeeded = False
        try:
            check()
            result = run(coordinator, request, search=search, reader=read, select_sources=self.client.select,
                         admit_context=admit, clock=self.clock, run_id=ref['run_id'], check_cancel=check)
            check()
            result['web']['context_budget'] = {'method': 'utf8_json_bytes_plus_overhead_estimate',
                'context_tokens': self.config['context_tokens'], 'output_tokens': self.config['output_tokens'],
                'margin_tokens': self.config['margin_tokens'], 'tokenizer_verified': False}
            succeeded = True
            return result
        finally:
            with self.lock:
                item['active'] = False
                item['phase'] = 'cancelled' if item['cancelled'].is_set() else ('finished' if succeeded else 'failed')
