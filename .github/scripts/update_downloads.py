"""Update only the README download block from one verified run's artifact metadata."""
import argparse
import json
import re
from pathlib import Path

START = '<!-- seiya-downloads:start -->'
END = '<!-- seiya-downloads:end -->'
REQUIRED = ('seiya-rust-Windows-X64-installer', 'seiya-rust-Windows-X64-portable',
            'seiya-rust-packages-macOS-ARM64', 'seiya-rust-packages-Linux-X64',
            'seiya-rust-pkg-macOS-ARM64')


def updated(readme, metadata, repository, run_id, sha, language='es'):
    labels = {
        'es': ('Último build verificado', 'Sistema', 'Descarga', 'Instalador .exe', 'ZIP portable', 'DMG · arrastrar a Aplicaciones', 'PKG opcional'),
        'en': ('Latest verified build', 'Platform', 'Download', '.exe installer', 'Portable ZIP', 'DMG · drag to Applications', 'Optional PKG'),
    }
    if language not in labels:
        raise ValueError('unsupported README language')
    verified, platform, download, exe, portable, dmg, pkg = labels[language]
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
**{verified}:** [`{sha[:7]}`]({base}).

| {platform} | {download} |
|---|---|
| Windows x64 | [{exe}]({links[REQUIRED[0]]}) · [{portable}]({links[REQUIRED[1]]}) |
| macOS Apple Silicon | [{dmg}]({links[REQUIRED[2]]}) · [{pkg}]({links[REQUIRED[4]]}) |
| Linux x64 | [.deb]({links[REQUIRED[3]]}) |
{END}'''
    return readme[:readme.index(START)] + block + readme[readme.index(END) + len(END):]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--readme', type=Path, required=True)
    parser.add_argument('--language', choices=('en','es'), default='es')
    parser.add_argument('--readme-es', type=Path)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--run-id', type=int, required=True)
    parser.add_argument('--sha', required=True)
    args = parser.parse_args()
    if args.artifacts.stat().st_size > 4 * 1024 * 1024:
        parser.error('artifact metadata exceeds bound')
    if args.readme_es and args.readme_es.resolve()==args.readme.resolve():
        parser.error('language documents must be distinct')
    metadata=json.loads(args.artifacts.read_text(encoding='utf-8'))
    changes=[]
    for path,language in [(args.readme,args.language)]+([(args.readme_es,'es')] if args.readme_es else []):
        original=path.read_text(encoding='utf-8')
        changes.append((path,original,updated(original,metadata,args.repository,args.run_id,args.sha,language)))
    # Validate both before writing; a rejected document never partially updates links.
    for path,original,text in changes:
        if text!=original:path.write_text(text,encoding='utf-8')


if __name__ == '__main__':
    main()
