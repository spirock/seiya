"""Run fixed tests ONLY after manually reviewing the generated files.

Not a sandbox. Do not use this on unreviewed/untrusted Python.
"""
import argparse
import copy
import importlib.util
import json
from pathlib import Path


def load(path):
    spec = importlib.util.spec_from_file_location('generated', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def intervals(m):
    f = m.merge_intervals
    for source, expected in [([], []), ([(1, 2)], [(1, 2)]),
                             ([(1, 2), (3, 4)], [(1, 2), (3, 4)]),
                             ([(1, 2), (2, 4)], [(1, 4)]),
                             ([(1, 10), (2, 3)], [(1, 10)]),
                             ([(3, 5), (-3, 0), (0, 3)], [(-3, 5)]),
                             ([(0.1, 0.2), (0.4, 0.5)], [(0.1, 0.2), (0.4, 0.5)])]:
        original = copy.deepcopy(source)
        actual = f(source)
        assert actual == expected, f'{source}: expected {expected}, got {actual}'
        assert source == original, 'input mutated'
    try:
        f([(3, 1)])
    except ValueError:
        pass
    else:
        raise AssertionError('invalid interval accepted')


def lru(m):
    for capacity in [0, -1]:
        try:
            m.LRUCache(capacity)
        except ValueError:
            pass
        else:
            raise AssertionError('invalid capacity accepted')
    c = m.LRUCache(2)
    assert c.get('absent') == -1
    c.put('a', 1); c.put('b', 2)
    assert c.get('a') == 1
    c.put('c', 3)
    assert c.get('b') == -1, 'evicted MRU rather than LRU'
    assert c.get('a') == 1 and c.get('c') == 3
    c.put('a', 9)
    assert c.get('c') == 3 and c.get('a') == 9, 'update evicted another key'
    c.put('d', 4)
    assert c.get('c') == -1 and c.get('a') == 9


def store(m):
    s = m.TaskStore()
    try:
        assert s.list_tasks() == [], 'list_tasks must return list, initially empty'
        for title in ['', ' ', '\n\t']:
            try:
                s.add(title)
            except ValueError:
                pass
            else:
                raise AssertionError('blank accepted')
        title = "'); DROP TABLE tasks; --"
        a, b = s.add(' áé 🚀 '), s.add(title)
        assert type(a) is int and a != b
        assert s.list_tasks() == [(a, ' áé 🚀 ', 0), (b, title, 0)]
        s.complete(b); s.complete(b)
        assert s.list_tasks() == [(a, ' áé 🚀 ', 0), (b, title, 1)]
        s.delete(a); s.delete(b)
        assert s.list_tasks() == []
        for op in [s.complete, s.delete]:
            try:
                op(99999)
            except KeyError:
                pass
            else:
                raise AssertionError('missing KeyError')
        s.add('after deletion')
        assert not s.conn.in_transaction, 'write was not committed'
    finally:
        s.close()


def main():
    p = argparse.ArgumentParser()
    p.add_argument('directory')
    args = p.parse_args()
    root = Path(args.directory)
    results = []
    for name, check in [('merge_intervals', intervals), ('lru_cache', lru), ('task_store', store)]:
        try:
            check(load(root / (name + '.py')))
            row = {'task': name, 'passed': True}
        except Exception as error:
            row = {'task': name, 'passed': False, 'error': f'{type(error).__name__}: {error}'}
        results.append(row)
    (root / 'functional-tests.json').write_text(json.dumps(results, indent=2))
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
