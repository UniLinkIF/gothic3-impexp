"""File > Export > Gothic 3 Motion: the active action of a Gothic 3 armature replaces one of the game's clips, or becomes
a new clip next to it (the game clip is then the template).

Blender's own glTF exporter samples the action (every frame, bone space); gothic3-core puts those keys into the
game's clip (its structure stays) and installs it as a mod or builds a package. By default the clip is the one the
action came from. IK and other constraints go in as their result, since the exporter samples the final pose.
Frame effects (footsteps, sounds) go in from the action's own list when it has one, else the clip's stay.
"""

import json
import os
import shutil
import tempfile

import bpy
from bpy.props import EnumProperty, IntProperty, StringProperty

from . import catalog, core, effects
from .import_actor import actor_clips


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


# The words the game makes clip names of (Game.dll / Script.dll tables, from the core), as enum items. Blender keeps
# only references to the item strings, so they live here.
_words = {}
WORD_PROPS = (("stance", "stance"), ("left", "left"), ("right", "right"), ("pose", "pose"), ("action_word", "action"),
              ("phase", "phase"), ("nio", "nio"), ("direction", "direction"))


def _word_items(field):
    def items(self, context):
        if field not in _words:
            try:
                got = core.run("clip-words")
            except core.CoreError:
                return [("None", "None", "")]
            for k, v in got.items():
                _words[k] = [(w, w, "") for w in v]
        return _words.get(field) or [("None", "None", "")]
    return items


def split_clip(name):
    """(actor, the eight words, variant) of a clip named the composed way, else None."""
    if "_%_" not in name:
        return None
    head = name.split("_%_", 1)[0].split("_")
    if len(head) != 10 or not head[9].isdigit():
        return None
    return head[0], head[1:9], int(head[9])


class G3_OT_export_motion(bpy.types.Operator):
    bl_idname = "gothic3.export_motion"
    bl_label = "Gothic 3 Motion → Mod (.xmot)"
    bl_description = "Активна дія скелета Gothic 3 замінює анімацію гри (за замовчуванням ту, з якої вона прийшла)"

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.CLIP_CATEGORIES))
    clip: StringProperty(name="Анімація гри", description="Яку анімацію замінити (або взяти за зразок для нової)", search=_search_clip, options={"SKIP_SAVE"})
    target: EnumProperty(name="Як", items=(
        ("replace", "Замінити", "Замінити цю анімацію гри"),
        ("variant", "Новим варіантом", "Нова анімація під наступним вільним номером варіанта тієї ж дії (вовк: Ambient_Loop 01–05 → 06)"),
        ("words", "Зі слів гри", "Нова назва зі слів, з яких гра складає назви анімацій (стійка, руки, поза, дія, фаза, напрям): іншої назви гра не складе й не зіграє"),
    ), default="replace", options={"SKIP_SAVE"})
    stance: EnumProperty(name="Стійка", items=_word_items("stance"), options={"SKIP_SAVE"})
    left: EnumProperty(name="Ліва рука", items=_word_items("left"), options={"SKIP_SAVE"})
    right: EnumProperty(name="Права рука", items=_word_items("right"), options={"SKIP_SAVE"})
    pose: EnumProperty(name="Поза", items=_word_items("pose"), options={"SKIP_SAVE"})
    action_word: EnumProperty(name="Дія", items=_word_items("action"), options={"SKIP_SAVE"})
    phase: EnumProperty(name="Фаза", items=_word_items("phase"), options={"SKIP_SAVE"})
    nio: EnumProperty(name="N / O / I", items=_word_items("nio"), options={"SKIP_SAVE"})
    direction: EnumProperty(name="Напрям", items=_word_items("direction"), options={"SKIP_SAVE"})
    variant: IntProperty(name="Варіант", min=0, max=99, default=0, options={"SKIP_SAVE"})
    mode: EnumProperty(name="Куди", items=core.MODES, default="mod")
    folder: StringProperty(name="Тека", subtype="DIR_PATH", description="Куди покласти файл мода чи пакет (типово: файл — у теку Mods гри)")
    title: StringProperty(name="Назва мода", description="Типово — назва анімації", options={"SKIP_SAVE"})

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE" and "g3_actor" in o and o.animation_data and o.animation_data.action

    def invoke(self, context, event):
        if not self.clip:
            self.clip = context.active_object.animation_data.action.get("g3_clip", "")
        self.fill_words()
        return context.window_manager.invoke_props_dialog(self, width=560)

    def fill_words(self):
        """The word lists start at the template clip's own words."""
        got = split_clip(self.clip)
        if not got:
            return
        for (prop, field), w in zip(WORD_PROPS, got[1]):
            match = next((i[0] for i in _word_items(field)(self, None) if i[0].lower() == w.lower()), None)
            if match:
                setattr(self, prop, match)
        self.variant = got[2]

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "category")
        col.prop(self, "clip")
        col.prop(self, "target", expand=True)
        if self.target == "words":
            box = col.box()
            for prop, _ in WORD_PROPS:
                box.prop(self, prop)
            box.prop(self, "variant")
            if not split_clip(self.clip):
                box.label(text="Ця анімація гри названа не так, як гра складає назви", icon="ERROR")
        col.prop(self, "mode", expand=True)
        if self.mode != "install":
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
            doc = {"clip": self.clip, "glb": glb, "variant": self.target == "variant", "effects": effects.as_times(context.scene, action)}
            if self.target == "words":
                doc["compose"] = {"words": [getattr(self, p) for p, _ in WORD_PROPS], "variant": self.variant}
            json.dump(doc, f)
        target = self.clip
        try:
            out, where = core.deliver("export-motion", spec, self.mode, target, self.title, self.folder)
            if self.mode == "install":
                from . import ui
                ui.refresh()
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        r = out["report"]
        what = f"нова, за зразком {r['template']}" if r["new"] else "замінено"
        if r.get("note"):
            self.report({"WARNING"} if "never" in r["note"] or "only" in r["note"] else {"INFO"}, f"{r['clip']}: {r['note']}")
        self.report({"INFO"}, f"{r['clip']} ({what}): {r['bones']} кісток, {r['keys']} ключів, {r['effects']} ефектів, {r['duration']:.2f} с — {action.name} → {where}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_motion)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_motion)
