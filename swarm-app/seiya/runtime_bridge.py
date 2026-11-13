"""Opt-in adapter to an already configured Rust worker. No runtime launch/tools."""
import json
import http.client
import io
import os
import re
import threading
import time
import urllib.error
import urllib.parse

from .coordinator import BackendError


class BridgeError(BackendError):
    def __init__(self, message, code='bridge_unavailable', status=503):
        super().__init__(message)
        self.code, self.status = code, status


def validate_bridge(config):
    if not isinstance(config, dict) or set(config) - {'base_url', 'api_key_env'}:
        raise ValueError('invalid runtime_bridge configuration')
    url = config.get('base_url')
    if not isinstance(url, str):
        raise ValueError('runtime_bridge base_url must be a string')
    try:
        parsed = urllib.parse.urlsplit(url)
        valid = (isinstance(url, str) and parsed.port and
                 url == f'http://127.0.0.1:{parsed.port}/v1' and 1 <= parsed.port <= 65535)
    except (ValueError, TypeError):
        valid = False
    if not valid:
        raise ValueError('runtime_bridge requires canonical HTTP 127.0.0.1 with port and /v1')
    name = config.get('api_key_env')
    if name is not None and (not isinstance(name, str) or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', name)):
        raise ValueError('invalid runtime_bridge API key environment name')


class RuntimeBridge:
    def __init__(self, config, model):
        validate_bridge(config)
        if not isinstance(model, str) or not 1 <= len(model.encode()) <= 4096:
            raise ValueError('invalid bridge backend model binding')
        self.base = config['base_url']
        self.port = urllib.parse.urlsplit(self.base).port
        self.binding = model.encode().hex()
        self.authorization = None
        if config.get('api_key_env'):
            key = os.environ.get(config['api_key_env'], '')
            if not key or len(key) > 4096 or not all(33 <= ord(c) <= 126 for c in key):
                raise ValueError('bridge API key environment variable unavailable or invalid')
            self.authorization = 'Bearer ' + key
        self.lock = threading.Lock()
        self.failed = False
        self.failure_code = 'bridge_unavailable'

    def _request(self, path, body=None, timeout=3, limit=65536):
        headers = {'Content-Type': 'application/json'}
        if self.authorization:
            headers['Authorization'] = self.authorization
        data = None
        if body is not None:
            data = json.dumps(body, ensure_ascii=False).encode()
            if len(data) > 256 * 1024:
                raise BridgeError('bridge request exceeds input limit', 'bridge_request_rejected', 400)
            headers['X-Seiya-Model-Binding'] = self.binding
        # Direct numeric loopback: no proxy lookup, DNS or redirect following.
        deadline = time.monotonic() + timeout
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=timeout)
        response = None
        try:
            connection.request('POST' if body is not None else 'GET', path, body=data, headers=headers)
            sock = connection.sock  # Keep socket reference even for HTTP/1.0 close.
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('bridge deadline')
            sock.settimeout(remaining)
            response = connection.getresponse()
            if response.length is not None and response.length > limit:
                raise BridgeError('bridge response exceeds limit', 'bridge_protocol_error')
            raw = bytearray()
            while len(raw) <= limit:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError('bridge deadline')
                sock.settimeout(remaining)
                chunk = response.read1(min(65536, limit + 1 - len(raw)))
                if not chunk:
                    break
                raw.extend(chunk)
                if response.isclosed():
                    break
            if len(raw) > limit:
                raise BridgeError('bridge response exceeds limit', 'bridge_protocol_error')
            if response.length not in (None, 0):
                raise http.client.IncompleteRead(bytes(raw), response.length)
            if response.status != 200:
                raise urllib.error.HTTPError(self.base[:-3] + path, response.status,
                                             'bridge rejected request', response.headers, io.BytesIO(raw))
            return json.loads(raw), response.getheader('X-Seiya-Experimental')
        finally:
            # HTTPConnection may relinquish a close-delimited response socket.
            # Its response still owns the makefile: close it deterministically,
            # including header/status/size/decoding failures, not at a later GC.
            try:
                if response is not None:
                    response.close()
            finally:
                connection.close()

    def health(self):
        try:
            body, header = self._request('/health')
            status = body['runtime_status']
            if (not isinstance(body, dict) or not isinstance(status, dict)
                    or body.get('runtime_bridge') != {'version': 1, 'model_binding': 'hex-utf8-v1'}
                    or type(body['runtime_bridge']['version']) is not int
                    or header != 'chat-buffered-sse' or body.get('model') != 'pegassus'
                    or type(status.get('version')) is not int or status['version'] not in (1, 2)
                    or status.get('model_residency') != 'unknown'
                    or status.get('admission') not in ('may_attempt', 'blocked', 'closed')
                    or status.get('endpoint_observation') not in ('not_checked', 'request_in_flight',
                        'last_request_completed', 'connection_not_established', 'unconfirmed')):
                raise ValueError('unsupported health')
            # Copy only closed metadata; never pass arbitrary backend HTML/data.
            result = {k: status[k] for k in ('version', 'model_residency', 'admission', 'endpoint_observation')}
            memory = status.get('memory_admission')
            if memory == 'not_integrated':
                result['memory_admission'] = memory
            elif (isinstance(memory, dict) and type(memory.get('version')) is int and memory['version'] == 1
                    and memory.get('scope') == 'stop_only_pressure'
                    and memory.get('state') in ('awaiting_observation', 'monitoring', 'stopped')):
                result['memory_admission'] = {'scope': 'stop_only_pressure', 'state': memory['state']}
            else:
                raise ValueError('unsupported memory metadata')
            with self.lock:
                if self.failed:
                    result['admission'] = 'blocked'
                    result['bridge_error'] = self.failure_code
            return result
        except (OSError, http.client.HTTPException, ValueError, KeyError, TypeError, BridgeError):
            raise BridgeError('Rust bridge unavailable or incompatible; backend state not verified') from None

    def complete(self, backend_body, timeout):
        with self.lock:
            if self.failed:
                raise BridgeError('Rust bridge admission is blocked; explicit recovery required', self.failure_code)
        status = self.health()  # Discovery only; not load/admission authorization.
        if status['admission'] != 'may_attempt':
            raise BridgeError('Rust admission is blocked or closed; explicit recovery required')
        body = {k: backend_body[k] for k in ('messages', 'max_tokens', 'temperature', 'tools') if k in backend_body}
        body.update(model='pegassus', stream=False, swarm_mode='fast')
        try:
            result, header = self._request('/v1/chat/completions', body, timeout + 125, 8 * 1024 * 1024)
            if not isinstance(result, dict) or header != 'chat-buffered-sse' or result.get('model') != 'pegassus':
                raise ValueError('unsupported response')
            return result
        except urllib.error.HTTPError as error:
            try:
                raw = error.read(4097)
                if len(raw) > 4096:
                    raise ValueError('too large')
                failure = json.loads(raw)['error']
                if (not isinstance(failure, dict) or error.code not in (400, 429, 502, 503) or failure.get('code') not in
                        ('runtime_unavailable', 'runtime_state_unconfirmed', 'runtime_memory_blocked', 'memory_observation_unavailable')
                        or not isinstance(failure.get('message'), str) or len(failure['message']) > 512):
                    raise ValueError('unsupported error')
                if failure['code'] != 'memory_observation_unavailable':
                    with self.lock:
                        self.failed = True
                        self.failure_code = failure['code']
                raise BridgeError(failure['message'], failure['code'], error.code)
            except (OSError, http.client.HTTPException, ValueError, KeyError, TypeError):
                with self.lock:
                    self.failed = True
                raise BridgeError('Rust bridge rejected the request; check the experimental profile', 'bridge_protocol_error') from None
            finally:
                error.close()
        except BridgeError as error:
            if error.code == 'bridge_request_rejected':
                raise
            with self.lock:
                self.failed = True
            raise BridgeError('Rust bridge exchange failed; backend completion is unconfirmed') from None
        except (OSError, http.client.HTTPException, ValueError, TypeError):
            with self.lock:
                self.failed = True
            raise BridgeError('Rust bridge exchange failed; backend completion is unconfirmed') from None
