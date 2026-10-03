"""Gothic 3 ImpExp: Blender import for Gothic 3 (Piranha Bytes, Genome engine).

Blender side only: UI, preferences, menu entries and the hand-over to the native core (``gothic3-core``, Rust;
reads the game's .pak archives and patch volumes directly), which converts through OBJ and glTF so Blender's own
importers do the mesh, skeleton and animation work.
"""

bl_info = {
    "name": "Gothic 3 ImpExp",
    "author": "UniLinkIF",
    "version": (0, 1, 0),
    "blender": (5, 1, 0),
    "location": "File > Import > Gothic 3",
    "description": "Import Gothic 3 models, characters, animations and collision",
    "category": "Import-Export",
}

from . import prefs, import_mesh, import_actor, rig, ui, menus  # noqa: E402

_modules = (prefs, import_mesh, import_actor, rig, ui, menus)


def register():
    for m in _modules:
        m.register()


def unregister():
    for m in reversed(_modules):
        m.unregister()
