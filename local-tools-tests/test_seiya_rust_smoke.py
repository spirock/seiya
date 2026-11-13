"""Synthetic evidence checker tests: no model, service, network or user data."""
import copy
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

from seiya_rust_opencode_smoke import inspect_evidence


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.output = Path(self.temp.name)
        db = self.output / 'home/data/opencode/opencode.db'
        db.parent.mkdir(parents=True)
        with sqlite3.connect(db) as connection:
            connection.execute('create table session_message (type text, data text, seq integer)')
            connection.execute('insert into session_message values (?, ?, ?)',
                               ('idle', json.dumps({'outcome': 'succeeded'}), 1))
        self.command = '/opt/homebrew/bin/python3 -m unittest -v'
        inputs = [('read', {'path': 'calc.py'}), ('read', {'path': 'test_calc.py'}),
                  ('edit', {'path': 'calc.py', 'oldString': '-', 'newString': '+'}),
                  ('shell', {'command': self.command})]
        self.events = []
        calls = []
        for index, (name, args) in enumerate(inputs):
            call_id = 'synthetic-' + str(index)
            self.events.append({'type': 'tool_use', 'part': {
                'id': call_id, 'tool': name, 'state': {'status': 'completed', 'input': args,
                'output': 'Ran 3 tests\n\nOK\n', 'metadata': {'metadata': {'exit': 0}}}}})
            calls.append({'id': call_id, 'function': {'name': name, 'arguments': json.dumps(args)}})
        self.records = [{'status': 200, 'done': True, 'calls': calls,
                         'tool_results': [call['id'] for call in calls],
                         'usage': {'prompt_tokens': 100, 'completion_tokens': 20}}]

    def check(self):
        (self.output / 'opencode.stdout').write_text('\n'.join(json.dumps(e) for e in self.events))
        return inspect_evidence(self.output, self.records, self.command)

    def test_complete_synthetic_evidence(self):
        self.assertEqual(self.check()['tools_completed'], ['read', 'read', 'edit', 'shell'])

    def test_file_change_alone_is_not_success(self):
        self.events = []
        with self.assertRaises(RuntimeError): self.check()

    def test_failed_test_tool_is_not_success(self):
        self.events[-1]['part']['state']['metadata']['metadata']['exit'] = 1
        with self.assertRaises(RuntimeError): self.check()

    def test_modified_tool_arguments_are_not_success(self):
        self.records[0]['calls'][0]['function']['arguments'] = '{"path":"other.py"}'
        with self.assertRaises(RuntimeError): self.check()

    def test_missing_tool_result_is_not_success(self):
        self.records[0]['tool_results'].pop()
        with self.assertRaises(RuntimeError): self.check()

    def test_incomplete_sse_is_not_success(self):
        self.records[0]['done'] = False
        with self.assertRaises(RuntimeError): self.check()

    def test_duplicate_execution_is_not_success(self):
        self.events.append(copy.deepcopy(self.events[-1]))
        with self.assertRaises(RuntimeError): self.check()

    def test_non_idle_session_is_not_success(self):
        db = self.output / 'home/data/opencode/opencode.db'
        with sqlite3.connect(db) as connection:
            connection.execute('delete from session_message')
        with self.assertRaises(RuntimeError): self.check()
