"""File > Import / Export entries, one per Gothic 3 format."""

import bpy

IMPORTS = (
    "gothic3.import_mesh",
    "gothic3.import_character",
    "gothic3.import_motion",
    "gothic3.import_collision",
)


EXPORTS = (
    "gothic3.export_mesh",
)


def _menu_import(self, context):
    self.layout.separator()
    for idname in IMPORTS:
        self.layout.operator(idname)


def _menu_export(self, context):
    self.layout.separator()
    for idname in EXPORTS:
        self.layout.operator(idname)


def register():
    bpy.types.TOPBAR_MT_file_import.append(_menu_import)
    bpy.types.TOPBAR_MT_file_export.append(_menu_export)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(_menu_export)
    bpy.types.TOPBAR_MT_file_import.remove(_menu_import)
