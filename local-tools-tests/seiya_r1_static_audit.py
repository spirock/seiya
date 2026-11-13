#!/usr/bin/env python3
"""Explicit offline R1 artifact/tokenizer audit. Never imports MLX or loads tensors.

Requires existing tokenizers/Jinja2 in the operator interpreter; installs nothing.
No HTTP, personal documents, generated shell or host/model inference is involved.
"""
import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path
import struct
import time


REVISION = '21848dbf533d2518a1ef895104820d5ee51317ea'
EXPECTED = {
    'model.safetensors': (4284346255, '75ed9ed7ad66720041dd186dd0d8ce8a2dd8f487ea3f45a4b8fcaa7bbbf1f64f'),
    'tokenizer.json': (11422778, 'e20ddafc659ba90242154b55275402edeca0715e5dbb30f56815a4ce081f4893'),
}


def small_json(path, cap):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > cap:
        raise ValueError('invalid or oversized metadata')
    return json.loads(path.read_bytes())


def validate_tensor_index(header, payload_bytes, index):
    tensors = {key: value for key, value in header.items() if key != '__metadata__'}
    if not tensors or len(tensors) > 10000:
        raise ValueError('invalid tensor inventory')
    intervals = []
    for tensor in tensors.values():
        start, end = tensor['data_offsets']
        if type(start) is not int or type(end) is not int or not 0 <= start <= end <= payload_bytes:
            raise ValueError('invalid tensor offset')
        intervals.append((start, end))
    intervals.sort()
    if intervals[0][0] != 0 or intervals[-1][1] != payload_bytes or any(a[1] != b[0] for a, b in zip(intervals, intervals[1:])):
        raise ValueError('gaps or overlapping tensor payloads')
    if set(index['weight_map']) != set(tensors) or set(index['weight_map'].values()) != {'model.safetensors'}:
        raise ValueError('index/header mismatch')
    return len(tensors)


def audit(model):
    from jinja2.sandbox import ImmutableSandboxedEnvironment
    from tokenizers import Tokenizer

    manifest = small_json(model / 'download-manifest.json', 65536)
    if manifest.get('revision') != REVISION or manifest.get('completed') is not True:
        raise ValueError('unconfirmed model download')
    if manifest.get('repository') != 'mlx-community/DeepSeek-R1-Distill-Qwen-7B-4bit':
        raise ValueError('unexpected checkpoint')
    files = manifest['files']
    allowed = {'README.md', 'config.json', 'model.safetensors', 'model.safetensors.index.json',
               'special_tokens_map.json', 'tokenizer.json', 'tokenizer_config.json'}
    if len(files) != 7 or {item['name'] for item in files} != allowed:
        raise ValueError('unexpected inventory')
    verified = []
    for item in files:
        path = model / item['name']
        if path.is_symlink() or not path.is_file() or path.stat().st_size != item['bytes']:
            raise ValueError('missing or changed file')
        size = path.stat().st_size
        if size > 5 * 1024**3:
            raise ValueError('asset exceeds audit bound')
        digest = hashlib.sha256()
        with path.open('rb') as file:
            while chunk := file.read(1024 * 1024):
                digest.update(chunk)
        sha = digest.hexdigest()
        if sha != item['sha256'] or item['name'] in EXPECTED and (size, sha) != EXPECTED[item['name']]:
            raise ValueError('file digest mismatch')
        verified.append({'name': item['name'], 'bytes': size, 'sha256': sha})

    config = small_json(model / 'config.json', 2 * 1024**2)
    tokenizer_config = small_json(model / 'tokenizer_config.json', 2 * 1024**2)
    if config['model_type'] != 'qwen2' or config['quantization'] != {'bits': 4, 'group_size': 64}:
        raise ValueError('unexpected architecture or quantization')
    if 'auto_map' in config or 'auto_map' in tokenizer_config:
        raise ValueError('remote code mapping rejected')

    # Parse/validate only the bounded safetensors header, never materialize tensors.
    weights = model / 'model.safetensors'
    with weights.open('rb') as file:
        header_length = struct.unpack('<Q', file.read(8))[0]
        if not 2 <= header_length <= 2 * 1024**2:
            raise ValueError('oversized safetensors header')
        header = json.loads(file.read(header_length))
    payload_bytes = weights.stat().st_size - 8 - header_length
    index = small_json(model / 'model.safetensors.index.json', 2 * 1024**2)
    tensor_count = validate_tensor_index(header, payload_bytes, index)

    template = ImmutableSandboxedEnvironment().from_string(tokenizer_config['chat_template'])
    messages = [{'role': 'user', 'content': 'Responde exactamente SEIYA_TOKENIZER_OK_47.'}]
    options = {'messages': messages, 'bos_token': tokenizer_config['bos_token'], 'add_generation_prompt': True}
    plain = template.render(**options)
    tools = [{'type': 'function', 'function': {'name': 'read', 'description': 'synthetic only', 'parameters': {
        'type': 'object', 'properties': {'path': {'type': 'string', 'enum': ['fixture.txt']}}, 'required': ['path']}}}]
    with_tools = template.render(**options, tools=tools)
    tokenizer = Tokenizer.from_file(str(model / 'tokenizer.json'))
    encoded = tokenizer.encode(plain, add_special_tokens=False)
    decoded = tokenizer.decode(encoded.ids, skip_special_tokens=False)
    if not encoded.ids or 'SEIYA_TOKENIZER_OK_47' not in decoded:
        raise ValueError('tokenizer failed synthetic marker roundtrip')
    return {
        'mode': 'offline-artifact-and-tokenizer-audit', 'model_loaded': False, 'mlx_imported': False,
        'inference_tested': False, 'host_pressure_tested': False, 'tools_executed': False,
        'repository': manifest['repository'], 'revision': REVISION, 'verified_files': verified,
        'safetensors_header_bytes': header_length, 'tensor_count': tensor_count, 'index_matches_header': True,
        'versions': {name: importlib.metadata.version(name) for name in ['tokenizers', 'jinja2']},
        'tokenizer_marker_roundtrip': True, 'full_prompt_roundtrip_exact': decoded == plain,
        'synthetic_prompt_tokens': len(encoded.ids),
        'generation_prefix_forces_think': plain.endswith('<think>\n'),
        'provided_tool_schema_changes_prompt': with_tools != plain,
        'read_schema_present_in_rendered_prompt': '"name": "read"' in with_tools,
        'tool_calling_inference_validated': False,
        'limitations': ['tokenizer/template tested, no inference or model memory claim',
                        'tool schema not rendered is a template limitation, not a measured model score',
                        'token presence does not validate tools or vision',
                        'normal campaign provenance cap still rejects this large tokenizer'],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not args.model.is_absolute() or not args.output.is_absolute():
        raise ValueError('absolute operator paths required')
    args.output.mkdir(mode=0o700, exist_ok=False)
    started = time.monotonic()
    try:
        result = audit(args.model)
    except Exception as error:
        with (args.output / 'failure.json').open('x') as file:
            json.dump({'completed': False, 'error_type': type(error).__name__, 'model_loaded': False}, file)
        raise
    result['audit_seconds'] = time.monotonic() - started
    with (args.output / 'report.json').open('x') as file:
        json.dump(result, file, indent=2)
    print(json.dumps({key: value for key, value in result.items() if key != 'verified_files'}, indent=2))


if __name__ == '__main__':
    main()
