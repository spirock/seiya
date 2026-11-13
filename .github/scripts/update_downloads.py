"""Update only the README download block from one verified run's artifact metadata."""
import argparse
import json
import re
from pathlib import Path

START = '<!-- seiya-downloads:start -->'
END = '<!-- seiya-downloads:end -->'
REQUIRED = ('seiya-rust-Windows-X64-installer', 'seiya-rust-Windows-X64-portable',
            'seiya-rust-packages-macOS-ARM64', 'seiya-rust-packages-Linux-X64')


def updated(readme, metadata, repository, run_id, sha):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository) or not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('invalid repository/revision')
    if type(run_id) is not int or run_id <= 0:
        raise ValueError('invalid run id')
    links = {}
    base = f'https://github.com/{repository}/actions/runs/{run_id}'
    for artifact in metadata['artifacts']:
        name = artifact['name']
        if name not in REQUIRED:
            continue
        if name in links or artifact.get('expired') is not False or type(artifact['id']) is not int or artifact['id'] <= 0:
            raise ValueError('duplicate/expired/invalid artifact')
        origin = artifact['workflow_run']
        if origin['id'] != run_id or origin['head_sha'] != sha:
            raise ValueError('artifact source run/revision mismatch')
        links[name] = f'{base}/artifacts/{artifact["id"]}'
    if set(links) != set(REQUIRED):
        raise ValueError('required installers missing; previous links preserved')
    if readme.count(START) != 1 or readme.count(END) != 1 or readme.index(START) >= readme.index(END):
        raise ValueError('ambiguous README markers')
    block = f'''{START}
**Último build verificado:** [`{sha[:7]}`]({base}).

| Sistema | Descarga |
|---|---|
| Windows x64 | [Instalador .exe]({links[REQUIRED[0]]}) · [ZIP portable]({links[REQUIRED[1]]}) |
| macOS Apple Silicon | [.dmg / .pkg]({links[REQUIRED[2]]}) |
| Linux x64 | [.deb]({links[REQUIRED[3]]}) |
{END}'''
    return readme[:readme.index(START)] + block + readme[readme.index(END) + len(END):]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--readme', type=Path, required=True)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--run-id', type=int, required=True)
    parser.add_argument('--sha', required=True)
    args = parser.parse_args()
    if args.artifacts.stat().st_size > 4 * 1024 * 1024:
        parser.error('artifact metadata exceeds bound')
    original = args.readme.read_text(encoding='utf-8')
    text = updated(original, json.loads(args.artifacts.read_text(encoding='utf-8')), args.repository, args.run_id, args.sha)
    if text != original:
        args.readme.write_text(text, encoding='utf-8')


if __name__ == '__main__':
    main()
