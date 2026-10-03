"""The Gothic 3 tab in the 3D view's sidebar."""

import bpy


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
        from . import rig
        rig.draw_panel(self.layout, context)


def register():
    bpy.utils.register_class(G3_PT_main)


def unregister():
    bpy.utils.unregister_class(G3_PT_main)
