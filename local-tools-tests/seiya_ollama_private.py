"""Manual macOS adapter for ADR0039–0043. Never run by normal tests or the service.

No downloads, shared-service signals, cache purge or generated-code execution.
Pure pressure/response decisions are delegated to the tested Rust checker.
"""
import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.request

BASE = Path('/private/var/folders/tc/6nkb5t5d5116q143rzc2dy400000gn/T/opencode')
LOCK = BASE / 'seiya-host-model-coordination/seiya-mlx-baseline.lock'
OLLAMA = Path('/Applications/Ollama.app/Contents/Resources/ollama')
TAGS = ('gemma4:e2b-mlx', 'qwen3.5:9b-mlx')
DIGESTS = ('f4a8449d17818601a45b32b22b90520b5c1c0327a27374bfb84e1974bb74104c',
           '203e30078279db51132b9e026ceb7bb21330e5b1af67ef190671b375c9770404')
LIMIT = 65536


def fixed(argv, timeout=2):
    result = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True,
                            timeout=timeout, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
    if result.returncode or len(result.stdout) > 1024 * 1024:
        raise RuntimeError('bounded command failed: ' + argv[0])
    return result.stdout.decode('utf-8')


def policy(checker, body):
    result = subprocess.run([str(checker)], input=json.dumps(body).encode(),
                            capture_output=True, timeout=2, env={'LC_ALL': 'C'})
    if result.returncode or len(result.stdout) > LIMIT:
        raise RuntimeError('Rust policy failed')
    return json.loads(result.stdout)


def snapshot():
    vm = fixed(['/usr/bin/vm_stat'])
    size = int(re.search(r'page size of (\d+) bytes', vm)[1])
    values = {}
    for label in ('Pages free', 'Pages inactive', 'Pages purgeable',
                  'Pages speculative', 'File-backed pages'):
        values[label] = int(re.search(re.escape(label) + r':\s+(\d+)\.', vm)[1]) * size
    swap = fixed(['/usr/sbin/sysctl', 'vm.swapusage'])
    used = re.search(r'used\s*=\s*([\d.]+)([KMG])', swap)
    if not used:
        raise RuntimeError('invalid swap measurement')
    return {'pressure': int(fixed(['/usr/sbin/sysctl', '-n', 'kern.memorystatus_vm_pressure_level'])),
            'swap_bytes': int(float(used[1]) * 1024 ** ('KMG'.index(used[2]) + 1)),
            'vm_bytes_overlapping_do_not_sum': values}


def admitted(checker, sample, baseline):
    return policy(checker, {'operation': 'snapshot', 'pressure': sample['pressure'],
                           'swap_bytes': sample['swap_bytes'],
                           'baseline_swap_bytes': baseline})['allowed'] is True


def memory_sample(observe=False):
    try:
        return snapshot()
    except Exception as exc:
        if not observe:
            raise
        return {'pressure': None, 'swap_bytes': None, 'vm_bytes_overlapping_do_not_sum': None,
                'memory_probe_error': type(exc).__name__ + ': ' + str(exc)[:128]}


def observation(checker, sample, baseline):
    return policy(checker, {'operation': 'qwen_observe_snapshot',
                           'pressure': sample['pressure'], 'swap_bytes': sample['swap_bytes'],
                           'baseline_swap_bytes': baseline})


class WarningWindow:
    """Host-owned state; Rust decides the accumulated bound. Never resets on normal."""
    def __init__(self, checker, baseline, start, qwen=False, critical=False):
        if critical and not qwen:
            raise ValueError('critical calibration is Qwen-only')
        self.checker, self.baseline, self.previous_time = checker, baseline, start
        self.previous_pressure, self.warning_ms = 1, 0
        self.operation = 'qwen_calibration_snapshot' if qwen else 'e2b_calibration_snapshot'
        self.budget_key = 'high_pressure_used_ms' if critical else 'warning_used_ms'
        if critical:
            self.operation = 'qwen_critical_calibration_snapshot'

    def observe(self, sample, now):
        if now < self.previous_time:
            raise RuntimeError('calibration clock regressed')
        # Round UP; no sub-ms budget lost by repeated truncation.
        delta_ms = math.ceil((now - self.previous_time) * 1000)
        result = policy(self.checker, {'operation': self.operation,
            'pressure': sample['pressure'], 'previous_pressure': self.previous_pressure,
            'delta_ms': delta_ms, self.budget_key: self.warning_ms,
            'swap_bytes': sample['swap_bytes'], 'baseline_swap_bytes': self.baseline})
        self.previous_time, self.previous_pressure = now, sample['pressure']
        self.warning_ms = result[self.budget_key]
        return result


