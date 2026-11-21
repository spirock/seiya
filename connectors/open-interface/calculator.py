"""Protocolo calculator.v1: ejecutar la suma en Xvfb y observar el resultado con el modelo local."""
import argparse
import base64
import contextlib
import http.client
import io
import json
import re
import subprocess
import sys
import time
from pathlib import Path
from queue import SimpleQueue


def capture(a, b):
    from PIL import ImageGrab
    from Xlib import X, display
    sys.path.insert(0, str(Path(__file__).resolve().parent / "upstream/app"))
    from interpreter import Interpreter
    d = display.Display()
    calc = subprocess.Popen(["/usr/bin/xcalc", "-geometry", "+10+10", "-title", "Seiya calculator"],
                            stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            wins = [w for w in d.screen().root.query_tree().children
                    if w.get_wm_name() == "Seiya calculator" and w.get_attributes().map_state == X.IsViewable]
            if wins:
                break
            time.sleep(0.05)
        else:
            raise RuntimeError("calculator_not_ready")
        window = wins[0]
        window.set_input_focus(X.RevertToParent, X.CurrentTime)
        d.sync()
        keys = [*a, "add", *b, "enter"]
        with contextlib.redirect_stdout(sys.stderr):
            interpreter = Interpreter(SimpleQueue())
            for key in keys:
                if not interpreter.process_command({"function":"press","parameters":{"key":key}}):
                    raise RuntimeError("calculator_input_failed")
        time.sleep(.2)
        g = window.get_geometry()
        # Solo el visor superior de la calculadora, en una pantalla dedicada.
        crop = ImageGrab.grab(xdisplay=d.get_display_name()).crop((g.x, g.y, g.x+g.width, g.y+51))
        buf = io.BytesIO()
        crop.resize((crop.width*3,crop.height*3)).save(buf, format="PNG")
        return base64.b64encode(buf.getvalue()).decode("ascii")
    finally:
        calc.terminate()
        calc.wait(timeout=3)
        d.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("operation", choices=["calculator.v1"])
    parser.add_argument("a")
    parser.add_argument("b")
    parser.add_argument("port", type=int)
    parser.add_argument("model")
    args = parser.parse_args()
    if not all(re.fullmatch(r"[0-9]{1,6}", n) for n in [args.a,args.b]) or not 0 < args.port < 65536:
        parser.error("invalid calculator request")
    screenshot = capture(args.a,args.b)
    conn = http.client.HTTPConnection("127.0.0.1", args.port, timeout=180)
    try:
        conn.request("POST","/api/chat",json.dumps({"model":args.model,"stream":False,"think":False,
            "messages":[{"role":"user","content":"Read the number shown at the top right of this calculator display. Return just the decimal integer.","images":[screenshot]}],
            "options":{"temperature":0,"num_ctx":2048,"num_predict":32,"num_gpu":0,"num_thread":4}}),{"Content-Type":"application/json"})
        response = conn.getresponse()
        if response.status != 200:
            raise RuntimeError("visual_model_unavailable")
        observed = json.loads(response.read(65537))["message"]["content"].strip()
    finally:
        conn.close()
    if not re.fullmatch(r"[0-9]{1,7}", observed) or int(observed) != int(args.a)+int(args.b):
        raise RuntimeError("visual_result_not_confirmed")
    print(json.dumps({"operation":"calculator.v1","observed":observed}))


if __name__ == "__main__":
    main()
