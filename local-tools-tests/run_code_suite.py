"""Collect comparable coding/review outputs; do not execute generated code."""
import argparse
import ast
import json
import time
import urllib.request
from pathlib import Path
from probe_mamba import PROBES


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--model', required=True)
    p.add_argument('--output', required=True)
    p.add_argument('--url', default='http://127.0.0.1:8081')
    p.add_argument('--temperature', type=float, default=0)
    p.add_argument('--top-p', type=float, default=1)
    p.add_argument('--code-tokens', type=int, default=1000)
    p.add_argument('--review-tokens', type=int, default=384)
    p.add_argument('--reasoning-effort', choices=['none', 'low', 'medium', 'high'])
    args = p.parse_args()
    root = Path(args.output)
    root.mkdir(parents=True, exist_ok=True)
    summary = []
    for name, instruction in PROBES.items():
        body = {'model': args.model, 'messages': [{'role': 'user', 'content': instruction}],
                'max_tokens': args.code_tokens if not name.startswith('review_') else args.review_tokens,
                'temperature': args.temperature, 'top_p': args.top_p,
                'chat_template_kwargs': {'enable_thinking': False}}
        if args.reasoning_effort:
            body['reasoning_effort'] = args.reasoning_effort
        req = urllib.request.Request(args.url + '/v1/chat/completions',
                                     data=json.dumps(body).encode(),
                                     headers={'Content-Type': 'application/json'})
        start = time.monotonic()
        try:
            with urllib.request.urlopen(req, timeout=180) as response:
                result = json.load(response)
            text = result['choices'][0]['message'].get('content') or ''
            row = {'name': name, 'seconds': round(time.monotonic()-start, 3),
                   'finish_reason': result['choices'][0]['finish_reason'],
                   'usage': result.get('usage')}
            (root / (name+'.txt')).write_text(text)
            (root / (name+'.json')).write_text(json.dumps({'request': body, 'response': result}, indent=2))
            if name.startswith('review_'):
                expected = {'review_sql':'B','review_lru':'B','review_range':'A'}[name]
                try:
                    value = json.loads(text)
                    row['strict_json'] = True
                    row['choice_correct'] = value.get('choice') == expected
                except (ValueError, AttributeError):
                    row['strict_json'] = False
                    row['choice_correct'] = False
            else:
                code = text.strip()
                if code.startswith('```python\n') and code.endswith('```'):
                    code = code[len('```python\n'):-3].strip()
                (root / (name+'.py')).write_text(code+'\n')
                try:
                    ast.parse(code)
                    row['syntax_valid'] = True
                except SyntaxError as error:
                    row['syntax_valid'] = False
                    row['error'] = str(error)
        except Exception as error:
            row = {'name': name, 'error': str(error)}
        summary.append(row)
        (root / 'summary.json').write_text(json.dumps(summary, indent=2))
        print(json.dumps(row), flush=True)


if __name__ == '__main__':
    main()