class BsdInfo(ctypes.Structure):
    # SDK sys/proc_info.h, PROC_PIDTBSDINFO. Exact-size result required.
    _fields_ = [(name, ctypes.c_uint32) for name in (
        'flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid', 'ruid', 'rgid',
        'svuid', 'svgid', 'reserved')] + [
        ('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32),
        ('nfiles', ctypes.c_uint32), ('pgid', ctypes.c_uint32),
        ('jobc', ctypes.c_uint32), ('tdev', ctypes.c_uint32),
        ('tpgid', ctypes.c_uint32), ('nice', ctypes.c_int32),
        ('start_sec', ctypes.c_uint64), ('start_usec', ctypes.c_uint64)]


class Native:
    def __init__(self):
        if sys.platform != 'darwin' or ctypes.sizeof(BsdInfo) != 136:
            raise RuntimeError('unsupported native ABI')
        self.lib = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
        self.lib.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                                         ctypes.c_void_p, ctypes.c_int]
        self.lib.proc_pidinfo.restype = ctypes.c_int

    def info(self, pid):
        result = BsdInfo()
        ctypes.set_errno(0)
        count = self.lib.proc_pidinfo(pid, 3, 0, ctypes.byref(result), ctypes.sizeof(result))
        if count <= 0 and ctypes.get_errno() == errno.ESRCH:
            return None
        if count != ctypes.sizeof(result) or result.pid != pid or not result.start_sec:
            raise RuntimeError('native identity unavailable')
        return (result.pid, result.start_sec, result.start_usec, result.ppid,
                result.pgid, result.status)


class OwnedTree:
    """Original Popen + native start identities; no externally supplied root PID."""
    def __init__(self, child):
        self.child, self.native = child, Native()
        root = self.native.info(child.pid)
        if root is None or root[3] != os.getpid() or root[4] != child.pid:
            raise RuntimeError('original child ownership not established')
        self.root = root
        self.known = {child.pid: root}
        self.scan()

    def scan(self):
        root = self.native.info(self.child.pid)
        if root is not None and root[:3] != self.root[:3]:
            raise RuntimeError('root identity changed')
        if root is not None and root[3] != os.getpid():
            raise RuntimeError('root parent changed')
        # Query only PID/parent/group, never command arguments or user content.
        rows = [tuple(map(int, line.split())) for line in
                fixed(['/bin/ps', '-axo', 'pid=,ppid=,pgid=']).splitlines()]
        members = {pid for pid, parent, group in rows if group == self.child.pid}
        found = {self.child.pid}
        for _ in range(32):
            grown = found | {pid for pid, parent, group in rows if parent in found}
            if grown == found:
                break
            found = grown
        if len(found | members | self.known.keys()) > 32:
            raise RuntimeError('process tree limit')
        for pid in found | members | self.known.keys():
            info = self.native.info(pid)
            if info is None:
                continue
            if pid in self.known and info[:3] != self.known[pid][:3]:
                raise RuntimeError('descendant PID recycled')
            if pid not in self.known:
                if info[3] not in found or info[4] != self.child.pid or root is None:
                    raise RuntimeError('unwitnessed or escaped process')
                self.known[pid] = info
            if info[4] != self.child.pid:
                raise RuntimeError('process escaped owned group')
        return [pid for pid in self.known if (i := self.native.info(pid)) is not None and i[5] != 5]

    def rss(self, live):
        if not live:
            raise RuntimeError('runtime tree exited')
        raw = fixed(['/bin/ps', '-p', ','.join(map(str, live)), '-o', 'pid=,rss='])
        values = {}
        for line in raw.splitlines():
            pid, rss = map(int, line.split())
            current = self.native.info(pid)
            if pid not in self.known or current is None or current[:3] != self.known[pid][:3]:
                raise RuntimeError('RSS identity changed')
            values[str(pid)] = rss
        if set(map(int, values)) != set(live):
            raise RuntimeError('incomplete RSS sample')
        return values

    def close(self):
        # Validate complete group while original root PID still belongs to us.
        live = self.scan()
        root = self.native.info(self.child.pid)
        if root and root[:3] == self.root[:3] and root[5] != 5:
            os.kill(self.child.pid, signal.SIGTERM)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            live = self.scan()
            if not live:
                break
            time.sleep(.1)
        for sig in (signal.SIGTERM, signal.SIGKILL):
            for pid in self.scan():
                info = self.native.info(pid)
                if info is None:
                    continue
                if info[:3] != self.known[pid][:3] or info[4] != self.child.pid:
                    raise RuntimeError('cleanup identity mismatch; not signalled')
                os.kill(pid, sig)
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and self.scan():
                time.sleep(.1)
        self.child.wait(timeout=2)
        if self.scan():
            raise RuntimeError('owned group cleanup unconfirmed')
        return {'root_exit_code': self.child.returncode, 'processes_observed': len(self.known),
                'tree_stopped': True}

    def emergency_stop_known(self):
        """Best effort on failure; never upgrades uncertainty to confirmed cleanup."""
        for pid, known in reversed(list(self.known.items())):
            try:
                info = self.native.info(pid)
                if info is not None and info[:3] == known[:3] and info[4] == self.child.pid:
                    os.kill(pid, signal.SIGKILL)
            except (OSError, RuntimeError):
                pass
        try:
            self.child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass


