"""File > Export > Gothic 3 Motion: the active action of a Gothic 3 armature replaces one of the game's clips, or becomes
a new clip next to it (the game clip is then the template).

Blender's own glTF exporter samples the action (every frame, bone space); gothic3-core puts those keys into the
game's clip (its structure stays) and installs it as a mod or builds a package. By default the clip is the one the
action came from. IK and other constraints go in as their result, since the exporter samples the final pose.
Frame effects (footsteps, sounds) go in from the action's own list when it has one, else the clip's stay.
"""

import json
import os
import tempfile

import bpy
from bpy.props import EnumProperty, StringProperty

from . import catalog, core, effects
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
    clip: StringProperty(name="Анімація гри", description="Яку анімацію замінити (або взяти за зразок для нової)", search=_search_clip)
    as_name: StringProperty(name="Нова назва", description="Порожньо — замінити анімацію гри; назва — нова анімація поруч (напр. Wolf_Stand_None_Fist_P0_Move_Run_N_Fwd_00_%_00_P0_401)")
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
        col.prop(self, "as_name")
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
        spec = os.path.join(tmp, "spec.json")
        with open(spec, "w", encoding="utf-8") as f:
            json.dump({"clip": self.clip, "glb": glb, "as_name": self.as_name or None, "effects": effects.as_times(context.scene, action)}, f)
        target = self.as_name or self.clip
        try:
            if self.mode == "package":
                folder = bpy.path.abspath(self.folder) if self.folder else os.path.join(prefs().mods_dir or tmp, target)
                out = core.run("export-motion", core.game_dir(), spec, "package", folder, self.title or target)
            else:
                out = core.run("export-motion", core.game_dir(), spec, "install", target)
                from . import ui
                ui.refresh()
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        r = out["report"]
        what = f"нова, за зразком {r['template']}" if r["new"] else "замінено"
        self.report({"INFO"}, f"{r['clip']} ({what}): {r['bones']} кісток, {r['keys']} ключів, {r['effects']} ефектів, {r['duration']:.2f} с — {action.name}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_motion)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_motion)
