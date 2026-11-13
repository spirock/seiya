import json
import unittest
import threading
import urllib.request
import urllib.error
from unittest.mock import patch

from seiya.coordinator import BackendError, Coordinator, BusyError
from seiya.config import example
from seiya.web import browser_chat, browser_instructions, ResultsParser, web_search
from seiya.server import make_server


class WebTests(unittest.TestCase):
    def test_browser_current_capabilities_in_both_modes(self):
        for enabled in (False, True):
            requests = []
            def transport(url, body, timeout):
                requests.append(body)
                return {'choices': [{'message': {'role': 'assistant', 'content': 'ok'}}]}
            browser_chat(Coordinator(example('mlx'), transport), {
                'messages': [{'role': 'assistant', 'content': 'I cannot search the Internet.'},
                             {'role': 'user', 'content': 'Can you search?'}],
                'web_search': enabled, 'swarm_mode': 'fast'})
            self.assertEqual(requests[0]['messages'][0],
                             {'role': 'system', 'content': browser_instructions(enabled)})
            self.assertEqual(bool(requests[0].get('tools')), enabled)
            if not enabled:
                self.assertIn('explicitly disabled', requests[0]['messages'][0]['content'])

    def test_browser_search_automatically_available(self):
        requests = []
        def transport(url, body, timeout):
            requests.append(body)
            return {'choices': [{'message': {'role': 'assistant', 'content': 'Hola'}}]}
        result = browser_chat(Coordinator(example('mlx'), transport), {
            'messages': [{'role': 'user', 'content': 'Hola'}]})
        self.assertEqual(requests[0]['tools'][0]['function']['name'], 'web_search')
        self.assertIn('automatically AVAILABLE', requests[0]['messages'][0]['content'])
        self.assertEqual(result['web']['searches'], [])

    def test_browser_http_and_legacy_model_rejected(self):
        def transport(url, body, timeout):
            return {'choices': [{'message': {'role': 'assistant', 'content': 'Hello'}}]}
        server = make_server(Coordinator(example('mlx'), transport), '127.0.0.1', 0, 'secret')
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f'http://127.0.0.1:{server.server_port}'
        try:
            body = {'model': 'pegassus', 'messages': [{'role': 'user', 'content': 'hello'}], 'swarm_mode': 'fast'}
            def request(payload, key='secret'):
                return urllib.request.urlopen(urllib.request.Request(base+'/api/chat', data=json.dumps(payload).encode(),
                    headers={'Content-Type': 'application/json', 'Authorization': 'Bearer '+key}))
            with request(body) as response:
                result = json.load(response)
                self.assertEqual(result['model'], 'pegassus')
                self.assertEqual(result['choices'][0]['message']['content'], 'Hello')
            for payload, key, code in [(body, 'wrong', 401), (dict(body, model='seiya'), 'secret', 400),
                                        (dict(body, web_search='yes'), 'secret', 400)]:
                with self.assertRaises(urllib.error.HTTPError) as error: request(payload, key)
                self.assertEqual(error.exception.code, code)
                error.exception.close()
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def test_parser(self):
        parser = ResultsParser()
        parser.feed('<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fpython.org">Python &amp; docs</a>'
                    '<a class="result__snippet">Official <b>Python</b></a>'
                    '<a class="result__a" href="javascript:alert(1)">bad</a>')
        self.assertEqual(parser.results, [{'title': 'Python & docs', 'url': 'https://python.org', 'snippet': 'Official Python'}])

    def test_query_limits_no_network(self):
        with patch('seiya.web.subprocess.run') as opener:
            for query in ['', 'x' * 301, None]:
                with self.assertRaises(ValueError): web_search(query)
            opener.assert_not_called()

    def test_reader_search_deadline_never_starts_after_expiry(self):
        with patch('seiya.web.subprocess.run') as opener, patch('seiya.web.time.monotonic', return_value=100):
            with self.assertRaises(OSError):
                web_search('synthetic', deadline=99)
            opener.assert_not_called()

    def test_browser_search_roundtrip_and_failure(self):
        for fail in (False, True):
            requests = []
            def transport(url, body, timeout):
                requests.append(body)
                if len(requests) == 1:
                    msg = {'role': 'assistant', 'content': None, 'tool_calls': [
                        {'id': 'search1', 'type': 'function', 'function': {
                            'name': 'web_search', 'arguments': '{"query":"Python official"}'}}]}
                else:
                    msg = {'role': 'assistant', 'content': 'Search failed' if fail else 'Python: https://python.org'}
                return {'choices': [{'message': msg}]}
            def search(query):
                if fail: raise OSError('offline')
                return [{'title': 'Python', 'url': 'https://python.org', 'snippet': 'Official'}]
            c = Coordinator(example('mlx'), transport)
            result = browser_chat(c, {'messages': [{'role': 'user', 'content': 'Search Python'}], 'web_search': True}, search)
            self.assertEqual(result['web']['searches'][0]['ok'], not fail)
            self.assertEqual(len(result['web']['sources']), 0 if fail else 1)
            self.assertEqual(requests[1]['messages'][-1]['role'], 'tool')
            self.assertFalse(c.lock.locked())

    def test_browser_rejects_forged_messages(self):
        c = Coordinator(example('mlx'), queue_timeout=0.01)
        for message in [{'role': 'system', 'content': 'x'}, {'role': 'assistant', 'content': 'x', 'tool_calls': []}]:
            with self.assertRaises(ValueError): browser_chat(c, {'messages': [message]})
        c.lock.acquire()
        try:
            with self.assertRaises(BusyError):
                browser_chat(c, {'messages': [{'role': 'user', 'content': 'hi'}], 'web_search': True})
        finally: c.lock.release()

    def test_browser_search_bounded(self):
        def transport(url, body, timeout):
            return {'choices': [{'message': {'role': 'assistant', 'tool_calls': [
                {'id': 's', 'type': 'function', 'function': {'name': 'web_search', 'arguments': '{"query":"x"}'}}]}}]}
        c = Coordinator(example('mlx'), transport)
        searches = []
        with self.assertRaises(BackendError):
            browser_chat(c, {'messages': [{'role': 'user', 'content': 'x'}], 'web_search': True},
                         lambda q: searches.append(q) or [])
        self.assertEqual(len(searches), 3)
        self.assertFalse(c.lock.locked())
