"""Rehearsal only: replace this owned process with the fixed Rust fixture.

No MLX imports, model reads, downloads or inference. Not a model entrypoint.
"""
import os
import sys

binary = os.environ["SEIYA_REHEARSAL_BINARY"]
os.execv(binary, [binary, "--recipe-fixture", *sys.argv[1:]])