def profile(source):
    # Paths are operator-created, not model input. JSON string quoting is SBPL compatible.
    return '(version 1)\n(allow default)\n(deny file-write* (subpath ' + json.dumps(str(source)) + '))\n'


def protect_probe(root):
    protected = root / 'protected-fixture'
    protected.mkdir()
    original = protected / 'sentinel'
    original.write_text('unchanged')
    alias = root / 'fixture-link'
    alias.symlink_to(original)
    sb = root / 'fixture.sb'
    sb.write_text(profile(protected))
    code = ("import pathlib,sys; a,b=map(pathlib.Path,sys.argv[1:]); "
            "assert a.read_text()==b.read_text()=='unchanged'\n"
            "for p in (a,b):\n"
            " try: p.write_text('forbidden')\n"
            " except PermissionError: pass\n"
            " else: raise RuntimeError('sandbox did not protect source')\n")
    subprocess.run(['/usr/bin/sandbox-exec', '-f', str(sb), sys.executable, '-I', '-S',
                    '-c', code, str(original), str(alias)], check=True, timeout=5,
                   env={'HOME': str(root), 'PATH': '/usr/bin:/bin'},
                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
    if original.read_text() != 'unchanged':
        raise RuntimeError('protection fixture modified; stop')


def stage(store, source, tag, expected):
    model, version = tag.split(':')
    manifest = source / 'manifests-v2/ollama.com/library' / model / version
    raw = manifest.read_bytes()
    if len(raw) > 1024 * 1024 or hashlib.sha256(raw).hexdigest() != expected:
        raise RuntimeError('manifest identity mismatch')
    data = json.loads(raw)
    refs = [data['config']] + data['layers']
    if len(refs) > 4096:
        raise RuntimeError('manifest reference limit')
    (store / 'blobs').mkdir(parents=True)
    refs.append({'digest': 'sha256:' + expected, 'size': len(raw)})
    for ref in refs:
        if not re.fullmatch(r'sha256:[0-9a-f]{64}', ref['digest']):
            raise RuntimeError('invalid blob reference')
        name = ref['digest'].replace(':', '-')
        src = source / 'blobs' / name
        if src.is_symlink() or src.stat().st_size != ref['size']:
            raise RuntimeError('blob provenance/size mismatch')
        link = store / 'blobs' / name
        if not link.exists():
            link.symlink_to(src)
    target = store / 'manifests-v2/ollama.com/library' / model / version
    target.parent.mkdir(parents=True)
    # A private tag copy avoids any migration updating the shared tag link.
    target.write_bytes(raw)


def http(port, path, body=None, timeout=3):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    request = urllib.request.Request('http://127.0.0.1:' + str(port) + path,
                                    data=None if body is None else json.dumps(body).encode(),
                                    headers={'Content-Type': 'application/json'})
    with opener.open(request, timeout=timeout) as response:
        raw = response.read(LIMIT + 1)
    if len(raw) > LIMIT:
        raise RuntimeError('HTTP response limit')
    return json.loads(raw)


def request_body(tag):
    if tag not in TAGS:
        raise ValueError('tag not authorized')
    return {'model': tag, 'messages': [{'role': 'user', 'content': 'Responde exactamente SEIYA_OK, sin explicaciones.'}],
            'stream': False, 'think': False, 'keep_alive': 0,
            'options': {'num_ctx': 4096, 'num_predict': 256, 'temperature': 0}}


def listeners(port):
    result = subprocess.run(['/usr/sbin/lsof', '-nP', '-t', '-iTCP:' + str(port), '-sTCP:LISTEN'],
                            capture_output=True, text=True, timeout=2)
    if result.returncode not in (0, 1):
        raise RuntimeError('listener ownership unavailable')
    return set(map(int, result.stdout.split()))


def run_one(root, checker, tag=None, calibration=False, qwen_calibration=False,
            critical_calibration=False, observe_memory=False):
    if observe_memory and (not qwen_calibration or critical_calibration or calibration):
        raise ValueError('observation requires its separate Qwen selector')
    if critical_calibration and not qwen_calibration:
        raise ValueError('critical calibration requires its Qwen selector')
    if calibration and qwen_calibration:
        raise ValueError('calibration approvals cannot be combined')
    if calibration and tag != TAGS[0]:
        raise ValueError('warning calibration authorized only for Gemma E2B')
    if qwen_calibration and tag != TAGS[1]:
        raise ValueError('Qwen warning calibration authorized only for selected Qwen')
    root.mkdir(mode=0o700)
    for name in ('home', 'tmp', 'models'):
        (root / name).mkdir(mode=0o700)
    source = (Path.home() / '.ollama').resolve()
    if tag:
        stage(root / 'models', source, tag, DIGESTS[TAGS.index(tag)])
    sb = root / 'readonly.sb'
    sb.write_text(profile(source))
    with socket.socket() as reserve:
        reserve.bind(('127.0.0.1', 0))
        port = reserve.getsockname()[1]
    if port in (8010, 11434):
        raise RuntimeError('reserved service port')
    env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LC_ALL': 'C',
           'HOME': str(root / 'home'), 'TMPDIR': str(root / 'tmp'),
           'OLLAMA_MODELS': str(root / 'models'), 'OLLAMA_HOST': '127.0.0.1:' + str(port),
           'OLLAMA_NO_CLOUD': '1', 'OLLAMA_NOPRUNE': '1', 'OLLAMA_MAX_LOADED_MODELS': '1',
           'OLLAMA_NUM_PARALLEL': '1', 'OLLAMA_CONTEXT_LENGTH': '4096',
           'OLLAMA_KEEP_ALIVE': '0', 'OLLAMA_LOAD_TIMEOUT': '120s'}
    report = {'tag': tag, 'model_inference_requested': False, 'port': port,
              'mode': 'ADR0039-single-model' if tag else 'private-empty-store', 'samples': []}
    if calibration:
        report['mode'] = 'ADR0040-e2b-warning-calibration'
    if qwen_calibration:
        report['mode'] = 'ADR0041-qwen-warning-calibration'
    if critical_calibration:
        report['mode'] = 'ADR0042-qwen-highpressure-calibration'
    if observe_memory:
        report['mode'] = 'ADR0043-qwen-memory-observation-calibration'
    child = tree = worker = None
    error = None
    try:
        baseline = memory_sample(observe_memory)
        report['preflight'] = baseline
        if not observe_memory and not admitted(checker, baseline, baseline['swap_bytes']):
            raise RuntimeError('preflight pressure policy denied')
        if http(11434, '/api/ps')['models']:
            raise RuntimeError('shared runtime not empty; no adoption/unload')
        with (root / 'server.log').open('xb') as log:
            child = subprocess.Popen(['/usr/bin/sandbox-exec', '-f', str(sb), str(OLLAMA), 'serve'],
                                     env=env, cwd=root, stdin=subprocess.DEVNULL,
                                     stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            tree = OwnedTree(child)
            ready = time.monotonic() + 15
            while True:
                owners = listeners(port)
                if owners:
                    if owners != {child.pid}:
                        raise RuntimeError('foreign listener; not adopted')
                    break
                tree.scan()
                if time.monotonic() > ready or child.poll() is not None:
                    raise RuntimeError('private server readiness failed')
                time.sleep(.1)
            if http(port, '/api/version')['version'] != '0.40.14' or http(port, '/api/ps')['models']:
                raise RuntimeError('private version/residency mismatch')
            # /api/tags can exceed the inference frame limit with full manifests;
            # do not fetch it for a staged model. Empty-store smoke stays bounded.
            if not tag:
                if http(port, '/api/tags')['models']:
                    raise RuntimeError('empty smoke unexpectedly has tags')
            else:
                sample = memory_sample(observe_memory)
                if ((not observe_memory and not admitted(checker, sample, baseline['swap_bytes']))
                        or http(11434, '/api/ps')['models']):
                    raise RuntimeError('fresh pre-load gate failed')
                outcome = {}

                def generate():
                    try:
                        outcome['response'] = http(port, '/api/chat', request_body(tag), timeout=120)
                    except Exception as exc:
                        outcome['error'] = type(exc).__name__ + ': ' + str(exc)[:256]

                start = time.monotonic()
                window = (WarningWindow(checker, baseline['swap_bytes'], start, qwen=qwen_calibration,
                                        critical=critical_calibration)
                          if (calibration or qwen_calibration) and not observe_memory else None)
                report['model_inference_requested'] = True
                worker = threading.Thread(target=generate, daemon=True)
                worker.start()
                while True:
                    live = tree.scan()
                    sample = memory_sample(observe_memory)
                    sample['elapsed_seconds'] = round(time.monotonic() - start, 3)
                    sample['owned_live_processes'] = len(live)
                    sample['rss_kib_by_pid_not_additive'] = tree.rss(live)
                    report['samples'].append(sample)
                    if observe_memory:
                        decision = observation(checker, sample, baseline['swap_bytes'])
                        sample['calibration_guard'] = decision
                        allowed = decision['continue'] is True
                    elif window is not None:
                        decision = window.observe(sample, time.monotonic())
                        sample['calibration_guard'] = decision
                        allowed = decision['allowed'] is True
                    else:
                        allowed = admitted(checker, sample, baseline['swap_bytes'])
                    if not allowed:
                        raise RuntimeError('pressure/swap guard stopped calibration')
                    if time.monotonic() - start >= 120:
                        raise RuntimeError('120-second inference deadline')
                    if not worker.is_alive():
                        break
                    time.sleep(.25)
                report['elapsed_seconds'] = round(time.monotonic() - start, 3)
                if 'error' in outcome:
                    raise RuntimeError(outcome['error'])
                report['response'] = outcome['response']
                report['validation'] = policy(checker, {'operation': 'response', 'tag': tag,
                                                         'response': outcome['response']})
                if report['validation']['valid'] is not True:
                    raise RuntimeError('response validation failed; no next model')
    except Exception as exc:
        error = type(exc).__name__ + ': ' + str(exc)[:512]
    finally:
        if tree is not None:
            try:
                report['cleanup'] = tree.close()
                report['cleanup']['port_closed'] = not listeners(port)
                if not report['cleanup']['port_closed']:
                    raise RuntimeError('port cleanup unconfirmed')
            except Exception as exc:
                tree.emergency_stop_known()
                report['cleanup_uncertain'] = type(exc).__name__ + ': ' + str(exc)[:256]
                error = error or 'cleanup unconfirmed'
        elif child is not None:
            # No tree ownership: only the original unreaped handle may be killed.
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
            error = error or 'tree ownership unavailable'
        if worker is not None:
            worker.join(timeout=3)
            if worker.is_alive():
                report['cleanup_uncertain'] = 'HTTP thread still alive'
                error = error or 'HTTP completion unconfirmed'
        report['error'] = error
        (root / 'evidence.json').write_text(json.dumps(report, indent=2))
    if error:
        raise RuntimeError(error + '; evidence=' + str(root))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=('empty', 'models', 'gemma-calibration', 'qwen-calibration',
                                         'qwen-critical-calibration', 'qwen-observe-calibration'), required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--checker', type=Path, required=True)
    parser.add_argument('--approval', required=True)
    args = parser.parse_args()
    expected = approval_for_mode(args.mode)
    if args.approval != expected or sys.platform != 'darwin':
        parser.error('explicit approval/native macOS required')
    output = args.output
    if not output.is_absolute() or output.parent.resolve() != BASE.resolve():
        parser.error('output must be a new direct child of approved OpenCode temp')
    output.mkdir(mode=0o700)  # no resume/overwrite/retry
    checker = args.checker.resolve(strict=True)
    fd = os.open(LOCK, os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
    with os.fdopen(fd, 'r+') as lease:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        protect_probe(output)
        # Own-tree cleanup exercised on a synthetic parent+child before Ollama.
        with (output / 'helper.log').open('xb') as log:
            helper = subprocess.Popen([sys.executable, '-I', '-S', '-c',
                "import subprocess,time;subprocess.Popen(['/bin/sleep','60']);time.sleep(60)"],
                stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
            tree = OwnedTree(helper)
            try:
                end = time.monotonic() + 3
                while len(tree.known) < 2 and time.monotonic() < end:
                    tree.scan()
                    time.sleep(.05)
                result = tree.close()
            except BaseException:
                tree.emergency_stop_known()
                raise
            (output / 'helper-evidence.json').write_text(json.dumps(result, indent=2))
            if result['processes_observed'] != 2:
                raise RuntimeError('synthetic child cleanup not exercised')
        tags = tags_for_mode(args.mode)
        reports = []
        for index, tag in enumerate(tags):
            report = run_one(output / ('run-' + str(index)), checker, tag,
                             calibration=args.mode == 'gemma-calibration',
                             qwen_calibration=args.mode in ('qwen-calibration', 'qwen-critical-calibration',
                                                           'qwen-observe-calibration'),
                             critical_calibration=args.mode == 'qwen-critical-calibration',
                             observe_memory=args.mode == 'qwen-observe-calibration')
            reports.append({k: v for k, v in report.items() if k not in ('samples', 'response')})
            print(json.dumps(reports[-1]), flush=True)
        (output / 'summary.json').write_text(json.dumps(reports, indent=2))
    with LOCK.open('r+') as verify:
        fcntl.flock(verify, fcntl.LOCK_EX | fcntl.LOCK_NB)
    print('Confirmed cleanup and lease reacquisition; evidence=' + str(output))


def approval_for_mode(mode):
    return {'empty': 'ADR0039-sequential-120s-256tokens',
            'models': 'ADR0039-sequential-120s-256tokens',
            'gemma-calibration': 'ADR0040-gemma-warning10s-120s-256tokens',
            'qwen-calibration': 'ADR0041-qwen-warning10s-120s-256tokens',
            'qwen-critical-calibration': 'ADR0042-qwen-highpressure10s-120s-256tokens',
            'qwen-observe-calibration': 'ADR0043-qwen-memory-observe-120s-256tokens'}[mode]


def tags_for_mode(mode):
    return {'empty': (None,), 'models': TAGS,
            'gemma-calibration': (TAGS[0],), 'qwen-calibration': (TAGS[1],),
            'qwen-critical-calibration': (TAGS[1],), 'qwen-observe-calibration': (TAGS[1],)}[mode]


if __name__ == '__main__':
    main()
