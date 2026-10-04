"""The Gothic 3 tab in the 3D view's sidebar: every import and export, the animation rig, and the mods installed
from Blender with a button to remove each."""

import bpy
from bpy.props import StringProperty

from . import core

_installed = None


def refresh():
    global _installed
    try:
        _installed = core.run("installed", core.game_dir())
    except core.CoreError:
        _installed = []


class G3_OT_refresh_mods(bpy.types.Operator):
    bl_idname = "gothic3.refresh_mods"
    bl_label = "Оновити список модів"

    def execute(self, context):
        refresh()
        return {"FINISHED"}


class G3_OT_uninstall_mod(bpy.types.Operator):
    bl_idname = "gothic3.uninstall_mod"
    bl_label = "Прибрати мод"
    bl_description = "Прибрати цей мод з гри (томи-латки перебудовуються, архіви гри не змінюються)"
    mod: StringProperty()

    def invoke(self, context, event):
        return context.window_manager.invoke_confirm(self, event)

    def execute(self, context):
        try:
            core.run("uninstall", core.game_dir(), self.mod)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        refresh()
        self.report({"INFO"}, f"{self.mod}: прибрано")
        return {"FINISHED"}


class G3_PT_main(bpy.types.Panel):
    bl_label = "Gothic 3"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Gothic 3"

    def draw(self, context):
        col = self.layout.column(align=True)
        col.operator("gothic3.import_mesh", icon="IMPORT")
        col.operator("gothic3.import_character", icon="ARMATURE_DATA")
        col.operator("gothic3.import_motion", icon="ACTION")
        col.operator("gothic3.import_collision", icon="MOD_PHYSICS")
        col.separator()
        col.operator("gothic3.export_mesh", icon="EXPORT")
        from . import rig
        rig.draw_panel(self.layout, context)
        box = self.layout.box()
        row = box.row()
        row.label(text="Встановлені моди", icon="PACKAGE")
        row.operator("gothic3.refresh_mods", text="", icon="FILE_REFRESH")
        if _installed is None:
            box.label(text="Натисніть ⟳, щоб прочитати")
        elif not _installed:
            box.label(text="Немає")
        else:
            for name, files in _installed:
                r = box.row()
                r.label(text=f"{name} ({len(files)})")
                r.operator("gothic3.uninstall_mod", text="", icon="X").mod = name


_classes = (G3_OT_refresh_mods, G3_OT_uninstall_mod, G3_PT_main)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
