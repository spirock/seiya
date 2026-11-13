"""Synthetic in-process counters. No MLX import, model, network or exec."""
import sys
import time
from types import SimpleNamespace

sys.modules["mlx.core"] = SimpleNamespace(
    get_active_memory=lambda: 1024,
    get_cache_memory=lambda: 0,
    get_peak_memory=lambda: 2048,
)
print("synthetic application stdout must never enter the telemetry channel", flush=True)
while True:
    time.sleep(0.01)
