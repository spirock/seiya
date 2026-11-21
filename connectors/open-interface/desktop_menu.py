"""Menú gráfico inicial del escritorio de evaluación, independiente del protocolo MCP."""
import subprocess
import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk

window = Gtk.Window(title="Aplicaciones de Seiya")
window.set_default_size(520, 280)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
box.set_border_width(16)
window.add(box)
for title, argv in [("Calculadora", ["/usr/bin/xcalc"]),
                    ("LibreOffice Writer", ["/usr/bin/libreoffice", "--writer"])]:
    button = Gtk.Button(label=title)
    button.connect("clicked", lambda _, cmd=argv: subprocess.Popen(cmd))
    box.pack_start(button, True, True, 0)
window.connect("destroy", Gtk.main_quit)
window.show_all()
Gtk.main()
