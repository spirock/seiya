"""Explicit integration run against the user's already-running local MLX server.

Never imported by unittest; starts and stops only its own Seiya child process.
"""
import json
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

from seiya.config import example


def main():
    root = Path(__file__).resolve().parents[1]
    out = Path.home() / 'colibri-models/tool-test-results/seiya-integration-structured'
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='seiya-smoke-') as tmp:
        config = example('mlx')
        config.update(mode='fast', max_output_tokens=384)
        path = Path(tmp) / 'config.json'
        path.write_text(json.dumps(config))
        with (out/'server.log').open('w') as log:
            process = subprocess.Popen([sys.executable, '-m', 'seiya', 'serve', '--config', str(path), '--port', '8010'],
                                       cwd=root, stdout=log, stderr=log)
            try:
                for _ in range(30):
                    if process.poll() is not None:
                        raise RuntimeError('Seiya startup failed; inspect server.log')
                    try:
                        with urllib.request.urlopen('http://127.0.0.1:8010/v1/models', timeout=2) as r:
                            models = json.load(r)
                            assert models['data'][0]['id'] == 'pegassus'
                            break
                    except OSError:
                        time.sleep(.2)
                else:
                    raise RuntimeError('Seiya startup timeout')
                subprocess.run([sys.executable, str(root.parent/'local-tools-tests/run_suite.py'),
                                '--model', 'pegassus', '--url', 'http://127.0.0.1:8010',
                                '--output', str(out/'tools'), '--repeat', '3'], check=True)
                body = {'model': 'pegassus', 'swarm_mode': 'consensus', 'messages': [
                    {'role': 'user', 'content': 'Responde únicamente con el número entero que resulta de 2+2.'}]}
                request = urllib.request.Request('http://127.0.0.1:8010/v1/chat/completions',
                                                 data=json.dumps(body).encode(), headers={'Content-Type':'application/json'})
                with urllib.request.urlopen(request, timeout=600) as response:
                    result = json.load(response)
                (out/'consensus.json').write_text(json.dumps(result, ensure_ascii=False, indent=2))
                assert result['choices'][0]['message']['content'].strip() == '4'
                assert sum(result['swarm']['votes'].values()) >= 2
                print('LIVE CONSENSUS:', json.dumps(result['swarm'], ensure_ascii=False), flush=True)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait()


if __name__ == '__main__':
    main()
