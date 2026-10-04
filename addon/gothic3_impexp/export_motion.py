"""File > Export > Gothic 3 Motion: the active action of a Gothic 3 armature replaces one of the game's clips.

Blender's own glTF exporter samples the action (every frame, bone space); gothic3-core puts those keys into the
game's clip (its structure stays) and installs it as a mod or builds a package. By default the clip is the one the
action came from. IK and other constraints go in as their result, since the exporter samples the final pose.
"""

import os
import tempfile

import bpy
from bpy.props import EnumProperty, StringProperty

from . import catalog, core
from .import_actor import actor_clips
from .prefs import prefs


def _search_clip(self, context, edit_text):
    arm = context.active_object
    actor = arm.get("g3_actor") if arm else None
    if not actor:
        return []
    try:
        words = edit_text.lower().split()
        return [c for c in actor_clips(actor) if (self.category == catalog.ALL or catalog.clip_category(c) == self.category) and all(w in c.lower() for w in words)][:300]
    except core.CoreError:
        return []


class G3_OT_export_motion(bpy.types.Operator):
    bl_idname = "gothic3.export_motion"
    bl_label = "Gothic 3 Motion → Mod (.xmot)"
    bl_description = "Активна дія скелета Gothic 3 замінює анімацію гри (за замовчуванням ту, з якої вона прийшла)"

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.CLIP_CATEGORIES))
    clip: StringProperty(name="Анімація гри", description="Яку анімацію замінити", search=_search_clip)
    mode: EnumProperty(name="Куди", items=(
        ("install", "Встановити в гру", "Одразу в гру (том-латка); прибрати — панель Gothic 3"),
        ("package", "Пакет мода", "Папка з томом, INSTALL.bat і ROLLBACK.bat"),
    ), default="install")
    folder: StringProperty(name="Папка пакета", subtype="DIR_PATH")
    title: StringProperty(name="Назва мода")

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE" and "g3_actor" in o and o.animation_data and o.animation_data.action

    def invoke(self, context, event):
        if not self.clip:
            self.clip = context.active_object.animation_data.action.get("g3_clip", "")
        return context.window_manager.invoke_props_dialog(self, width=560)

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "category")
        col.prop(self, "clip")
        col.prop(self, "mode", expand=True)
        if self.mode == "package":
            col.prop(self, "folder")
            col.prop(self, "title")

    def execute(self, context):
        if not self.clip:
            self.report({"ERROR"}, "Не вибрано анімацію гри, яку замінити")
            return {"CANCELLED"}
        arm = context.active_object
        action = arm.animation_data.action
        tmp = tempfile.mkdtemp(prefix="g3_motion_")
        glb = os.path.join(tmp, "motion.glb")
        sel = list(context.selected_objects)
        try:
            for o in sel:
                o.select_set(False)
            arm.select_set(True)
            bpy.ops.export_scene.gltf(
                filepath=glb, export_format="GLB", use_selection=True, export_animations=True,
                export_animation_mode="ACTIVE_ACTIONS", export_force_sampling=True, export_frame_step=1,
                export_anim_slide_to_zero=True, export_optimize_animation_size=False, export_skins=True,
                export_materials="NONE", export_yup=True, export_def_bones=False,
            )
        finally:
            arm.select_set(False)
            for o in sel:
                o.select_set(True)
        try:
            if self.mode == "package":
                folder = bpy.path.abspath(self.folder) if self.folder else os.path.join(prefs().mods_dir or tmp, self.clip)
                out = core.run("export-motion", core.game_dir(), self.clip, glb, "package", folder, self.title or self.clip)
            else:
                out = core.run("export-motion", core.game_dir(), self.clip, glb, "install", self.clip)
                from . import ui
                ui.refresh()
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        r = out["report"]
        self.report({"INFO"}, f"{r['clip']} (замінено): {r['bones']} кісток, {r['keys']} ключів, {r['duration']:.2f} с — {action.name}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_motion)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_motion)
