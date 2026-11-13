"""Invoked by Rust integration test. Synthetic models/search, actual local IPC."""
import json
import sys
import threading
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'swarm-app'))
from seiya.config import example
from seiya.coordinator import Coordinator
from seiya.reader_profile import BrowserReader
from seiya.server import make_server

port = int(sys.argv[1])
cases = json.loads((ROOT / 'contracts/benchmarks/chat-session/v0.1.0/workload.json').read_bytes())['synthesis_cases']
report = []
for case in cases:
    refs = case['sources']
    unreadable = case['id'] == 'summary-03'
    ids = [s['id'] for s in refs]
    read_ids = ids + (['s2'] if case['id'] == 'summary-06' else [])
    answer = '; '.join(case['expected_facts'] + ([case['expected_limitation']] if case['expected_limitation'] else []))
    steps = [('web_search', {'query': case['id']})]
    if refs:
        steps.append(('web_read', {'ids': read_ids}))
    steps.append(('web_answer', {'summary': answer, 'citations': [] if unreadable else ids}))
    actions = iter(steps)

    def model(url, body, timeout):
        name, args = next(actions)
        if name == 'web_answer' and refs:
            delivered = json.loads(body['messages'][-1]['content'])
            assert delivered['kind'] == 'read_excerpts' and delivered['untrusted'] is True
            assert [s['id'] for s in delivered['sources']] == ([] if unreadable else ids)
            assert 'INSTRUCCIÓN MALICIOSA' not in body['messages'][0]['content']
        return {'usage': {'total_tokens': 1}, 'choices': [{'message': {'role': 'assistant', 'content': None,
            'tool_calls': [{'id': 'synthetic-call', 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}}]}

    def search(query, deadline):
        return [{'title': 'Synthetic article', 'url': 'https://example.org/' + case['id'] + '/' + source_id,
                 'snippet': 'Untrusted snippet only'} for source_id in read_ids]

    config = example('mlx')
    config['mode'], config['models'] = 'fast', config['models'][:1]
    profile = BrowserReader({'base_url': f'http://127.0.0.1:{port}/internal/reader/v0.1',
        'api_key_env': 'SEIYA_READER_FIXTURE_KEY', 'context_tokens': 4096, 'output_tokens': 256, 'margin_tokens': 128}, search=search)
    server = make_server(Coordinator(config, model), '127.0.0.1', 0, 'synthetic-browser-key', profile)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    def post(path, data):
        request = urllib.request.Request(f'http://127.0.0.1:{server.server_port}' + path,
            data=json.dumps(data).encode(), headers={'Content-Type': 'application/json', 'Authorization': 'Bearer synthetic-browser-key'})
        # No environment proxy even for loopback fixture traffic.
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=5) as response:
            return json.load(response)
    try:
        ref = post('/api/reader/runs', {})
        result = post('/api/chat', {'messages': [{'role': 'user', 'content': case['prompt']}], 'reader_run': ref})
        assert result['choices'][0]['message']['content'] == answer
        assert result['web']['citations'] == ([] if unreadable else ids)
        assert result['web']['usefulness'] == 'not_evaluated'
        if case['id'] in ('summary-03', 'summary-06', 'summary-09'):
            assert result['web']['pipeline_outcome'] == 'partial'
        if case['id'] == 'summary-06':
            assert result['web']['reads'][1]['status'] == 'http_error'
        if case['id'] == 'summary-03':
            assert result['web']['reads'][0]['status'] == 'no_useful_text'
        assert post('/api/reader/status', ref)['phase'] == 'finished'
        report.append({'id': case['id'], 'orchestration': 'passed', 'real_model_quality': None,
                       'pipeline_outcome': result['web']['pipeline_outcome'],
                       'reference_answer_matches': result['choices'][0]['message']['content'] == answer,
                       'citation_ids_validated': True, 'source_text_in_tool_not_system': True})
    finally:
        server.shutdown(); server.server_close(); thread.join()
print(json.dumps({'synthetic_only': True, 'cases': report}))
