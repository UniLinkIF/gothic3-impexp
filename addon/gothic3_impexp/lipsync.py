"""File > Import > Gothic 3 Lip-sync: a voice line of the game on the face of an animated head.

Gothic 3's faces move by bones, not shapes: the `G3_Head_…_Animated_…` heads have lip, jaw and brow bones, and a clip
of 15 poses next to each (`<head>_LipSync`). A voice line keeps a timeline of phones beside its sound; every phone
shows one of those poses. This puts a line on the selected character as an action on its face bones, and the line's
voice into the Video Sequencer, so it plays in sync. The character has to wear an animated head (Gothic 3 Character
with such a head brings its face bones along).
"""

import os

import bpy
from bpy.props import EnumProperty, StringProperty

from . import catalog, core
from .import_actor import _import_glb

LANGUAGES = (("English", "English", ""), ("German", "Deutsch", ""), ("Russian", "Русский", ""), ("Polish", "Polski", ""),
             ("French", "Français", ""), ("Italian", "Italiano", ""), ("Spanish", "Español", ""), ("Czech", "Čeština", ""))

_lines = {}


def _search_line(self, context, edit_text):
    try:
        key = (core.game_dir(), self.language)
        if key not in _lines:
            _lines[key] = core.run("lip-lines", key[0], self.language)
    except core.CoreError:
        return []
    words = edit_text.lower().split()
    out = []
    for n in _lines[key]:
        if all(w in n.lower() for w in words):
            out.append(n)
            if len(out) >= 300:
                break
    return out


def _search_head(self, context, edit_text):
    try:
        return [n for n in catalog.search(catalog.actors(), "head", edit_text) if "animated" in n.lower()]
    except core.CoreError:
        return []


def _fcurves(action):
    """The F-curves of `action` (the layered actions of Blender 4.4+ keep them per slot)."""
    try:
        from bpy_extras import anim_utils
        out = []
        for slot in action.slots:
            bag = anim_utils.action_get_channelbag_for_slot(action, slot)
            if bag is not None:
                out.append(bag.fcurves)
        if out:
            return out
    except (ImportError, AttributeError):
        pass
    return [action.fcurves]


class G3_OT_import_lipsync(bpy.types.Operator):
    bl_idname = "gothic3.import_lipsync"
    bl_label = "Gothic 3 Lip-sync (voice line)"
    bl_description = "Репліка гри на обличчі вибраного персонажа з анімованою головою: дія на кістках обличчя і голос у відеоредакторі"
    bl_options = {"REGISTER", "UNDO"}

    language: EnumProperty(name="Мова", items=LANGUAGES, default="English")
    line: StringProperty(name="Репліка", description="Назва репліки, напр. INFO_Ass_Elite_Hard_Bandaro…", search=_search_line, options={"SKIP_SAVE"})
    head: StringProperty(name="Голова", description="Анімована голова персонажа (G3_Head_…_Animated_…); порожньо — та, з якою його імпортовано", search=_search_head, options={"SKIP_SAVE"})

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE" and "g3_actor" in o

    def invoke(self, context, event):
        if not self.head:
            self.head = context.active_object.get("g3_head", "")
        return context.window_manager.invoke_props_dialog(self, width=520)

    def execute(self, context):
        arm = context.active_object
        head = self.head or arm.get("g3_head", "")
        if not head or not self.line:
            self.report({"ERROR"}, "Вкажіть репліку й анімовану голову")
            return {"CANCELLED"}
        try:
            out = core.run("lipsync", core.game_dir(), head, self.line, self.language, os.path.join(core.cache_dir(), "lipsync"))
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        missing = [b for b in out["face_bones"] if b not in arm.data.bones]
        if missing:
            self.report({"ERROR"}, f"У скелета немає кісток обличчя ({len(missing)}, напр. {missing[0]}): імпортуйте персонажа з анімованою головою {out['head']}")
            return {"CANCELLED"}
        obs, acts = _import_glb(out["glb"])
        datas = {o.data for o in obs if o.data is not None}
        for o in obs:
            bpy.data.objects.remove(o, do_unlink=True)
        for d in datas:
            if isinstance(d, bpy.types.Mesh) and d.users == 0:
                bpy.data.meshes.remove(d)
            elif isinstance(d, bpy.types.Armature) and d.users == 0:
                bpy.data.armatures.remove(d)
        if not acts:
            self.report({"ERROR"}, "Анімацію не прочитано")
            return {"CANCELLED"}
        act = acts[0]
        for a in acts[1:]:
            bpy.data.actions.remove(a)
        # Only the face moves: the clip's other bones are the pose the face hangs from, not the character's.
        face = {f'pose.bones["{b}"]' for b in out["face_bones"]}
        for fcs in _fcurves(act):
            for fc in [fc for fc in fcs if fc.data_path.rsplit(".", 1)[0] not in face]:
                fcs.remove(fc)
        act.name = f"{out['line']}_LipSync"
        act["g3_lipsync"] = out["line"]
        act.use_fake_user = True
        if arm.animation_data is None:
            arm.animation_data_create()
        arm.animation_data.action = act
        if hasattr(arm.animation_data, "action_slot") and act.slots:
            arm.animation_data.action_slot = act.slots[0]
        sc = context.scene
        fps = sc.render.fps / sc.render.fps_base
        start = sc.frame_start
        sc.frame_end = max(sc.frame_end, start + int(round(out["duration"] * fps)))
        if out.get("sound"):
            if sc.sequence_editor is None:
                sc.sequence_editor_create()
            se = sc.sequence_editor
            # An empty collection is false: ask by name (`sequences` before Blender 4.4).
            strips = se.strips if hasattr(se, "strips") else se.sequences
            strips.new_sound(out["line"], out["sound"], 1, start)
        self.report({"INFO"}, f"{out['line']}: {out['phones']} фонем, {out['duration']:.2f} с на {len(out['face_bones'])} кістках обличчя {out['head']}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_import_lipsync)


def unregister():
    bpy.utils.unregister_class(G3_OT_import_lipsync)
