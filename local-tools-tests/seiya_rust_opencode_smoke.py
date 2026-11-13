#!/usr/bin/env python3
"""Opt-in real smoke. Own processes only; no install, downloads or shared service.

macOS/local MLX and an existing release build required. Results are synthetic
artifacts, retained in --output. This is a manual integration probe, not CI.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import socket
import sqlite3
import subprocess
import threading
import time
import urllib.error
import urllib.request


def inspect_evidence(output, records, command):
    """Fail closed: a changed fixture and a zero CLI exit are not tool evidence."""
    events = [json.loads(line) for line in (output / 'opencode.stdout').read_text().splitlines()]
    tools = [event['part'] for event in events if event['type'] == 'tool_use']
    if any(event['type'] == 'error' for event in events):
        raise RuntimeError('OpenCode emitted an error')
    if not tools or any(tool['state']['status'] != 'completed' for tool in tools):
        raise RuntimeError('missing or failed actual tools')
    reads = {tool['state']['input'].get('path') for tool in tools if tool['tool'] == 'read'}
    if reads != {'calc.py', 'test_calc.py'}:
        raise RuntimeError('expected both actual fixture reads')
    if not any(tool['tool'] in {'edit', 'write', 'patch'} for tool in tools):
        raise RuntimeError('no actual edit')
    shells = [tool for tool in tools if tool['tool'] == 'shell']
    if len(shells) != 1 or shells[0]['state']['input']['command'] != command:
        raise RuntimeError('missing fixed shell test command')
    shell = shells[0]['state']
    if shell['metadata']['metadata'].get('exit') != 0 or 'Ran 3 tests' not in shell['output'] or '\nOK\n' not in shell['output']:
        raise RuntimeError('OpenCode tests did not really pass')
    calls = [call for record in records for call in record.get('calls', [])]
    executed = {tool['id']: tool for tool in tools}
    if len(executed) != len(tools) or len(calls) != len(tools) or {call['id'] for call in calls} != set(executed):
        raise RuntimeError('structured call IDs do not match actual tools')
    for call in calls:
        tool = executed[call['id']]
        if call['function']['name'] != tool['tool'] or json.loads(call['function']['arguments']) != tool['state']['input']:
            raise RuntimeError('tool arguments changed in transit')
        if not any(call['id'] in record['tool_results'] for record in records):
            raise RuntimeError('tool result did not return through HTTP')
    if not records or any(record.get('status') != 200 or not record.get('done') for record in records):
        raise RuntimeError('failed or incomplete SSE request')
    # Inspect only our stopped, isolated database, never the user's service DB.
    db = output / 'home/data/opencode/opencode.db'
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as connection:
        messages = [(kind, json.loads(data)) for kind, data in connection.execute(
            'select type,data from session_message order by seq')]
    if not messages or messages[-1][0] != 'idle' or messages[-1][1].get('outcome') != 'succeeded':
        raise RuntimeError('session did not reach succeeded/idle')
    types = {}
    for kind, _ in messages:
        types[kind] = types.get(kind, 0) + 1
    return {'tools_completed': [tool['tool'] for tool in tools],
            'session_message_types': types, 'idle_outcome': 'succeeded',
            'compaction_messages': sum(n for kind, n in types.items() if 'compact' in kind),
            'request_count': len(records), 'http_errors': 0,
            'max_prompt_tokens': max(record['usage']['prompt_tokens'] for record in records),
            'total_prompt_tokens_including_repeated_history': sum(record['usage']['prompt_tokens'] for record in records),
            'total_completion_tokens': sum(record['usage']['completion_tokens'] for record in records)}


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def request(url, body=None, key=None, timeout=5):
    headers = {'Content-Type': 'application/json'}
    if key:
        headers['Authorization'] = 'Bearer ' + key
    req = urllib.request.Request(url, data=body, headers=headers)
    return urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=timeout)


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    os.chmod(output, 0o700)
    work = output / 'workspace'
    home = output / 'home'
    work.mkdir()
    home.mkdir()
    # No inherited credentials, plugins, service endpoint, or proxy environment.
    env = {'PATH': '/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin',
           'HOME': str(home), 'TMPDIR': str(output), 'LANG': 'en_US.UTF-8',
           'XDG_CONFIG_HOME': str(home / 'config'), 'XDG_DATA_HOME': str(home / 'data'),
           'XDG_CACHE_HOME': str(home / 'cache'), 'XDG_STATE_HOME': str(home / 'state'),
           'PYTHONDONTWRITEBYTECODE': '1', 'HF_HUB_OFFLINE': '1',
           'TRANSFORMERS_OFFLINE': '1', 'HF_HUB_DISABLE_TELEMETRY': '1'}
    (work / 'calc.py').write_text('def add(a, b):\n    return a - b\n')
    # Guard the only editable module before importing it. No arbitrary generated
    # Python is executed: only the exact harmless add/sub function is admitted.
    (work / 'test_calc.py').write_text('''import ast
from pathlib import Path
import unittest

source = Path(__file__).with_name("calc.py").read_text()
tree = ast.dump(ast.parse(source))
allowed = [ast.dump(ast.parse("def add(a, b):\\n    return a " + op + " b\\n")) for op in ["+", "-"]]
if tree not in allowed:
    raise RuntimeError("unsafe fixture edit: refusing to execute generated module")
from calc import add

class TestAdd(unittest.TestCase):
    def test_positive(self): self.assertEqual(add(2, 3), 5)
    def test_zero(self): self.assertEqual(add(7, 0), 7)
    def test_negative(self): self.assertEqual(add(-2, -4), -6)
''')
    test_hash = hashlib.sha256((work / 'test_calc.py').read_bytes()).hexdigest()
    rules = [{'action': '*', 'resource': '*', 'effect': 'deny'}]
    for name in ['calc.py', 'test_calc.py']:
        for path in [name, str(work / name)]:
            rules.append({'action': 'read', 'resource': path, 'effect': 'allow'})
    for path in ['calc.py', str(work / 'calc.py')]:
        rules.append({'action': 'edit', 'resource': path, 'effect': 'allow'})
    command = '/opt/homebrew/bin/python3 -m unittest -v'
    rules.append({'action': 'shell', 'resource': command, 'effect': 'allow'})
    key = os.urandom(32).hex()
    env['SEIYA_SMOKE_KEY'] = key
    processes, files, servers, records, samples = [], [], [], [], []
    lock = threading.Lock()
    stop_sampling = threading.Event()
    summary = {'passed': False, 'model': args.model.name, 'requests': records,
               'memory_method': 'ps RSS KiB sampled every 1s; not MLX GPU peak or system available memory',
               'shared_services_modified': False}
    start = time.monotonic()

    def spawn(command, name, cwd=output):
        stdout = (output / (name + '.stdout')).open('w')
        stderr = (output / (name + '.stderr')).open('w')
        files.extend([stdout, stderr])
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=stdout, stderr=stderr)
        processes.append((name, process))
        return process

    def proxy(target):
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def do_POST(self):
                if self.path != '/v1/chat/completions' or self.headers.get('Authorization') != 'Bearer ' + key:
                    self.send_error(403)
                    return
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= 262144:
                    self.send_error(400)
                    return
                data = self.rfile.read(size)
                body = json.loads(data)
                item = {'input_bytes': len(data), 'message_count': len(body.get('messages', [])),
                        'tools': [t['function']['name'] for t in body.get('tools', [])],
                        'stream': body.get('stream'), 'request_fields': list(body),
                        'tool_results': [m.get('tool_call_id') for m in body.get('messages', []) if m['role'] == 'tool']}
                at = time.monotonic()
                try:
                    try:
                        response = request(target + self.path, data, key, timeout=240)
                    except urllib.error.HTTPError as failure:
                        response = failure
                    payload = response.read(16 * 1024 * 1024)
                    item.update(status=response.code, seconds=round(time.monotonic() - at, 3),
                                content_type=response.headers.get('Content-Type'))
                    text = payload.decode()
                    if text.startswith('data: '):
                        events = [json.loads(s[6:]) for s in text.strip().split('\n\n') if s != 'data: [DONE]']
                        item['usage'] = events[-1].get('usage')
                        item['calls'] = events[0]['choices'][0]['delta'].get('tool_calls', [])
                        item['done'] = text.endswith('data: [DONE]\n\n')
                    else:
                        item['json'] = json.loads(text)
                    self.send_response(response.code)
                    self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
                    self.send_header('Content-Length', str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    response.close()
                except Exception as error:
                    item['proxy_error'] = type(error).__name__
                    self.send_error(502)
                finally:
                    with lock:
                        records.append(item)
                        (output / 'requests.json').write_text(json.dumps(records, indent=2))
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        servers.append(server)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        return f'http://127.0.0.1:{server.server_port}'

    try:
        port = free_port()
        runtime = spawn([str(args.mlx), '--model', str(args.model), '--host', '127.0.0.1', '--port', str(port),
                         '--allowed-origins', f'http://127.0.0.1:{port}', '--decode-concurrency', '1',
                         '--prompt-concurrency', '1', '--prefill-step-size', '256', '--prompt-cache-size', '1',
                         '--prompt-cache-bytes', '128MB', '--max-tokens', '2048',
                         '--chat-template-args', '{"enable_thinking":false}'], 'mlx')
        ready = time.monotonic() + 120
        while True:
            if runtime.poll() is not None:
                raise RuntimeError('own MLX process exited; inspect mlx.stderr')
            try:
                with request(f'http://127.0.0.1:{port}/v1/models') as response:
                    if response.status == 200: break
            except (urllib.error.URLError, TimeoutError):
                if time.monotonic() > ready: raise RuntimeError('MLX startup deadline')
                time.sleep(.2)
        # Verify ownership before configuring a generation endpoint (port race).
        owner = subprocess.check_output(['/usr/sbin/lsof', '-nP', f'-iTCP:{port}', '-sTCP:LISTEN', '-t'], text=True)
        if {int(pid) for pid in owner.split()} != {runtime.pid}:
            raise RuntimeError('runtime listener ownership mismatch')
        cfg = {'listen': '127.0.0.1:0', 'api_key_env': 'SEIYA_SMOKE_KEY',
               'runtime': {'id': 'gemma-smoke', 'backend': 'mlx', 'model': str(args.model),
                           'base_url': f'http://127.0.0.1:{port}/v1', 'timeout_seconds': 180,
                           'max_output_tokens': 2048}}
        (output / 'rust.json').write_text(json.dumps(cfg, indent=2))
        rust_log = (output / 'rust.stdout').open('w+')
        rust_err = (output / 'rust.stderr').open('w')
        files.extend([rust_log, rust_err])
        rust = subprocess.Popen([str(args.rust), 'serve', '--config', str(output / 'rust.json')],
                                env=env, cwd=output, stdout=subprocess.PIPE, stderr=rust_err, text=True)
        processes.append(('rust', rust))
        selector = selectors.DefaultSelector()
        selector.register(rust.stdout, selectors.EVENT_READ)
        if not selector.select(10): raise RuntimeError('Rust startup deadline')
        line = rust.stdout.readline()
        selector.close()
        rust_log.write(line); rust_log.flush()
        address = re.search(r'http://127\.0\.0\.1:\d+', line)
        if not address: raise RuntimeError('Rust listener missing')
        url = proxy(address.group())
        config = {'$schema': 'https://opencode.ai/config.json', 'model': 'seiya/pegassus',
                  'snapshots': False, 'formatter': False, 'plugins': ['-opencode.provider.ollama', '-opencode.provider.lmstudio', '-opencode.provider.vllm'],
                  'providers': {'seiya': {'name': 'Seiya isolated Rust',
                    'package': '@opencode/ai/providers/openai-compatible',
                    'settings': {'baseURL': url + '/v1', 'apiKey': '{env:SEIYA_SMOKE_KEY}', 'encoding': 'none',
                                 'headerTimeout': 240000, 'timeout': 240000},
                    'body': {'swarm_mode': 'fast'},
                    'models': {'pegassus': {'name': 'Pegassus',
                        'capabilities': {'tools': True, 'input': ['text'], 'output': ['text']},
                        'limit': {'context': 32768, 'output': 2048}}}}},
                  'policies': [{'action': 'provider.use', 'resource': '*', 'effect': 'deny'},
                               {'action': 'provider.use', 'resource': 'seiya', 'effect': 'allow'}],
                  'permissions': rules,
                  'agents': {'smoke': {'mode': 'primary', 'steps': 8,
                    'system': 'You are testing a local tool integration. Use actual tools, never invent results. '
                              'Work only on calc.py and test_calc.py in the working directory. '
                              'Only calc.py may be edited. No delegation, network, or unrelated files. '
                              'Use read, edit/patch, shell as needed. Keep replies brief.',
                    'permissions': rules}}}
        (work / 'opencode.json').write_text(json.dumps(config, indent=2))
        # This command is documented to avoid starting a service or opening DBs.
        check = subprocess.run([str(args.opencode), 'debug', 'paths'], cwd=work, env=env,
                               capture_output=True, text=True, timeout=20)
        (output / 'isolated-paths.txt').write_text(check.stdout + check.stderr)
        if check.returncode: raise RuntimeError('isolated config validation failed')

        def sample():
            while not stop_sampling.wait(1):
                pids = ','.join(str(p.pid) for _, p in processes if p.poll() is None)
                if not pids: continue
                rss = subprocess.run(['ps', '-p', pids, '-o', 'pid=,rss='], capture_output=True, text=True)
                samples.append({'seconds': round(time.monotonic()-start, 2), 'rss_kib': rss.stdout.strip()})
        threading.Thread(target=sample, daemon=True).start()
        prompt = ('Read calc.py and test_calc.py with tools. Fix add in calc.py so it adds instead of subtracting; '
                  'keep its signature and change nothing else. Do not modify tests. '
                  f'Run exactly `{command}` using the shell tool. '
                  'Finish only after the three tests pass. Briefly report the actual test result.')
        client = spawn([str(args.opencode), 'run', '--standalone', '--model', 'seiya/pegassus',
                        '--agent', 'smoke', '--format', 'json', '--title', 'Seiya S1-06 isolated fixture', prompt],
                       'opencode', cwd=work)
        try:
            summary['opencode_exit'] = client.wait(timeout=420)
        except subprocess.TimeoutExpired:
            raise RuntimeError('OpenCode overall deadline; stop own client and drain own runtime')
        if hashlib.sha256((work / 'test_calc.py').read_bytes()).hexdigest() != test_hash:
            raise RuntimeError('fixed tests changed')
        verify = subprocess.run(['/opt/homebrew/bin/python3', '-m', 'unittest', '-v'],
                                cwd=work, env=env, capture_output=True, text=True, timeout=10)
        (output / 'independent-tests.txt').write_text(verify.stdout + verify.stderr)
        summary['independent_tests_exit'] = verify.returncode
        summary['opencode_output'] = 'opencode.stdout'
        summary['evidence'] = inspect_evidence(output, records, command)
        summary['passed'] = summary['opencode_exit'] == 0 and verify.returncode == 0
    except Exception as error:
        summary['failure'] = str(error)
    finally:
        stop_sampling.set()
        # Only processes started above. Never signal a shared OpenCode/MLX/Python.
        for name, process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=190 if name == 'rust' else 20)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=10)
                    summary.setdefault('forced_own_process_cleanup', []).append(name)
            summary.setdefault('process_exit', {})[name] = process.returncode
            summary.setdefault('process_pid', {})[name] = process.pid
            if process.stdout is not None: process.stdout.close()
        for server in servers:
            server.shutdown(); server.server_close()
        for file in files: file.close()
        summary['seconds'] = round(time.monotonic() - start, 3)
        (output / 'rss-samples.json').write_text(json.dumps(samples, indent=2))
        peaks = {}
        for name, process in processes:
            values = [int(row.split()[1]) for sample in samples
                      for row in sample['rss_kib'].splitlines()
                      if row.split()[0] == str(process.pid)]
            peaks[name] = max(values, default=None)
        summary['sampled_peak_rss_kib'] = peaks
        summary['memory_scope'] = 'Started process PIDs only; OpenCode private server child excluded; not total RAM/VRAM.'
        # Our configs contain an env reference, not a literal secret.
        (output / 'summary.json').write_text(json.dumps(summary, indent=2))
    print(json.dumps({'passed': summary['passed'], 'failure': summary.get('failure'), 'output': str(output)}))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-real', action='store_true', required=True,
                        help='Explicitly authorize loading existing local weights and real fixture tools')
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--mlx', type=Path, required=True)
    parser.add_argument('--opencode', type=Path, default=Path(shutil.which('opencode') or 'opencode'))
    parser.add_argument('--rust', type=Path, default=Path('target/release/seiya-server'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    for name in ['model', 'mlx', 'opencode', 'rust']:
        setattr(args, name, getattr(args, name).resolve(strict=True))
    raise SystemExit(run(args))
