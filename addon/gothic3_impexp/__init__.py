"""Gothic 3 ImpExp: Blender import and export for Gothic 3 (Piranha Bytes, Genome engine).

Blender side only: UI, preferences, menu entries and the hand-over to the native core (``gothic3-core``, Rust;
reads the game's .pak archives and patch volumes directly), which converts through OBJ and glTF so Blender's own
importers and exporters do the mesh, skeleton and animation work; it writes Gothic 3 files and installs
them as patch volumes over the game's archives.

Copyright (C) 2026 UniLinkIF. GPL-3.0-or-later with additional terms (section 7): see NOTICE.
"""

bl_info = {
    "name": "Gothic 3 ImpExp",
    "author": "UniLinkIF",
    "version": (0, 9, 1),
    "blender": (5, 1, 0),
    "location": "File > Import / Export > Gothic 3",
    "description": "Import and export Gothic 3 models, characters, animations and collision",
    "category": "Import-Export",
}

from . import prefs, import_mesh, import_actor, export_mesh, export_motion, export_actor, rig, effects, ui, menus  # noqa: E402

_modules = (prefs, import_mesh, import_actor, export_mesh, export_motion, export_actor, rig, effects, ui, menus)


def register():
    for m in _modules:
        m.register()


def unregister():
    for m in reversed(_modules):
        m.unregister()
