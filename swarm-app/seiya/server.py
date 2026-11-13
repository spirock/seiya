import hmac
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from .coordinator import BackendError, BusyError
from .web import browser_chat
from .runtime_bridge import BridgeError


def validate(body):
    if not isinstance(body, dict):
        raise ValueError('request must be an object')
    if body.get('model', 'pegassus') != 'pegassus':
        raise ValueError('the exposed model ID is pegassus')
    messages = body.get('messages')
    if not isinstance(messages, list) or not messages or len(messages) > 100:
        raise ValueError('messages must contain 1–100 messages')
    for message in messages:
        if not isinstance(message, dict) or message.get('role') not in ('system', 'user', 'assistant', 'tool'):
            raise ValueError('unsupported message role')
        if message.get('content') is not None and not isinstance(message['content'], str):
            raise ValueError('text-only messages are supported')
        if message['role'] == 'tool' and not isinstance(message.get('tool_call_id'), str):
            raise ValueError('tool messages require tool_call_id')
    if type(body.get('max_tokens', 768)) is not int or not 1 <= body.get('max_tokens', 768) <= 4096:
        raise ValueError('max_tokens must be an integer between 1 and 4096')
    if type(body.get('temperature', 0)) not in (int, float) or not 0 <= body.get('temperature', 0) <= 2:
        raise ValueError('temperature must be 0–2')
    if type(body.get('stream', False)) is not bool:
        raise ValueError('stream must be boolean')
    if body.get('swarm_mode', 'consensus') not in ('fast', 'consensus'):
        raise ValueError('swarm_mode must be fast or consensus')
    for key in ('tool_choice', 'response_format', 'logprobs', 'parallel_tool_calls'):
        if key in body:
            raise ValueError(f'{key} is not supported in this version')
    tools = body.get('tools', [])
    if not isinstance(tools, list) or len(tools) > 64:
        raise ValueError('tools must be a list of at most 64 function tools')
    names = set()
    for tool in tools:
        if not isinstance(tool, dict) or tool.get('type') != 'function':
            raise ValueError('only function tools are supported')
        fn = tool.get('function', {})
        if not isinstance(fn, dict) or not isinstance(fn.get('name'), str) or not fn['name']:
            raise ValueError('tool requires function.name')
        if fn['name'] in names or not isinstance(fn.get('parameters', {}), dict):
            raise ValueError('tool names must be unique; parameters must be an object')
        names.add(fn['name'])


def ui_page(preview=False, preview_key=False):
    data = Path(__file__).with_name('ui.html').read_bytes()
    if preview:
        # A demonstration must not read/change the real chat's localStorage.
        data = data.replace(b"const storageKey='seiya.chats.v1';",
                            b"const storageKey='seiya.preview.chats.v1';")
        data = data.replace(b'<section id="scroll"',
            '<p role="status" style="padding:12px;background:#473b18">'
            'VISTA PREVIA SIMULADA · Sin Gemma/Qwen conectados. '
            'Los trabajos usan un ejecutor simulado. No se ejecuta código.'
            '</p><section id="scroll"'.encode('utf-8'))
        if preview_key:
            data = data.replace(b'<script>', b"<script>\ndocument.getElementById('key').value='preview-local';\n", 1)
    return data


class Server(ThreadingHTTPServer):
    daemon_threads = True


