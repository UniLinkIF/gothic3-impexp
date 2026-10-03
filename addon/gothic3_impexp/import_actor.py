"""File > Import > Gothic 3 Character (the model, no animation) and Gothic 3 Animations (onto a character already in
the scene), each by category.

gothic3-core turns the `.xact` (skeleton + skinned meshes; a human body with the head of your choice) and chosen
`.xmot` clips into a glTF; Blender's glTF importer builds the armature, the skin and one action per clip. Bones show
as sticks.
"""

import os
import tempfile

import bpy
from bpy.props import BoolProperty, EnumProperty, IntProperty, StringProperty

from . import catalog, core


def _search_actor(self, context, edit_text):
    try:
        return catalog.search(catalog.actors(), getattr(self, "category", catalog.ALL), edit_text)
    except core.CoreError:
        return []


def _search_head(self, context, edit_text):
    try:
        return [n for n in catalog.search(catalog.actors(), "head", edit_text) if "_lod" not in n.lower()]
    except core.CoreError:
        return []


def _import_glb(path):
    before_obs, before_acts = set(bpy.data.objects), set(bpy.data.actions)
    bpy.ops.import_scene.gltf(filepath=path, merge_vertices=True, disable_bone_shape=True)
    return [o for o in bpy.data.objects if o not in before_obs], [a for a in bpy.data.actions if a not in before_acts]


def _glb_path(name):
    d = os.path.join(core.cache_dir(), "actors")
    os.makedirs(d, exist_ok=True)
    return os.path.join(d, f"{name}.glb")


def _actor_category(name):
    return next((c for n, _, c in catalog.actors() if n.lower() == name.lower()), "monster")


def import_character(name, clips="", limit=40, head="-"):
    out = core.run("actor", core.game_dir(), name, _glb_path(name), clips, str(limit), "full", head or "-")
    obs, acts = _import_glb(out["glb"])
    arm = next((o for o in obs if o.type == "ARMATURE"), None)
    if arm:
        arm["g3_actor"] = name
        arm.name = name
        arm.data.display_type = "STICK"
        arm.show_in_front = True
        if acts and arm.animation_data:
            arm.animation_data.action = acts[0]
    for a in acts:
        a.use_fake_user = True
    return out, arm, acts


class G3_OT_import_character(bpy.types.Operator):
    bl_idname = "gothic3.import_character"
    bl_label = "Gothic 3 Character (.xact)"
    bl_description = "Модель персонажа, монстра чи анімованого об'єкта — скелет і шкірка, без анімацій"
    bl_options = {"REGISTER", "UNDO"}

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.ACTOR_CATEGORIES))
    name: StringProperty(name="Модель", description="Назва .xact, напр. G3_Hero_Body_Player або G3_Wolf_Body_01", search=_search_actor)
    with_head: BoolProperty(name="З головою", description="Людське тіло без голови — підставити голову", default=True)
    head: StringProperty(name="Голова", description="Порожньо — голова Безіменного (G3_Head_Hero_Hero_01)", search=_search_head)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=460)

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "category")
        col.prop(self, "name")
        if self.category in ("human", catalog.ALL):
            col.prop(self, "with_head")
            if self.with_head:
                col.prop(self, "head")

    def execute(self, context):
        if not self.name:
            self.report({"ERROR"}, "Не вказано модель")
            return {"CANCELLED"}
        try:
            human = _actor_category(self.name) == "human"
            head = (self.head or "G3_Head_Hero_Hero_01") if (human and self.with_head) else "-"
            out, arm, _ = import_character(self.name, "", 1, head)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        for w in out["warnings"]:
            self.report({"WARNING"}, w)
        self.report({"INFO"}, f"{self.name}: {out['joints']} кісток, {out['triangles']} трикутників")
        return {"FINISHED"}


_clip_cache = {}


def actor_clips(actor):
    if actor not in _clip_cache:
        _clip_cache[actor] = core.run("clips", core.game_dir(), actor, "", "100000")
    return _clip_cache[actor]


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


class G3_OT_import_motion(bpy.types.Operator):
    bl_idname = "gothic3.import_motion"
    bl_label = "Gothic 3 Animations (.xmot)"
    bl_description = "Анімації на виділений скелет Gothic 3: одна або ціла категорія"
    bl_options = {"REGISTER", "UNDO"}

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.CLIP_CATEGORIES))
    clip: StringProperty(name="Анімація", description="Одна анімація; порожньо — усі з категорії (до «Не більше»)", search=_search_clip)
    limit: IntProperty(name="Не більше", default=20, min=1, max=2000)

    @classmethod
    def poll(cls, context):
        o = context.active_object
        return o is not None and o.type == "ARMATURE" and "g3_actor" in o

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=520)

    def execute(self, context):
        arm = context.active_object
        actor = arm["g3_actor"]
        try:
            names = [self.clip] if self.clip else [c for c in actor_clips(actor) if self.category == catalog.ALL or catalog.clip_category(c) == self.category][:self.limit]
            if not names:
                self.report({"ERROR"}, f"{actor}: у цій категорії немає анімацій")
                return {"CANCELLED"}
            listing = os.path.join(tempfile.mkdtemp(prefix="g3_clips_"), "clips.txt")
            with open(listing, "w", encoding="utf-8") as f:
                f.write("\n".join(names))
            out = core.run("actor", core.game_dir(), actor, _glb_path(actor + "_motion"), "@" + listing, str(len(names)), "skeleton", "-")
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        if not out["clips"]:
            self.report({"ERROR"}, f"{actor}: анімації не прочитались")
            return {"CANCELLED"}
        mats = set(bpy.data.materials)
        obs, acts = _import_glb(out["glb"])
        # Only the actions are wanted; the helper armature, skin and materials go again.
        datas = {o.data for o in obs if o.data is not None}
        for o in obs:
            bpy.data.objects.remove(o, do_unlink=True)
        for d in datas:
            if isinstance(d, bpy.types.Mesh) and d.users == 0:
                bpy.data.meshes.remove(d)
            elif isinstance(d, bpy.types.Armature) and d.users == 0:
                bpy.data.armatures.remove(d)
        for m in set(bpy.data.materials) - mats:
            if m.users == 0:
                bpy.data.materials.remove(m)
        for a in acts:
            a.use_fake_user = True
            for c, _ in out["clips"]:
                if a.name.startswith(c):
                    a.name = c
                    a["g3_clip"] = c
        context.view_layer.objects.active = arm
        arm.select_set(True)
        if arm.animation_data is None:
            arm.animation_data_create()
        arm.animation_data.action = acts[0]
        if hasattr(arm.animation_data, "action_slot") and acts[0].slots:
            arm.animation_data.action_slot = acts[0].slots[0]
        for w in out["warnings"]:
            self.report({"WARNING"}, w)
        self.report({"INFO"}, f"{actor}: {len(acts)} анімацій — {', '.join(a.name for a in acts[:3])}{' …' if len(acts) > 3 else ''}")
        return {"FINISHED"}


_classes = (G3_OT_import_character, G3_OT_import_motion)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
