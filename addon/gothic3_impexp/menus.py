"""File > Import entries, one per Gothic 3 format."""

import bpy

IMPORTS = (
    "gothic3.import_mesh",
    "gothic3.import_character",
    "gothic3.import_motion",
    "gothic3.import_collision",
)


def _menu_import(self, context):
    self.layout.separator()
    for idname in IMPORTS:
        self.layout.operator(idname)


def register():
    bpy.types.TOPBAR_MT_file_import.append(_menu_import)


def unregister():
    bpy.types.TOPBAR_MT_file_import.remove(_menu_import)
