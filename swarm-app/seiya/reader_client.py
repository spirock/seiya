"""Explicit local reader client. Never starts services, models, or external fetches."""
import http.client
import json
import os
import re
import threading
import time
import urllib.parse

from .coordinator import BackendError
from .reader_frame import _object, parse_request, validate_response


class ReaderError(BackendError):
    pass


class _DeadlineFile:
    """Bound buffered reads against an absolute deadline, including headers."""
    def __init__(self, fp, sock, deadline, clock):
        self.fp, self.sock, self.deadline, self.clock = fp, sock, deadline, clock
        self.header_bytes = 0

    def read1(self, n):
        remaining = self.deadline - self.clock()
        if remaining <= 0:
            raise TimeoutError('reader deadline')
        self.sock.settimeout(remaining)
        return self.fp.read1(n)

    def readline(self, limit=65537):
        out = bytearray()
        while len(out) < limit:
            byte = self.read1(1)
            self.header_bytes += len(byte)
            if self.header_bytes > 65536:
                raise ReaderError('reader response headers exceed limit')
            out.extend(byte)
            if not byte or byte == b'\n':
                break
        return bytes(out)

    def close(self):
        self.fp.close()

    def flush(self):
        self.fp.flush()


class ReaderClient:
    def __init__(self, config, clock=time.monotonic):
        if type(config) is not dict or set(config) != {'base_url', 'api_key_env'}:
            raise ValueError('invalid reader client configuration')
        url, name = config['base_url'], config['api_key_env']
        try:
            port = urllib.parse.urlsplit(url).port
            valid = 1 <= port <= 65535 and url == f'http://127.0.0.1:{port}/internal/reader/v0.1'
        except (ValueError, TypeError, AttributeError):
            valid = False
        if not valid or not isinstance(name, str) or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', name):
            raise ValueError('reader requires canonical loopback endpoint and key environment name')
        key = os.environ.get(name, '')
        if not key or len(key) > 4096 or not all(33 <= ord(c) <= 126 for c in key):
            raise ValueError('reader key unavailable or invalid')
        self.port, self.authorization, self.clock = port, 'Bearer ' + key, clock
        self.lock, self.failed = threading.Lock(), False

    def _post(self, operation, frame, deadline, status):
        deadline = min(deadline, self.clock() + (3 if operation == 'cancel' else 45))
        data = json.dumps(frame, ensure_ascii=False).encode()
        if len(data) > (160 * 1024 if operation == 'select' else 16384):
            raise ReaderError('reader request exceeds frame limit')
        remaining = deadline - self.clock()
        if remaining <= 0:
            raise ReaderError('reader deadline exhausted')
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=remaining)
        clock = self.clock

        class Reply(http.client.HTTPResponse):
            def __init__(self, sock, *args, **kwargs):
                super().__init__(sock, *args, **kwargs)
                self.fp = _DeadlineFile(self.fp, sock, deadline, clock)

        connection.response_class = Reply
        response = None
        try:
            connection.request('POST', '/internal/reader/v0.1/' + operation, body=data,
                               headers={'Authorization': self.authorization, 'Content-Type': 'application/json'})
            sock = connection.sock
            remaining = deadline - self.clock()
            if remaining <= 0:
                raise TimeoutError('deadline')
            sock.settimeout(remaining)
            response = connection.getresponse()
            lengths = [v for k, v in response.getheaders() if k.lower() == 'content-length']
            if (response.status != status or len(lengths) != 1 or not lengths[0].isdigit()
                    or not 1 <= int(lengths[0]) <= 131072 or response.getheader('Transfer-Encoding')
                    or response.getheader('Content-Encoding')
                    or response.getheader('Content-Type', '').lower() not in ('application/json', 'application/json; charset=utf-8')):
                raise ReaderError('reader rejected request or returned incompatible framing')
            raw = bytearray()
            while len(raw) < int(lengths[0]):
                remaining = deadline - self.clock()
                if remaining <= 0:
                    raise TimeoutError('deadline')
                sock.settimeout(remaining)
                chunk = response.read1(min(8192, int(lengths[0]) - len(raw)))
                if not chunk:
                    raise ReaderError('reader response incomplete')
                raw.extend(chunk)
            return bytes(raw)
        except (OSError, http.client.HTTPException, ValueError, ReaderError):
            self.failed = True
            raise ReaderError('reader exchange failed; state unconfirmed, explicit recovery required') from None
        finally:
            if response is not None:
                response.close()
            connection.close()

    def __call__(self, frame, deadline):
        if not self.lock.acquire(blocking=False):
            raise ReaderError('reader request already active')
        try:
            if self.failed:
                raise ReaderError('reader client blocked; explicit recovery required')
            encoded = json.dumps(frame).encode()
            frame = parse_request(encoded)
            remaining_ms = int((deadline - self.clock()) * 1000)
            if remaining_ms <= 0:
                raise ReaderError('reader deadline exhausted')
            frame['remaining_ms'] = min(frame['remaining_ms'], remaining_ms)
            raw = self._post('read', frame, deadline, 200)
            try:
                validate_response(frame, raw)
            except ValueError:
                self.failed = True
                raise ReaderError('reader returned invalid frame; explicit recovery required') from None
            return raw
        finally:
            self.lock.release()

    def cancel(self, run_id, deadline):
        if not isinstance(run_id, str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,128}', run_id):
            raise ValueError('invalid cancellation ID')
        # Cancellation is allowed even after an uncertain read; no retry of read.
        raw = self._post('cancel', {'version': '0.1-draft.1', 'run_id': run_id}, deadline, 202)
        try:
            value = json.loads(raw.decode('utf-8'), object_pairs_hook=_object)
        except (ValueError, UnicodeError):
            self.failed = True
            raise ReaderError('invalid cancellation acknowledgement') from None
        if value != {'status': 'cancellation_requested'}:
            self.failed = True
            raise ReaderError('invalid cancellation acknowledgement')
        return {'status': 'cancellation_requested', 'cleanup_confirmed': False}

    def select(self, frame, raw, ids):
        decoded = validate_response(frame, raw)
        # Failed pages remain visible in flow metadata, but are not sources.
        successful = [i for i in ids if any(p['id'] == i and p['status'] == 'ok' for p in decoded['results'])]
        if not successful:
            return []
        raw = self._post('select', {'request': json.dumps(frame, ensure_ascii=False),
                         'response': raw.decode('utf-8'), 'ids': successful}, self.clock() + 3, 200)
        try:
            value = json.loads(raw.decode('utf-8'), object_pairs_hook=_object)
            if type(value) is not dict or set(value) != {'sources'} or type(value['sources']) is not list:
                raise ValueError('invalid selection')
            return value['sources']
        except (ValueError, UnicodeError):
            self.failed = True
            raise ReaderError('reader selection incompatible') from None
