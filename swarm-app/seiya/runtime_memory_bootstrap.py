# Appended to runtime_memory.py by the opt-in Rust launcher, not imported.
# The entrypoint still runs on the main thread in this same process.
import json
import runpy
import sys
import threading


class _LoadedCore:
    def __getattr__(self, name):
        core = sys.modules.get("mlx.core")
        return getattr(core, name, None) if core is not None else None


def _memory_peer(reader, writer, sampler):
    try:
        for _ in range(1024):
            if reader.readline(8) != b"sample\n":
                return
            report = json.dumps(sampler.sample(), separators=(",", ":")).encode("utf-8")
            if len(report) > MAX_REPORT_BYTES:
                return
            writer.write(report + b"\n")
            writer.flush()
    finally:
        reader.close()
        writer.close()


def _run_instrumented():
    # Save private socket descriptors before redirecting application stdio.
    reader = os.fdopen(os.dup(0), "rb", buffering=0)
    writer = os.fdopen(os.dup(1), "wb")
    with open(os.devnull, "rb") as null:
        os.dup2(null.fileno(), 0)
    os.dup2(2, 1)
    sampler = RuntimeMemorySampler(_LoadedCore())
    threading.Thread(target=_memory_peer, args=(reader, writer, sampler), daemon=True).start()
    sys.argv = sys.argv[1:]
    runpy.run_path(sys.argv[0], run_name="__main__")


if __name__ == "__main__":
    _run_instrumented()
