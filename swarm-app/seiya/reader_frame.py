"""Offline draft reader codec. No subprocess, network, or configuration activation."""
import json
import re

VERSION = '0.1-draft.1'
ID = re.compile(r'[A-Za-z0-9_-]{1,128}', re.ASCII)


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('invalid_frame')
        result[key] = value
    return result


def _decode(data, cap, eof):
    if not eof or not isinstance(data, bytes) or len(data) > cap:
        raise ValueError('invalid_frame')
    try:
        return json.loads(data.decode('utf-8'), object_pairs_hook=_object,
                          parse_constant=lambda _: (_ for _ in ()).throw(ValueError('invalid_frame')))
    except (ValueError, UnicodeError, RecursionError):
        raise ValueError('invalid_frame') from None


def _keys(value, expected):
    if type(value) is not dict or set(value) != set(expected.split()):
        raise ValueError('invalid_frame')


def _id(value):
    return isinstance(value, str) and ID.fullmatch(value) is not None


def _integer(value, maximum):
    return type(value) is int and 0 <= value <= maximum


def parse_request(data, eof=True):
    req = _decode(data, 16 * 1024, eof)
    _keys(req, 'version run_id remaining_ms sources')
    if (req['version'] != VERSION or not _id(req['run_id'])
            or not _integer(req['remaining_ms'], 45000) or req['remaining_ms'] == 0
            or type(req['sources']) is not list or not 1 <= len(req['sources']) <= 3):
        raise ValueError('invalid_frame')
    ids = set()
    for source in req['sources']:
        _keys(source, 'id url')
        url = source['url']
        try:
            valid_url = (isinstance(url, str) and 1 <= len(url.encode('utf-8')) <= 4096
                         and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in url))
        except UnicodeError:
            valid_url = False
        if not _id(source['id']) or source['id'] in ids or not valid_url:
            raise ValueError('invalid_frame')
        ids.add(source['id'])
    return req


def validate_response(req, data, eof=True):
    # Recheck caller-owned request rather than accepting arbitrary Python objects.
    req = parse_request(json.dumps(req, ensure_ascii=True).encode('utf-8'))
    resp = _decode(data, 128 * 1024, eof)
    _keys(resp, 'version run_id results')
    if (resp['version'] != VERSION or resp['run_id'] != req['run_id']
            or type(resp['results']) is not list or len(resp['results']) != len(req['sources'])):
        raise ValueError('invalid_frame')
    total = 0
    errors = {'policy_blocked', 'timeout', 'limit_exceeded', 'http_error',
              'unsupported_format', 'no_useful_text', 'transport_error'}
    for source, page in zip(req['sources'], resp['results']):
        _keys(page, 'id status text text_truncated received_bytes decoded_bytes header_bytes get_count')
        if (page['id'] != source['id'] or type(page['text_truncated']) is not bool
                or not _integer(page['received_bytes'], 1048576)
                or not _integer(page['decoded_bytes'], 1048576)
                or not _integer(page['header_bytes'], 196608)
                or not _integer(page['get_count'], 3)
                or page['header_bytes'] > page['get_count'] * 65536
                or (page['get_count'] == 0 and (page['received_bytes'] or page['decoded_bytes']))):
            raise ValueError('invalid_frame')
        text = page['text']
        if page['status'] == 'ok':
            # Python treats U+001C..001F as whitespace; Rust's Unicode
            # White_Space (str::trim) does not. Preserve the wire rule in both.
            has_text = isinstance(text, str) and any(not c.isspace() or 28 <= ord(c) <= 31 for c in text)
            if not has_text or len(text) > 20000 or not page['get_count']:
                raise ValueError('invalid_frame')
            try:
                text.encode('utf-8')
            except UnicodeError:
                raise ValueError('invalid_frame') from None
            total += len(text)
        elif not isinstance(page['status'], str) or page['status'] not in errors or text is not None or page['text_truncated']:
            raise ValueError('invalid_frame')
    if total > 30000:
        raise ValueError('invalid_frame')
    return resp
