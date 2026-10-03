"""The hand-over to gothic3-core.exe: one subprocess per call, one JSON value back on stdout."""

import json
import os
import subprocess
import tempfile

from .prefs import prefs

_HERE = os.path.dirname(os.path.realpath(__file__))
# A packaged add-on ships the exe in bin/; a development checkout finds the cargo build next door.
_CANDIDATES = (
    os.path.join(_HERE, "bin", "gothic3-core.exe"),
    os.path.normpath(os.path.join(_HERE, "..", "..", "core", "target", "release", "gothic3-core.exe")),
)


class CoreError(RuntimeError):
    pass


def exe():
    p = prefs().core_exe
    if p:
        # Never start anything else from here (a game exe picked by mistake would launch the game).
        if os.path.basename(p).lower() != "gothic3-core.exe":
            raise CoreError(f"Ядро має бути gothic3-core.exe, а вказано {os.path.basename(p)} — очистіть поле в налаштуваннях аддона")
        if not os.path.isfile(p):
            raise CoreError(f"gothic3-core.exe не знайдено: {p}")
        return p
    for c in _CANDIDATES:
        if os.path.isfile(c):
            return c
    raise CoreError("gothic3-core.exe не знайдено — перевстановіть аддон")


def cache_dir():
    d = prefs().cache_dir or os.path.join(tempfile.gettempdir(), "gothic3_impexp")
    os.makedirs(d, exist_ok=True)
    return d


def game_dir():
    d = prefs().game_dir
    if not d or not os.path.isdir(os.path.join(d, "Data")):
        raise CoreError(f"У папці гри немає Data\\: {d!r} — вкажіть папку Gothic 3 в налаштуваннях аддона")
    return d


def run(*args):
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    r = subprocess.run([exe(), *args], capture_output=True, text=True, encoding="utf-8", creationflags=flags)
    if r.returncode != 0:
        raise CoreError(r.stderr.strip() or f"gothic3-core завершився з кодом {r.returncode}")
    return json.loads(r.stdout)
