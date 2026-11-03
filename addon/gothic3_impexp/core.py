"""The hand-over to gothic3-core.exe: one subprocess per call, one JSON value back on stdout."""

import json
import os
import subprocess
import tempfile

from .prefs import prefs, texture_dirs

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
    # The modder's texture folders, for the core's texture lookup.
    env = dict(os.environ, G3IE_TEXTURE_DIRS=os.pathsep.join(texture_dirs()))
    r = subprocess.run([exe(), *args], capture_output=True, text=True, encoding="utf-8", creationflags=flags, env=env)
    if r.returncode != 0:
        raise CoreError(r.stderr.strip() or f"gothic3-core завершився з кодом {r.returncode}")
    return json.loads(r.stdout)


def find_texture(filename):
    """A picture called `filename` (or its stem with another picture extension; any case) in the texture folders of the
    preferences or their subfolders, four deep; None when there is none."""
    stem = os.path.splitext(os.path.basename(filename))[0].lower()
    want = os.path.basename(filename).lower()
    for top in texture_dirs():
        found = None
        for d, subs, files in os.walk(top):
            if d[len(top):].count(os.sep) >= 4:
                subs[:] = []
            subs.sort()
            for f in files:
                if f.lower() == want:
                    return os.path.join(d, f)
                if found is None and os.path.splitext(f)[0].lower() == stem and os.path.splitext(f)[1].lower() in (".png", ".tga", ".dds", ".jpg", ".jpeg"):
                    found = os.path.join(d, f)
        if found:
            return found
    return None


def package_dir(folder, name):
    """Where a mod package goes: the folder picked in the dialog, else the mods folder from the preferences, else the
    cache's `mods` folder (never the export's own temporary folder, which is deleted afterwards)."""
    import bpy
    if folder:
        return bpy.path.abspath(folder)
    return os.path.join(bpy.path.abspath(prefs().mods_dir) if prefs().mods_dir else os.path.join(cache_dir(), "mods"), name)


# Where an export goes. A mod file is the way to share: a mod launcher puts it into the game and takes it out again.
MODES = (
    ("mod", "Файл мода (.g3mod)", "Один файл у теку Mods гри (або вказану): лаунчер модів підключає й прибирає його; ним зручно ділитися"),
    ("install", "Встановити в гру", "Одразу в гру (том-латка поверх архівів); прибрати — панель Gothic 3"),
    ("package", "Пакет з INSTALL.bat", "Тека з томами, INSTALL.bat і ROLLBACK.bat — для гравців без лаунчера"),
)


def mod_file_name(name):
    """`name` as a file name Windows allows, with the mod file's extension."""
    clean = "".join("_" if c in '<>:"/\\|?*' or ord(c) < 32 else c for c in name.strip()).strip(". ")
    return (clean or "mod") + ".g3mod"


def deliver(command, spec, mode, name, title, folder):
    """Run the core's export `command` with the chosen delivery: (its output, where it went)."""
    if mode == "package":
        where = package_dir(folder, name)
        return run(command, game_dir(), spec, "package", where, title or name), where
    if mode == "mod":
        import bpy
        base = bpy.path.abspath(folder) if folder else os.path.join(game_dir(), "Mods")
        where = os.path.join(base, mod_file_name(title or name))
        return run(command, game_dir(), spec, "mod", where, title or name, prefs().author), where
    return run(command, game_dir(), spec, "install", name), "гру"
