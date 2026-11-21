#!/bin/sh
# Instalar el conector opcional para el usuario, fuera del proceso de Seiya.
set -eu
umask 077
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
base="$HOME/.local/share/seiya-open-interface"
rev=71df8a174572e1cb4e5da0eb9da58ea5485abbdf
for tool in git uv xvfb-run xcalc xauth xclip xfwm4 dbus-run-session; do command -v "$tool" >/dev/null || { echo "Falta $tool" >&2; exit 1; }; done
mkdir -p "$base"
if [ ! -d "$base/upstream" ]; then
    git init -q "$base/upstream"
    git -C "$base/upstream" fetch -q --depth=1 https://github.com/AmberSahdev/Open-Interface.git "$rev"
    git -C "$base/upstream" checkout -q --detach FETCH_HEAD
fi
test "$(git -C "$base/upstream" rev-parse HEAD)" = "$rev"
git -C "$base/upstream" diff --quiet HEAD -- app/interpreter.py
test -x "$base/venv/bin/python" || uv venv --python 3.12 "$base/venv"
uv pip install --python "$base/venv/bin/python" PyAutoGUI==0.9.54 pillow==12.3.0 python-xlib==0.33
uv pip install --python "$base/venv/bin/python" mcp==1.30.0
install -m 600 "$here/desktop_mcp.py" "$here/desktop_menu.py" "$here/desktop_goal.py" "$base/"
install -m 700 "$here/seiya-desktop-mcp" "$base/seiya-desktop-mcp"
install -m 600 "$here/calculator.py" "$base/calculator.py"
install -m 700 "$here/seiya-open-interface" "$base/seiya-open-interface"
install -m 600 "$here/catalog.json" "$base/catalog.json"
mkdir -p "$HOME/.local/bin"
install -m 700 "$here/seiya-asistido" "$HOME/.local/bin/seiya-asistido"
echo "Conector instalado: $base/seiya-open-interface"
