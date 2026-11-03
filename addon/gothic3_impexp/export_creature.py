"""File > Export > Gothic 3 New Creature: an armature of your own, its mesh and its actions become a creature of the game.

The game ties a clip to its actor by the clip name's first word: a creature called `Mole` is the actor
`G3_Mole_Body_01` (`G3_Mole_Skeleton` without a mesh) and its clips `Mole_Stand_None_None_P0_Ambient_Loop_N_Fwd_01_…`.
So the bones are named `Mole_…` (`Mole_ROOT`, `Mole_Spine_Spine_1`, `Mole_Head_Head_1` — the game looks for these —,
sides as `Mole_Left_…` / `Mole_Right_…`, items on `Slot_…` bones), and each action is named with the words the game
composes clip names of (`Stand_None_None_P0_Ambient_Loop_N_Fwd_01`); actions named otherwise are left out. Bones may
carry `g3_limit` (18 numbers) and `g3_shape` (kind and 8 numbers) for the game's physics.

The creature still needs a template to be placed in the world (the game's editors make those).
"""

import json
import os
import shutil
import tempfile

import bpy
from bpy.props import BoolProperty, EnumProperty, FloatProperty, StringProperty

from . import core
from .export_actor import gather_skinned


def _meshes(arm):
    return [o for o in bpy.data.objects if o.type == "MESH" and (o.parent == arm or any(m.type == "ARMATURE" and m.object == arm for m in o.modifiers))]


def creature_spec(context, arm, word, scale, tmp, lenient=False):
    """The core's spec for `arm` as creature `word`: the glb of the armature with its meshes and actions, the skin."""
    meshes = _meshes(arm)
    glb = os.path.join(tmp, "creature.glb")
    sel = list(context.selected_objects)
    active = context.view_layer.objects.active
    try:
        for o in sel:
            o.select_set(False)
        for o in [arm] + meshes:
            o.select_set(True)
        context.view_layer.objects.active = arm
        bpy.ops.export_scene.gltf(
            filepath=glb, export_format="GLB", use_selection=True, export_animations=True,
            export_animation_mode="ACTIONS", export_force_sampling=True, export_frame_step=1,
            export_anim_slide_to_zero=True, export_optimize_animation_size=False, export_skins=True,
            export_materials="NONE", export_yup=True, export_def_bones=False,
        )
    finally:
        for o in [arm] + meshes:
            o.select_set(False)
        for o in sel:
            o.select_set(True)
        context.view_layer.objects.active = active
    # Only actions that move this armature's bones (Blender's exporter writes every action of the file).
    from .lipsync import _fcurves
    names = {b.name for b in arm.data.bones}
    own = [a.name for a in bpy.data.actions if any(fc.data_path.startswith('pose.bones["') and fc.data_path[12:].split('"', 1)[0] in names for fcs in _fcurves(a) for fc in fcs)]
    spec = {"word": word, "glb": glb, "lenient": lenient, "limits": {}, "shapes": {}, "actions": own}
    for b in arm.data.bones:
        if "g3_limit" in b:
            spec["limits"][b.name] = [float(x) for x in b["g3_limit"]]
        if "g3_shape" in b:
            v = [float(x) for x in b["g3_shape"]]
            spec["shapes"][b.name] = [int(v[0]), v[1:]]
    if meshes:
        g = gather_skinned(context, meshes, scale, tmp, arm=arm)
        if g is None:
            raise core.CoreError("У мешах істоти немає трикутників")
        pos, cv, nrm, uv, mat, packed, bones, materials, unweighted = g
        if unweighted:
            raise core.CoreError(f"{unweighted} вершин без ваг кісток — призначте їх групам вершин кісток")
        geo = os.path.join(tmp, "skin.bin")
        with open(geo, "wb") as f:
            for a in (pos, cv, nrm, uv, mat, packed):
                import numpy as np
                f.write(np.ascontiguousarray(a).tobytes())
        spec["skin"] = {"base": "", "name": word, "geometry": geo, "vertices": len(pos), "corners": len(cv), "bones": bones, "materials": materials}
    return spec


class G3_OT_export_creature(bpy.types.Operator):
    bl_idname = "gothic3.export_creature"
    bl_label = "Gothic 3 New Creature → Mod"
    bl_description = "Власний скелет, меш і дії → нова істота гри: G3_<Слово>_Body_01.xact і кліпи <Слово>_…xmot (назви кісток і дій перевіряються так, як їх чекає гра)"

    word: StringProperty(name="Слово істоти", description="Латиницею з великої (Mole): актор G3_Mole_Body_01, кліпи Mole_…, кістки Mole_…", options={"SKIP_SAVE"})
    lenient: BoolProperty(name="Не перевіряти назви кісток", description="Записати й кістки, назв яких код гри не чекає (голова й хребет можуть не знайтися)", default=False, options={"SKIP_SAVE"})
    mode: EnumProperty(name="Куди", items=core.MODES, default="mod")
    folder: StringProperty(name="Тека", subtype="DIR_PATH")
    title: StringProperty(name="Назва мода", options={"SKIP_SAVE"})
    scale: FloatProperty(name="Масштаб", default=0.01, min=1e-6)

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE"

    def invoke(self, context, event):
        if not self.word:
            self.word = context.active_object.name.split(".")[0].split("_")[0]
        return context.window_manager.invoke_props_dialog(self, width=480)

    def execute(self, context):
        arm = context.active_object
        tmp = tempfile.mkdtemp(prefix="g3_creature_")
        try:
            spec = creature_spec(context, arm, self.word, self.scale, tmp, self.lenient)
            path = os.path.join(tmp, "spec.json")
            with open(path, "w", encoding="utf-8") as f:
                json.dump(spec, f)
            out, where = core.deliver("export-creature", path, self.mode, self.word, self.title or self.word, self.folder)
            if self.mode == "install":
                from . import ui
                ui.refresh()
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        r = out["report"]
        for s in r["skipped"][:5]:
            self.report({"WARNING"}, f"дію пропущено: {s}")
        for n in r["notes"][:5]:
            self.report({"INFO"}, n)
        self.report({"INFO"}, f"{r['actor']}: {r['bones']} кісток, {r['vertices']} вершин, {r['triangles']} трикутників, кліпів {len(r['clips'])} → {where}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_creature)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_creature)
