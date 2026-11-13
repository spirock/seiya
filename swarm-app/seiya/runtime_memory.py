"""Opt-in in-runtime counters. Never imports MLX or loads a model.

Pass an already imported mlx.core from an owned, isolated runtime only.
These sequential allocator readings are not RSS or total Metal memory.
"""
import os
import time


MAX_BYTES = (1 << 64) - 1
MAX_REPORT_BYTES = 4096


def _unavailable(reason):
    return {"value": None, "reason": reason}


def _counter(core, name):
    try:
        getter = getattr(core, name, None)
        if not callable(getter):
            return _unavailable("missing_api")
        value = getter()
    except Exception:
        # Exception text can contain local paths/data; never export it.
        return _unavailable("probe_failed")
    if type(value) is not int or not 0 <= value <= MAX_BYTES:
        return _unavailable("invalid_value")
    return {"value": value, "reason": None}


class RuntimeMemorySampler:
    """One owner thread, constructed after fork inside the measured runtime.

    No API resets/synchronization; a stuck getter must be handled by the owning
    runtime supervisor, not by invoking this on the watchdog thread.
    """

    def __init__(self, core, *, clock=time.monotonic_ns):
        self._core = core
        self._clock = clock
        self._pid = os.getpid()
        self._epoch = clock()
        if type(self._epoch) is not int or self._epoch < 0:
            raise ValueError("invalid monotonic epoch")
        self._last = self._epoch
        self._sequence = 0

    def _elapsed(self):
        now = self._clock()
        if type(now) is not int or now < self._last:
            raise ValueError("invalid monotonic clock")
        elapsed = now - self._epoch
        if not 0 <= elapsed <= MAX_BYTES:
            raise ValueError("clock overflow")
        self._last = now
        return elapsed

    def sample(self):
        if os.getpid() != self._pid:
            raise ValueError("sampler inherited by another process")
        if self._sequence > MAX_BYTES:
            raise ValueError("sequence exhausted")
        start = self._elapsed()
        active = _counter(self._core, "get_active_memory")
        cache = _counter(self._core, "get_cache_memory")
        peak = _counter(self._core, "get_peak_memory")
        end = self._elapsed()
        report = {
            "version": "seiya-runtime-memory/1",
            "pid": self._pid,
            "sequence": self._sequence,
            "read_started_ns": start,
            "read_finished_ns": end,
            "peak_scope": "runtime_global_external_resets_unknown",
            "mlx_active_bytes": active,
            "mlx_cache_bytes": cache,
            "mlx_peak_bytes": peak,
            "metal_allocated_bytes": _unavailable("no_validated_probe"),
            "footprint_bytes": _unavailable("no_validated_probe"),
        }
        self._sequence += 1
        return report