def make_server(coordinator, host, port, api_key=None, reader_profile=None, jobs_profile=None):
    if jobs_profile is None and 'jobs' in coordinator.config:
        from .jobs_profile import BrowserJobs
        jobs_profile = BrowserJobs(coordinator.config['jobs'])
    if jobs_profile and (not api_key or not all(33 <= ord(c) <= 126 for c in api_key)):
        raise ValueError('jobs requires authenticated frontend')
    if jobs_profile and (reader_profile or 'web_reader' in coordinator.config or coordinator.config.get('mode') != 'fast' or len(coordinator.config['models']) != 1):
        raise ValueError('jobs requires fast mode, one foreground worker and no reader')
    if reader_profile is None and 'web_reader' in coordinator.config:
        from .reader_profile import BrowserReader
        reader_profile = BrowserReader(coordinator.config['web_reader'])
    if reader_profile and (not api_key or not all(33 <= ord(c) <= 126 for c in api_key)):
        raise ValueError('web_reader requires an authenticated frontend')
    if reader_profile and (coordinator.config.get('mode') != 'fast' or len(coordinator.config['models']) != 1):
        raise ValueError('web_reader requires fast mode and one worker')
    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'

        def setup(self):
            super().setup()
            self.connection.settimeout(30)

        def log_message(self, fmt, *args):
            # Do not log prompts, tool arguments, credentials or model outputs.
            pass

        def json(self, status, data):
            encoded = json.dumps(data, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json; charset=utf-8')
            self.send_header('Content-Length', str(len(encoded)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(encoded)

        def authorized(self):
            # Restrict Host/Origin as well as bind address to resist browser
            # cross-origin requests and DNS rebinding against local tooling.
            host_header = self.headers.get('Host', '')
            if any(len(self.headers.get_all(k, [])) > 1 for k in ('Host', 'Origin', 'Authorization')):
                self.json(403, {'error': {'message': 'ambiguous request guards'}})
                return False
            allowed_hosts = {f'127.0.0.1:{self.server.server_port}',
                             f'localhost:{self.server.server_port}'}
            if host_header not in allowed_hosts:
                self.json(403, {'error': {'message': 'untrusted Host'}})
                return False
            origin = self.headers.get('Origin')
            if origin and origin not in {f'http://{h}' for h in allowed_hosts}:
                self.json(403, {'error': {'message': 'cross-origin requests are disabled'}})
                return False
            if api_key and self.path != '/':
                if not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + api_key):
                    self.json(401, {'error': {'message': 'API key required'}})
                    return False
            return True

        def do_GET(self):
            if not self.authorized():
                return
            if self.path == '/v1/models':
                self.json(200, {'object': 'list', 'data': [
                    {'id': 'pegassus', 'object': 'model', 'owned_by': 'seiya'}]})
            elif self.path == '/health':
                body = {'status': 'ok', 'model': 'pegassus',
                                'busy': coordinator.lock.locked(),
                                 'backend_readiness': 'not checked', 'version': '0.1.0'}
                if getattr(coordinator, 'preview', False):
                    body['preview'] = {'simulated': True, 'model_inference': False,
                                       'background_jobs': 'simulated' if jobs_profile else 'in_development'}
                if jobs_profile:
                    body['jobs'] = {'enabled': True, 'drafts_only': True, 'volatile': True}
                bridge = getattr(coordinator, 'bridge', None)
                if reader_profile:
                    body['web_reader'] = {'version': 1, 'enabled': True,
                        'context_estimate_only': True, 'cancel': 'explicit', 'quality': 'not_validated'}
                if bridge:
                    try:
                        body['runtime_status'] = bridge.health()
                    except BridgeError:
                        body['runtime_status'] = {'version': 1, 'endpoint_observation': 'unconfirmed',
                            'admission': 'blocked', 'model_residency': 'unknown',
                            'memory_admission': 'not_integrated', 'bridge_error': 'bridge_unavailable'}
                self.json(200, body)
            elif self.path == '/':
                data = ui_page(getattr(coordinator, 'preview', False), api_key == 'preview-local')
                self.send_response(200)
                self.send_header('Content-Type', 'text/html; charset=utf-8')
                self.send_header('Content-Length', str(len(data)))
                self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'")
                self.end_headers()
                self.wfile.write(data)
            else:
                self.json(404, {'error': {'message': 'not found'}})

        def do_POST(self):
            if not self.authorized():
                self.close_connection = True
                return
            if jobs_profile and self.path == '/v1/chat/completions':
                self.json(409, {'error': {'message': 'jobs profile is browser-only; use /api/chat', 'code': 'jobs_browser_only'}})
                self.close_connection = True
                return
            browser_control = self.path in ('/api/reader/runs', '/api/reader/status', '/api/reader/cancel')
            job_control = self.path == '/api/jobs' and jobs_profile is not None
            if self.path not in ('/v1/chat/completions', '/api/chat') and not (browser_control and reader_profile) and not job_control:
                self.json(404, {'error': {'message': 'not found'}})
                self.close_connection = True
                return
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if (len(self.headers.get_all('Content-Length', [])) != 1
                        or self.headers.get('Transfer-Encoding') or not 0 < size <= 256 * 1024):
                    raise ValueError('body must be 1–262144 bytes; chunked requests unsupported')
                raw = self.rfile.read(size)
                if len(raw) != size:
                    raise ValueError('request length does not match Content-Length')
                if job_control or (jobs_profile and self.path == '/api/chat') or (reader_profile and (browser_control or self.path == '/api/chat')):
                    from .reader_frame import _object
                    if self.headers.get('Content-Type') != 'application/json' or self.headers.get('Content-Encoding'):
                        raise ValueError('reader browser requests require uncompressed JSON')
                    body = json.loads(raw, object_pairs_hook=_object)
                else:
                    body = json.loads(raw)
                if browser_control or job_control:
                    if self.headers.get('Content-Type') != 'application/json' or self.headers.get('Content-Encoding'):
                        raise ValueError('browser control requires uncompressed JSON')
                else:
                    validate(body)
            except (ValueError, UnicodeError, TimeoutError) as error:
                self.json(400, {'error': {'message': str(error)}})
                self.close_connection = True
                return
            try:
                if job_control:
                    self.json(200, jobs_profile.control(body))
                    return
                if browser_control:
                    if self.path == '/api/reader/runs':
                        if body != {}:
                            raise ValueError('run creation accepts no parameters')
                        self.json(201, reader_profile.create())
                    elif self.path == '/api/reader/status':
                        self.json(200, reader_profile.status(body))
                    else:
                        self.json(202, reader_profile.cancel(body))
                    return
                if self.path == '/api/chat' and jobs_profile:
                    result = jobs_profile.complete(coordinator, body)
                elif self.path == '/api/chat' and reader_profile and body.get('web_search', True):
                    result = reader_profile.complete(coordinator, body)
                elif self.path == '/api/chat':
                    if 'reader_run' in body:
                        raise ValueError('reader is disabled for this request')
                    result = browser_chat(coordinator, body)
                else:
                    if 'reader_run' in body:
                        raise ValueError('reader controls are browser-only')
                    result = coordinator.complete(body)
            except ValueError as error:
                self.json(400, {'error': {'message': str(error)}})
                return
            except BusyError as error:
                self.json(429, {'error': {'message': str(error)}})
                return
            except BridgeError as error:
                self.json(error.status, {'error': {'message': str(error), 'code': error.code}})
                return
            except BackendError as error:
                self.json(409 if str(error).startswith('reader_cancelled:') else 502,
                          {'error': {'message': str(error)}})
                return
            if not body.get('stream'):
                self.json(200, result)
                return
            # Buffered SSE compatibility: preserve structured calls; voting is
            # finished before emitting. Do not pretend this is live generation.
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Connection', 'close')
            self.end_headers()
            choice = result['choices'][0]
            delta = dict(choice['message'])
            for i, call in enumerate(delta.get('tool_calls', [])):
                call['index'] = i
            base = {k: result[k] for k in ('id', 'created', 'model')}
            chunks = [dict(base, object='chat.completion.chunk', choices=[
                {'index': 0, 'delta': delta, 'finish_reason': None}]),
                dict(base, object='chat.completion.chunk', choices=[
                    {'index': 0, 'delta': {}, 'finish_reason': choice['finish_reason']}],
                     usage=result['usage'], swarm=result['swarm'])]
            try:
                for chunk in chunks:
                    self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
                self.wfile.write(b'data: [DONE]\n\n')
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
            self.close_connection = True

    server = Server((host, port), Handler)
    bridge = getattr(coordinator, 'bridge', None)
    if bridge and bridge.port == server.server_port:
        server.server_close()
        raise ValueError('runtime bridge must not point to the frontend listener')
    if reader_profile and reader_profile.client.port == server.server_port:
        server.server_close()
        raise ValueError('reader must not point to the frontend listener')
    if jobs_profile and jobs_profile.port == server.server_port:
        server.server_close()
        raise ValueError('jobs must not point to the frontend listener')
    return server
