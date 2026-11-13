import argparse
import json
import os
import sys
import urllib.request
from pathlib import Path

from . import __version__
from .config import default_path, example, load
from .coordinator import Coordinator
from .server import make_server


def main():
    parser = argparse.ArgumentParser(prog='seiya')
    parser.add_argument('--version', action='version', version=__version__)
    commands = parser.add_subparsers(dest='command', required=True)
    init = commands.add_parser('init', help='write a starter config, never overwrite')
    init.add_argument('--backend', choices=['mlx', 'ollama'], default='mlx' if sys.platform == 'darwin' else 'ollama')
    init.add_argument('--config', type=Path, default=default_path())
    for name in ['serve', 'doctor']:
        sub = commands.add_parser(name)
        sub.add_argument('--config', type=Path, default=default_path())
        if name == 'serve':
            sub.add_argument('--host', choices=['127.0.0.1'], default='127.0.0.1')
            sub.add_argument('--port', type=int, default=8010)
            sub.add_argument('--open', action='store_true', help='open the local web interface')
    args = parser.parse_args()
    try:
        if args.command == 'init':
            args.config.parent.mkdir(parents=True, exist_ok=True)
            with args.config.open('x') as out:
                json.dump(example(args.backend), out, indent=2)
            print(f'Created {args.config}. No models downloaded or services started.')
            return
        config = load(args.config)
        if args.command == 'doctor':
            endpoint = config['models'][0]['base_url'] + '/models'
            with urllib.request.urlopen(endpoint, timeout=5) as response:
                data = json.load(response)
            print(json.dumps({'config_valid': True, 'endpoint_responding': True,
                              'models_load_tested': False, 'endpoint_models': data,
                              'note': 'Run tool and coding tests before relying on a model.'}, indent=2))
            return
        key = os.environ.get('SEIYA_API_KEY') or None
        server = make_server(Coordinator(config), args.host, args.port, key)
        print(f'Seiya {__version__} · model: pegassus\nBrowser: http://{args.host}:{args.port}/\nAPI: http://{args.host}:{args.port}/v1', flush=True)
        if args.open:
            import webbrowser
            webbrowser.open(f'http://{args.host}:{args.port}/')
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()
    except (ValueError, OSError) as error:
        parser.exit(1, f'Seiya: {error}\n')


if __name__ == '__main__':
    main()
