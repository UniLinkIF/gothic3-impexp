"""File > Import > Gothic 3 Lip-sync: a voice line of the game on the face of an animated head.

Gothic 3's faces move by bones, not shapes: the `G3_Head_…_Animated_…` heads have lip, jaw and brow bones, and a clip
of 15 poses next to each (`<head>_LipSync`). A voice line keeps a timeline of phones beside its sound; every phone
shows one of those poses. This puts a line on the selected character as an action on its face bones, and the line's
voice into the Video Sequencer, so it plays in sync. The character has to wear an animated head (Gothic 3 Character
with such a head brings its face bones along).
"""

import json
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
        if out or not hasattr(action, "fcurves"):
            return out
    except (ImportError, AttributeError):
        pass
    # Before Blender 4.4 (no slots); an action without slots in 5.x has no F-curves at all.
    return [action.fcurves] if hasattr(action, "fcurves") else []


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
        act["g3_head"] = out["head"]
        act.use_fake_user = True
        sc = context.scene
        fps = sc.render.fps / sc.render.fps_base
        # The frame of the line's time 0 (the export reads times from there).
        first = min((k.co.x for fcs in _fcurves(act) for fc in fcs for k in fc.keyframe_points), default=float(sc.frame_start))
        act["g3_frame0"] = first - out["timeline"][0][0] * fps if out["timeline"] else first
        context.view_layer.objects.active = arm
        arm.select_set(True)
        if arm.animation_data is None:
            arm.animation_data_create()
        arm.animation_data.action = act
        if hasattr(arm.animation_data, "action_slot") and act.slots:
            arm.animation_data.action_slot = act.slots[0]
        start = int(round(act["g3_frame0"]))
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


def _channels(action):
    """{(bone, kind): [fcurve per component]} of `action`: rotation_quaternion and location of pose bones."""
    out = {}
    for fcs in _fcurves(action):
        for fc in fcs:
            if not fc.data_path.startswith('pose.bones["'):
                continue
            bone, prop = fc.data_path[12:].split('"]', 1)
            kind = prop.lstrip(".")
            if kind in ("rotation_quaternion", "location"):
                out.setdefault((bone, kind), {})[fc.array_index] = fc
    return {k: [v[i] for i in sorted(v)] for k, v in out.items() if len(v) == (4 if k[1] == "rotation_quaternion" else 3)}


def head_poses(head):
    """The head's lip-sync poses in Blender's bone space: {(bone, kind): [value per pose]} for the key arrays the game
    takes (at least 14 keys; Engine.dll `CreateLipSyncMotion` reads key number `phone` of each)."""
    clip = f"{head}_LipSync"
    listing = os.path.join(core.cache_dir(), "lipsync", "poses.txt")
    os.makedirs(os.path.dirname(listing), exist_ok=True)
    with open(listing, "w", encoding="utf-8") as f:
        f.write(clip)
    out = core.run("actor", core.game_dir(), head, os.path.join(core.cache_dir(), "lipsync", f"{head}_poses.glb"), "@" + listing, "1", "skeleton", "-")
    if not out["clips"]:
        raise core.CoreError(f"{head}: немає кліпу {clip} — це не анімована голова")
    obs, acts = _import_glb(out["glb"])
    datas = {o.data for o in obs if o.data is not None}
    for o in obs:
        bpy.data.objects.remove(o, do_unlink=True)
    for d in datas:
        if isinstance(d, bpy.types.Armature) and d.users == 0:
            bpy.data.armatures.remove(d)
        elif isinstance(d, bpy.types.Mesh) and d.users == 0:
            bpy.data.meshes.remove(d)
    try:
        poses = {}
        for key, fcs in _channels(acts[0]).items():
            n = min(len(fc.keyframe_points) for fc in fcs)
            if n >= 14:
                poses[key] = [[fc.keyframe_points[p].co.y for fc in fcs] for p in range(n)]
        return poses
    finally:
        for a in acts:
            bpy.data.actions.remove(a)


def phones_of(action, poses, fps, frame0):
    """(start s, pose) at every key of the face channels of `action`: the head's pose nearest to the face there (the
    game's lines use poses 1-13)."""
    chans = {k: fcs for k, fcs in _channels(action).items() if k in poses}
    if not chans:
        return []
    frames = sorted({round(k.co.x, 4) for fcs in chans.values() for fc in fcs for k in fc.keyframe_points})
    out = []
    for f in frames:
        best, best_d = 1, None
        for p in range(1, 14):
            d = 0.0
            for key, fcs in chans.items():
                v = [fc.evaluate(f) for fc in fcs]
                q = poses[key][min(p, len(poses[key]) - 1)]
                if key[1] == "rotation_quaternion":
                    d += 1.0 - abs(sum(a * b for a, b in zip(v, q)))
                else:
                    d += sum((a - b) ** 2 for a, b in zip(v, q)) * 1e-4
            if best_d is None or d < best_d:
                best, best_d = p, d
        out.append((max(0.0, (f - frame0) / fps), best))
    return out


class G3_OT_export_lipsync(bpy.types.Operator):
    bl_idname = "gothic3.export_lipsync"
    bl_label = "Gothic 3 Lip-sync → Mod (.xlip)"
    bl_description = "Дія на кістках обличчя → таймлайн фонем репліки (.xlip): на кожному ключі — найближча поза голови з її кліпу _LipSync, як гра їх і читає"

    line: StringProperty(name="Репліка", description="Назва репліки гри (замінити її ліпсинк) або нова", search=_search_line, options={"SKIP_SAVE"})
    language: EnumProperty(name="Мова", items=LANGUAGES, default="English")
    head: StringProperty(name="Голова", description="Анімована голова, чиї пози (_LipSync) читати", search=_search_head, options={"SKIP_SAVE"})
    mode: EnumProperty(name="Куди", items=core.MODES, default="mod")
    folder: StringProperty(name="Тека", subtype="DIR_PATH")
    title: StringProperty(name="Назва мода", options={"SKIP_SAVE"})

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE" and o.animation_data is not None and o.animation_data.action is not None

    def invoke(self, context, event):
        act = context.active_object.animation_data.action
        if not self.line:
            self.line = act.get("g3_lipsync", "")
        if not self.head:
            self.head = act.get("g3_head", "") or context.active_object.get("g3_head", "")
        return context.window_manager.invoke_props_dialog(self, width=520)

    def execute(self, context):
        arm = context.active_object
        act = arm.animation_data.action
        if not self.line or not self.head:
            self.report({"ERROR"}, "Вкажіть репліку й анімовану голову")
            return {"CANCELLED"}
        sc = context.scene
        fps = sc.render.fps / sc.render.fps_base
        try:
            poses = head_poses(self.head)
            phones = phones_of(act, poses, fps, float(act.get("g3_frame0", sc.frame_start)))
            if not phones:
                self.report({"ERROR"}, "У дії немає ключів на кістках обличчя цієї голови")
                return {"CANCELLED"}
            tmp = os.path.join(core.cache_dir(), "lipsync", "export.json")
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump({"line": self.line, "language": self.language, "phones": phones}, f)
            out, where = core.deliver("export-lipsync", tmp, self.mode, self.line, self.title, self.folder)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        if self.mode == "install":
            from . import ui
            ui.refresh()
        r = out["report"]
        self.report({"INFO"}, f"{r['line']} ({r['language']}): {r['phones']} фонем, {r['duration']:.2f} с → {where}")
        return {"FINISHED"}


_classes = (G3_OT_import_lipsync, G3_OT_export_lipsync)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
