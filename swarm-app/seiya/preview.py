"""Explicit local UI preview. No configuration, models or external services.

This responder is intentionally synthetic; never use it as inference evidence.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import select
import signal
import subprocess
import uuid

from .coordinator import Coordinator
from .server import make_server


def synthetic_transport(url, body, timeout):
    messages = body['messages']
    if body.get('tools') and messages[-1]['role'] != 'tool':
        text = messages[-1].get('content', '')
        lowered = text.lower()
        if any(word in lowered for word in ('cómo va', 'como va', 'estado', 'progreso')):
            name, args = 'code_job_list', {}
        elif any(word in lowered for word in ('python', 'programa', 'script', 'archivo', 'código')):
            name, args = 'code_job_submit', {'task': text}
        else:
            name = None
        if name:
            return {'choices': [{'message': {'role': 'assistant', 'content': None,
                'tool_calls': [{'id': uuid.uuid4().hex, 'type': 'function', 'function': {
                'name': name, 'arguments': json.dumps(args)}}]}, 'finish_reason': 'tool_calls'}], 'usage': {}}
    if messages[-1]['role'] == 'tool':
        observed = json.loads(messages[-1]['content'])
        if 'job_id' in observed:
            answer = f"SIMULACIÓN: trabajo {observed['job_id']} recibido. El panel mostrará su estado. Puedes seguir conversando."
        else:
            answer = 'Estado observado (SIMULACIÓN): ' + ', '.join(f"{j['job_id']}: {j['phase']}" for j in observed.get('jobs', []))
        return {'choices': [{'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}], 'usage': {}}
    return {'choices': [{'message': {'role': 'assistant', 'content':
        'Vista previa simulada de Seiya. La interfaz está disponible, pero todavía '
        'no estoy conectado a Gemma o Qwen. Los trabajos en segundo plano están '
        'en desarrollo; este mensaje no procede de un modelo.'}, 'finish_reason': 'stop'}],
        'usage': {'prompt_tokens': 0, 'completion_tokens': 0, 'total_tokens': 0}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=8010)
    parser.add_argument('--jobs-binary', type=Path, help='explicit repository-built synthetic jobs binary')
    args = parser.parse_args()
    child = None
    profile = None
    if args.jobs_binary:
        from .jobs_profile import BrowserJobs
        key = secrets.token_hex(32)
        binary = args.jobs_binary.resolve(strict=True)
        child = subprocess.Popen([str(binary)], env={'SEIYA_JOBS_PREVIEW_KEY': key, 'PATH': '/usr/bin:/bin'},
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            if not select.select([child.stdout], [], [], 5)[0]:
                raise RuntimeError('preview jobs startup deadline')
            endpoint = child.stdout.readline(256).decode().strip()
            os.environ['SEIYA_JOBS_PREVIEW_KEY'] = key
            try:
                profile = BrowserJobs({'base_url': endpoint, 'api_key_env': 'SEIYA_JOBS_PREVIEW_KEY'})
            finally:
                os.environ.pop('SEIYA_JOBS_PREVIEW_KEY', None)
        except BaseException:
            child.terminate()
            child.wait(timeout=5)
            child.stdout.close()
            raise
    config = {'mode': 'fast', 'worker': 'preview', 'timeout_seconds': 5,
              'max_output_tokens': 256, 'models': [{'id': 'preview', 'backend': 'mlx',
              'model': 'synthetic-preview-not-a-model', 'base_url': 'http://127.0.0.1:1/v1'}]}
    coordinator = Coordinator(config, transport=synthetic_transport)
    coordinator.preview = True
    try:
        server = make_server(coordinator, '127.0.0.1', args.port,
                             api_key='preview-local' if profile else None, jobs_profile=profile)
    except BaseException:
        if child:
            child.terminate()
            child.wait(timeout=5)
            child.stdout.close()
        raise
    def stop_preview(signum, frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop_preview)
    print(f'Seiya · VISTA PREVIA SIMULADA · http://127.0.0.1:{server.server_port}/ · sin modelos', flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        if child:
            child.terminate()
            child.wait(timeout=5)
            child.stdout.close()


if __name__ == '__main__':
    main()
