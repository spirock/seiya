"""Explicit Python→Rust→synthetic runtime rehearsal. Never loads models.

Usage: python3 tests/bridge_smoke.py ABSOLUTE_RUST_BINARY NEW_ABSOLUTE_OUTPUT
"""
import http.client
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from seiya.coordinator import Coordinator
from seiya.runtime_bridge import BridgeError, RuntimeBridge
from seiya.server import make_server


def main():
    executable, output = map(Path, sys.argv[1:])
    assert executable.is_absolute() and executable.name == 'seiya-server' and executable.is_file()
    assert output.is_absolute() and not output.exists()
    output.mkdir(mode=0o700)
    calls = []
    state = {'tools': False, 'disconnect': False}

    class Runtime(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            calls.append(request)
            if state['disconnect']:
                self.close_connection = True
                return
            message = {'role': 'assistant', 'content': 'fixture answer'}
            if state['tools']:
                message = {'role': 'assistant', 'content': None, 'tool_calls': [
                    {'id': 'fixture-call', 'type': 'function',
                     'function': {'name': 'read_fixture', 'arguments': '{}'}}]}
            raw = json.dumps({'choices': [{'message': message, 'finish_reason': 'stop'}]}).encode()
            self.send_response(200)
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    runtime = ThreadingHTTPServer(('127.0.0.1', 0), Runtime)
    assert runtime.server_port not in (8010, 8012, 8081)
    thread = threading.Thread(target=runtime.serve_forever, kwargs={'poll_interval': .01}, daemon=True)
    thread.start()
    process = frontend = None
    frontend_thread = None
    try:
        cfg = {'listen': '127.0.0.1:0', 'api_key_env': 'SEIYA_BRIDGE_FIXTURE_KEY', 'runtime_status': True,
               'runtime': {'id': 'fixture', 'model': 'synthetic', 'backend': 'mlx',
                           'base_url': f'http://127.0.0.1:{runtime.server_port}/v1',
                           'timeout_seconds': 1, 'max_output_tokens': 32}}
        path = output / 'rust-config.json'
        path.write_text(json.dumps(cfg))
        path.chmod(0o600)
        with (output / 'rust-stderr.log').open('w') as log:
            process = subprocess.Popen([str(executable), 'serve', '--config', str(path)],
                                       stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=log,
                                       env={'SEIYA_BRIDGE_FIXTURE_KEY': 'fixture-not-real-key', 'LC_ALL': 'C'})
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                if not selector.select(5):
                    raise AssertionError('Rust fixture did not start')
                line = process.stdout.readline(1024).decode()
            match = re.search(r'http://127\.0\.0\.1:(\d+)', line)
            assert match, line
            port = int(match[1])
            assert port not in (8010, 8012, 8081, runtime.server_port)
            os.environ['SEIYA_BRIDGE_FIXTURE_KEY'] = 'fixture-not-real-key'
            bridge_config = {'base_url': f'http://127.0.0.1:{port}/v1', 'api_key_env': 'SEIYA_BRIDGE_FIXTURE_KEY'}
            config = {'mode': 'fast', 'worker': 'fixture', 'timeout_seconds': 1, 'max_output_tokens': 32,
                      'runtime_bridge': bridge_config,
                      'models': [{'id': 'fixture', 'model': 'synthetic', 'backend': 'mlx',
                                  'base_url': f'http://127.0.0.1:{runtime.server_port}/v1'}]}
            coordinator = Coordinator(config, transport=lambda *args: (_ for _ in ()).throw(AssertionError('direct runtime')))
            assert coordinator.bridge.health()['endpoint_observation'] == 'not_checked'
            assert not calls
            frontend = make_server(coordinator, '127.0.0.1', 0, 'fixture-frontend-key')
            frontend_thread = threading.Thread(target=frontend.serve_forever, kwargs={'poll_interval': .01}, daemon=True)
            frontend_thread.start()

            def exchange(path, body=None):
                connection = http.client.HTTPConnection('127.0.0.1', frontend.server_port, timeout=5)
                try:
                    connection.request('POST' if body else 'GET', path, json.dumps(body) if body else None,
                                       {'Authorization': 'Bearer fixture-frontend-key', 'Content-Type': 'application/json'})
                    response = connection.getresponse()
                    return response.status, json.loads(response.read())
                finally:
                    connection.close()

            body = {'messages': [{'role': 'user', 'content': 'fixture only'}], 'max_tokens': 32}
            status, result = exchange('/api/chat', body)
            assert status == 200 and result['choices'][0]['message']['content'] == 'fixture answer', result
            state['tools'] = True
            tool = {'type': 'function', 'function': {'name': 'read_fixture', 'parameters': {'type': 'object'}}}
            status, result = exchange('/v1/chat/completions', {**body, 'tools': [tool]})
            assert status == 200 and result['choices'][0]['message']['tool_calls'][0]['id'] == 'fixture-call', result
            assert calls[-1]['tools'] == [tool]
            state['tools'] = False
            count = len(calls)
            try:
                RuntimeBridge(bridge_config, 'wrong-model').complete(body, 1)
                raise AssertionError('model mismatch accepted')
            except BridgeError:
                pass
            assert len(calls) == count
            state['disconnect'] = True
            status, failure = exchange('/v1/chat/completions', body)
            assert status == 502 and failure['error']['code'] == 'runtime_state_unconfirmed', failure
            assert len(calls) == count + 1
            status, health = exchange('/health')
            assert status == 200 and health['runtime_status']['admission'] == 'blocked'
            exchange('/v1/chat/completions', body)
            assert len(calls) == count + 1
            report = {'mode': 'python-rust-synthetic-runtime', 'model_inference': False,
                      'text_and_tools_preserved': True, 'model_mismatch_blocked_before_dispatch': True,
                      'uncertain_disconnect_blocks_reentry': True, 'runtime_calls': len(calls),
                      'health': health['runtime_status'], 'tools_executed': 0}
            (output / 'report.json').write_text(json.dumps(report, indent=2))
    finally:
        if frontend:
            frontend.shutdown()
            frontend.server_close()
            frontend_thread.join(2)
        if process:
            process.terminate()  # Original owned Popen only, never service lookup.
            try:
                code = process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
                raise AssertionError('owned fixture failed to exit on SIGTERM')
            process.stdout.close()
            # Ambiguous fixture runtime deliberately makes clean drain fail.
            assert code == 1, code
        runtime.shutdown()
        runtime.server_close()
        thread.join(2)
    print('Python→Rust→synthetic runtime OK; no model, no operational service changed')


if __name__ == '__main__':
    main()
