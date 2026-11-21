"""Herramientas MCP de escritorio. Una conexión stdio conserva una pantalla X11."""
import contextlib
import io
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Annotated, Literal

from mcp.server.fastmcp import FastMCP, Image
from PIL import ImageGrab
from pydantic import Field

sys.path.insert(0, str(Path(__file__).resolve().parent / "upstream/app"))
from interpreter import Interpreter


class Discard:
    def put(self, _):
        pass


engine = Interpreter(Discard())
target_id = None
mcp = FastMCP("Seiya Desktop", instructions=(
    "Observa la pantalla, elige una acción según la petición del usuario y observa "
    "el resultado antes de continuar. Las coordenadas son píxeles de la captura."
))


def frame():
    buf = io.BytesIO()
    ImageGrab.grab(xdisplay=None).save(buf, format="PNG")
    return Image(data=buf.getvalue(), format="png")


def act(name, parameters):
    # stdout es el canal MCP; Open Interface escribe trazas de las pulsaciones.
    with contextlib.redirect_stdout(io.StringIO()):
        if not engine.process_command({"function": name, "parameters": parameters}):
            raise RuntimeError("El escritorio no confirmó la acción")
    time.sleep(0.25)
    return frame()


@mcp.tool()
def observe() -> Image:
    """Obtén una captura actual del escritorio de la sesión."""
    return frame()

def window_info():
    from Xlib import display
    d=display.Display()
    try:
        prop=d.screen().root.get_full_property(d.intern_atom("_NET_ACTIVE_WINDOW"),0)
        if prop is None or not prop.value[0]:
            return None, "", ()
        w=d.create_resource_object("window", int(prop.value[0]))
        title=w.get_full_property(d.intern_atom("_NET_WM_NAME"), d.intern_atom("UTF8_STRING"))
        return w.id, (bytes(title.value).decode("utf8", "replace") if title else str(w.get_wm_name() or "")), tuple(w.get_wm_class() or ())
    finally:
        d.close()

@mcp.tool()
def active_window() -> str:
    """Nombre de la ventana activa del escritorio de evaluación."""
    return window_info()[1]


@mcp.tool()
def open_app(app: Literal["Calculadora", "LibreOffice Writer"]) -> Image:
    """Abre una aplicación del escritorio de prueba; luego se opera desde su captura."""
    global target_id
    target_id = None
    profile = (Path(os.environ["HOME"])/"writer-profile").as_uri()
    apps = {"Calculadora":["/usr/bin/xcalc"], "LibreOffice Writer":["/usr/bin/libreoffice",f"-env:UserInstallation={profile}","--writer","--norestore","--nologo"]}
    process = subprocess.Popen(apps[app],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    deadline = time.monotonic()+20
    while time.monotonic()<deadline:
        time.sleep(.25)
        wid, title, classes = window_info()
        kind = tuple(c.lower() for c in classes)
        if (app=="Calculadora" and "xcalc" in kind) or (app=="LibreOffice Writer" and "libreoffice-writer" in kind and "writer" in title.lower()):
            target_id=wid
            return frame()
        if process.poll() not in (None,0):
            break
    raise RuntimeError(f"{app} no está lista o no ha obtenido el foco")


@mcp.tool()
def click(x: Annotated[int, Field(ge=0, lt=1024)], y: Annotated[int, Field(ge=0, lt=768)]) -> Image:
    """Pulsa el punto observado y devuelve una nueva captura."""
    return act("click", {"x": x, "y": y})


@mcp.tool()
def type_text(text: Annotated[str, Field(min_length=1, max_length=2048)]) -> Image:
    """Inserta texto Unicode en el campo enfocado usando el portapapeles de Xvfb."""
    wid,title,_=window_info()
    if not wid or title in ("","Aplicaciones de Seiya") or (target_id is not None and wid!=target_id):
        raise ValueError("Ninguna aplicación de destino tiene el foco")
    if any(not c.isprintable() and c not in "\n\t" for c in text):
        raise ValueError("Texto de control no permitido")
    subprocess.run(["/usr/bin/xclip","-selection","clipboard","-in"],input=text.encode(),
                   stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True,timeout=3)
    return act("hotkey", {"keys":["ctrl","v"]})


@mcp.tool()
def text_matches(expected: Annotated[str, Field(min_length=1, max_length=2048)]) -> bool:
    """Comprueba el texto real en Writer sin copiarlo fuera del escritorio de evaluación."""
    wid, _, classes = window_info()
    if not wid or wid != target_id or "libreoffice-writer" not in classes:
        return False
    # Vaciar el portapapeles: una copia fallida no puede devolver el pegado anterior.
    subprocess.run(["xclip","-selection","clipboard","-in"],input=b"",stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True,timeout=3)
    import pyautogui
    pyautogui.hotkey("ctrl","a")
    pyautogui.hotkey("ctrl","c")
    try:
        for _ in range(8):
            time.sleep(.1)
            read=subprocess.run(["xclip","-selection","clipboard","-out"],capture_output=True,timeout=2)
            if read.returncode==0 and read.stdout.decode("utf-8").rstrip("\n")==expected.rstrip("\n"):
                return True
        return False
    finally:
        pyautogui.press("right")


@mcp.tool()
def press(key: Literal["enter", "tab", "escape", "backspace", "left", "right", "up", "down"]) -> Image:
    """Pulsa una tecla de navegación y observa el resultado."""
    return act("press", {"key": key})


if __name__ == "__main__":
    mcp.run(transport="stdio")
