"""Adaptador local de evaluación: un modelo fijado y una generación a la vez."""
import base64
import json
import os
import struct
import subprocess
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Lock

ROOT = Path(__file__).resolve().parent
ID = "qwen-image-2.1-turbo-q4"
ASSETS = ROOT / ID
CLI = ROOT / "runtime/build/bin/sd-cli"
WEIGHTS = [ASSETS / n for n in ("diffusion-q4.gguf", "text-encoder-q4.gguf", "vae-bf16.safetensors")]
slot = Lock()


class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path != "/v1/models":
            return self.reply(404, {})
        self.reply(200, {"data": [{"id": ID, "object": "model"}]})

    def do_POST(self):
        if self.path != "/v1/images/generations":
            return self.reply(404, {})
        if self.headers.get("Origin") or self.headers.get_content_type() != "application/json":
            return self.reply(403, {})
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 32768:
                return self.reply(413, {})
            req = json.loads(self.rfile.read(length))
            if not isinstance(req, dict) or (req.get("model"), req.get("n"), req.get("size"), req.get("response_format")) != (ID, 1, "512x512", "b64_json"):
                return self.reply(400, {})
            prompt, seed = req["prompt"], req["seed"]
            if not isinstance(prompt, str) or not prompt.strip() or "\0" in prompt or type(seed) is not int or not 0 <= seed < 2**32:
                return self.reply(400, {})
        except (ValueError, KeyError):
            return self.reply(400, {})
        if not slot.acquire(blocking=False):
            return self.reply(409, {})
        try:
            with tempfile.TemporaryDirectory(dir=ROOT) as tmp:
                out = Path(tmp) / "result.png"
                subprocess.run([str(CLI), "--diffusion-model", str(WEIGHTS[0]), "--llm", str(WEIGHTS[1]),
                                "--vae", str(WEIGHTS[2]), "--sigmas", "qwen-image-2.1-turbo", "--steps", "8",
                                "--cfg-scale", "1", "--sampling-method", "euler", "-W", "512", "-H", "512",
                                "-t", "4", "-s", str(seed), "-p", prompt, "-o", str(out)],
                               cwd=tmp, check=True, timeout=560, stdin=subprocess.DEVNULL)
                if out.stat().st_size > 16*1024*1024:
                    return self.reply(502, {})
                png = out.read_bytes()
                if png[:8] != b"\x89PNG\r\n\x1a\n" or png[16:24] != struct.pack(">II",512,512):
                    return self.reply(502, {})
                self.reply(200, {"model": ID, "data": [{"b64_json": base64.b64encode(png).decode()}]})
        except (OSError, subprocess.SubprocessError):
            self.reply(503, {"error": "image_generation_failed"})
        finally:
            slot.release()


if __name__ == "__main__":
    if not os.access(CLI, os.X_OK) or not all(p.is_file() for p in WEIGHTS):
        raise SystemExit("Falta sd-cli o alguno de los tres pesos verificados.")
    ThreadingHTTPServer(("127.0.0.1", 21012), Handler).serve_forever()
