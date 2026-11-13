import json
import os
from pathlib import Path
from urllib.parse import urlparse


def default_path():
    if __import__('sys').platform == 'darwin':
        return Path.home() / 'Library/Application Support/Seiya/config.json'
    return Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'seiya/config.json'


def example(backend='mlx'):
    names = [('gemma', 'mlx-gemma4-e4b'), ('qwen', 'mlx-qwen35-9b'),
             ('ministral', 'mlx-ministral3-8b')]
    models = ['gemma4:e4b', 'qwen3.5:9b', 'ministral-3:8b']
    return {'mode': 'consensus', 'worker': 'gemma', 'timeout_seconds': 180,
            'max_output_tokens': 2048, 'models': [
                {'id': name, 'model': str(Path.home() / 'colibri-models' / folder)
                 if backend == 'mlx' else models[i],
                 'base_url': 'http://127.0.0.1:8081/v1' if backend == 'mlx'
                 else 'http://127.0.0.1:11434/v1',
                 'backend': backend}
                for i, (name, folder) in enumerate(names)]}


def load(path):
    data = json.loads(Path(path).read_text())
    if data.get('mode', 'consensus') not in ('fast', 'consensus'):
        raise ValueError('mode must be fast or consensus')
    models = data.get('models')
    if not isinstance(models, list) or not 1 <= len(models) <= 5:
        raise ValueError('models must contain 1–5 models')
    ids, endpoints = set(), set()
    for item in models:
        if not isinstance(item, dict):
            raise ValueError('each model must be an object')
        for key in ('id', 'model', 'base_url'):
            if not isinstance(item.get(key), str) or not item[key]:
                raise ValueError('each model requires id, model, base_url strings')
        if item['id'] in ids:
            raise ValueError('model IDs must be unique')
        ids.add(item['id'])
        parsed = urlparse(item['base_url'])
        if parsed.hostname not in ('127.0.0.1', 'localhost', '::1') or parsed.scheme != 'http':
            raise ValueError('only local HTTP model endpoints are allowed')
        if parsed.username or parsed.password or parsed.query or parsed.fragment:
            raise ValueError('endpoint must not contain credentials, query or fragment')
        if parsed.path.rstrip('/') != '/v1':
            raise ValueError('base_url must end in /v1')
        if item.get('backend') not in ('mlx', 'ollama'):
            raise ValueError('backend must be mlx or ollama')
        item['base_url'] = item['base_url'].rstrip('/')
        if parsed.port is None:
            raise ValueError('specify the model server port explicitly')
        endpoints.add((parsed.hostname, parsed.port, item['backend']))
    # One shared runtime switches models serially, instead of keeping several
    # heavyweight independent runtime processes resident on a 16 GB machine.
    if len(endpoints) != 1:
        raise ValueError('all models must share one runtime endpoint/backend')
    if data.get('worker') not in ids:
        raise ValueError('worker must name one of the configured model IDs')
    for key, default, low, high in [('timeout_seconds', 180, 1, 600),
                                   ('max_output_tokens', 2048, 32, 4096)]:
        value = data.get(key, default)
        if type(value) is not int or not low <= value <= high:
            raise ValueError(f'{key} must be an integer between {low} and {high}')
        data[key] = value
    if data.get('mode', 'consensus') == 'consensus' and len(models) < 3:
        raise ValueError('consensus requires at least three models')
    if 'runtime_bridge' in data:
        from .runtime_bridge import validate_bridge
        validate_bridge(data['runtime_bridge'])
        if data.get('mode') != 'fast' or len(models) != 1 or models[0]['backend'] != 'mlx':
            raise ValueError('runtime_bridge requires fast mode and one MLX worker')
    if 'web_reader' in data:
        from .reader_profile import validate_profile
        validate_profile(data['web_reader'])
        if data.get('mode') != 'fast' or len(models) != 1:
            raise ValueError('web_reader requires fast mode and one worker')
    if 'jobs' in data:
        from .jobs_profile import validate_profile
        validate_profile(data['jobs'])
        if data.get('mode') != 'fast' or len(models) != 1 or 'web_reader' in data:
            raise ValueError('jobs requires fast mode, one foreground worker and no web_reader')
    return data
