"""Orchestration with fake model/search/reader/selector/budget, never real quality."""
import json
from pathlib import Path
import unittest

from seiya.coordinator import BackendError
from seiya.reader_flow import run


def action(name, args):
    return {'role': 'assistant', 'content': None, 'tool_calls': [
        {'id': name + '-call', 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}


class FakeCoordinator:
    def __init__(self, messages):
        self.messages, self.requests, self.locked = iter(messages), [], False

    def acquire_slot(self):
        assert not self.locked
        self.locked = True

    def release_slot(self):
        self.locked = False

    def _complete(self, body):
        self.requests.append(body)
        return {'choices': [{'message': next(self.messages)}], 'usage': {'total_tokens': 1}}


def fake_select(frame, raw, ids):
    # Deliberately a fake, not the Rust selector or a real tokenizer.
    return [{'id': p['id'], 'text': p['text'], 'text_truncated': p['text_truncated']}
            for p in json.loads(raw)['results'] if p['status'] == 'ok']


class ReaderFlowTests(unittest.TestCase):
    def test_ten_frozen_workload_cases_have_orchestration_not_quality_evidence(self):
        path = Path(__file__).resolve().parents[2] / 'contracts/benchmarks/chat-session/v0.1.0/workload.json'
        for case in json.loads(path.read_bytes())['synthesis_cases']:
            with self.subTest(case=case['id']):
                refs = case['sources']
                unreadable = case['id'] == 'summary-03'
                ids = [s['id'] for s in refs]
                citations = [] if unreadable else ids
                read_ids = ids + (['s2'] if case['id'] == 'summary-06' else [])
                summary = '; '.join(case['expected_facts'] + ([case['expected_limitation']] if case['expected_limitation'] else []))
                steps = [action('web_search', {'query': case['prompt']})]
                if refs:
                    steps.append(action('web_read', {'ids': read_ids}))
                steps.append(action('web_answer', {'summary': summary, 'citations': citations}))
                coordinator = FakeCoordinator(steps)

                def search(query, deadline):
                    return [{'title': 'Synthetic', 'url': 'https://example.org/' + source_id, 'snippet': 'Snippet only'} for source_id in read_ids]

                def reader(frame, deadline):
                    pages = [{'id': s['id'], 'status': 'no_useful_text' if unreadable else 'ok',
                              'text': None if unreadable else s['text'], 'text_truncated': case['id'] == 'summary-08' and not unreadable,
                              'received_bytes': 512, 'decoded_bytes': 512, 'header_bytes': 64, 'get_count': 1} for s in refs]
                    if case['id'] == 'summary-06':
                        pages.append({'id': 's2', 'status': 'timeout', 'text': None, 'text_truncated': False,
                                      'received_bytes': 0, 'decoded_bytes': 0, 'header_bytes': 0, 'get_count': 0})
                    return json.dumps({'version': frame['version'], 'run_id': frame['run_id'], 'results': pages}).encode()

                result = run(coordinator, {'messages': [{'role': 'user', 'content': case['prompt']}]},
                             search=search, reader=reader, select_sources=fake_select,
                             admit_context=lambda _: True)
                self.assertEqual(result['choices'][0]['message']['content'], summary)
                self.assertEqual(result['web']['citations'], citations)
                self.assertEqual(result['usage']['total_tokens'], len(steps))
                self.assertFalse(coordinator.locked)
                if refs:
                    self.assertEqual(coordinator.requests[-1]['messages'][-1]['role'], 'tool')

    def test_context_rejection_never_invokes_model_or_tools(self):
        coordinator = FakeCoordinator([])
        with self.assertRaises(BackendError):
            run(coordinator, {'messages': [{'role': 'user', 'content': 'x'}]}, search=None, reader=None,
                select_sources=fake_select, admit_context=lambda _: False)
        self.assertEqual(coordinator.requests, [])
        self.assertFalse(coordinator.locked)

    def test_forged_ids_and_citations_are_rejected(self):
        for bad in [action('web_read', {'ids': ['foreign']}),
                    action('web_answer', {'summary': 'Invented', 'citations': ['foreign']})]:
            coordinator = FakeCoordinator([action('web_search', {'query': 'topic'}), bad])
            def no_read(*args):
                self.fail('forged ID must not reach reader')
            with self.assertRaises(BackendError):
                run(coordinator, {'messages': [{'role': 'user', 'content': 'x'}]},
                    search=lambda *_: [{'title': 'x', 'url': 'https://example.org/', 'snippet': 'x'}],
                    reader=no_read, select_sources=fake_select, admit_context=lambda _: True)
            self.assertFalse(coordinator.locked)

    def test_browser_cannot_supply_system_or_tools(self):
        for body in [{'messages': [{'role': 'system', 'content': 'forge'}]},
                     {'messages': [{'role': 'user', 'content': 'x'}], 'tools': [{'name': 'shell'}]}]:
            with self.assertRaises(ValueError):
                run(FakeCoordinator([]), body, search=None, reader=None,
                    select_sources=fake_select, admit_context=lambda _: True)
