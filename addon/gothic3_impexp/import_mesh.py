"""File > Import > Gothic 3 Model: a static `.xcmsh` straight from the game's archives, by category, with its
collision.

gothic3-core writes the mesh as OBJ + MTL + PNG into the cache (game axes mirrored in Z, centimetres; positions
welded, every corner keeping its own uv and normal) and Blender's own OBJ importer brings it in. The game's further
coordinate sets come beside it (`.uvx`) and become UV layers `g3_uv<stream>` as the game keeps them, as the terrain's:
g3_uv15, g3_uv21, g3_uv18 (more texture sets) and g3_uv73 (the lightmap's coordinates). The collision
(`<name>_COL.xnvmsh`, or `<name>.xnvmsh` beside a landscape cell), when the game has one, comes along as a wireframe
child named `<name>_COL`, one material per surface (`G3_Shape_<surface>`); an item's convex body (`<name>_CV.xnvmsh`,
what it has when it lies, falls and rolls) as `<name>_CV`.
"""

import os

import bpy
import numpy as np
from bpy.props import BoolProperty, EnumProperty, FloatProperty, StringProperty

from . import catalog, core, materials


def _search(self, context, edit_text):
    try:
        return catalog.search(catalog.meshes(), self.category, edit_text)
    except core.CoreError:
        return []


def _import_obj(path, scale):
    before = set(bpy.data.objects)
    bpy.ops.wm.obj_import(filepath=path, global_scale=scale, forward_axis="NEGATIVE_Z", up_axis="Y")
    return [o for o in bpy.data.objects if o not in before]


# The game's further coordinate sets as UV layers, named by stream and holding the game's numbers as they are (as the
# terrain's layers do): texture sets 2 to 4 (15, 21, 18) and the lightmap's coordinates (73). Export writes them back.
UV_LAYERS = {t: f"g3_uv{t}" for t in (15, 21, 18, 73)}


def add_uv_layers(ob, path):
    """Put the sets of `<stem>.uvx` (see gothic3-core's staticmesh.rs) on `ob` as UV layers. Returns the layer names,
    or a reason they could not be matched to the faces."""
    with open(path, "rb") as f:
        d = f.read()
    if d[:6] != b"G3UVX1":
        return "unknown .uvx"
    n = int(np.frombuffer(d, np.uint32, 1, 6)[0])
    types = np.frombuffer(d, np.uint32, n, 10).tolist()
    faces = int(np.frombuffer(d, np.uint32, 1, 10 + 4 * n)[0])
    rec = 1 + n * 3 * 8
    body = np.frombuffer(d, np.uint8, faces * rec, 14 + 4 * n).reshape(faces, rec)
    uv = body[:, 1:].copy().view(np.float32).reshape(faces, n, 3, 2)
    me = ob.data
    if len(me.polygons) != faces:
        return f"{len(me.polygons)} faces in Blender, {faces} in the game"
    if any(len(p.vertices) != 3 for p in me.polygons):
        return "faces are not triangles"
    active, render = me.uv_layers.active_index, next((i for i, l in enumerate(me.uv_layers) if l.active_render), 0)
    names = []
    for k, t in enumerate(types):
        name = UV_LAYERS.get(t)
        if name is None:
            continue
        data = uv[:, k].reshape(-1, 2).copy()
        layer = me.uv_layers.get(name) or me.uv_layers.new(name=name, do_init=False)
        layer.data.foreach_set("uv", data.ravel())
        names.append(name)
    me.uv_layers.active_index = active
    if len(me.uv_layers) > render:
        me.uv_layers[render].active_render = True
    return names


def import_collision(name, scale, parent=None, convex=False):
    """The game's collision of model `name` as a wireframe child `<name>_COL` (or its convex body `<name>_CV`).
    Returns (object, core report); (None, None) when the game has none; a core error otherwise."""
    end = "_CV" if convex else "_COL"
    path = os.path.join(core.cache_dir(), "meshes", f"{name}{end}.obj")
    try:
        out = core.run("collision", core.game_dir(), name, path, *(["convex"] if convex else []))
    except core.CoreError as e:
        if "Gothic 3 has no" in str(e):
            return None, None
        raise
    obs = _import_obj(path, scale)
    if not obs:
        return None, None
    col = obs[0]
    col.name = f"{name}{end}"
    materials.setup_shape_materials(col)
    col.display_type = "WIRE"
    col.hide_render = True
    if parent is not None:
        col.parent = parent
        col.matrix_parent_inverse = parent.matrix_world.inverted()
    return col, out


class G3_OT_import_mesh(bpy.types.Operator):
    bl_idname = "gothic3.import_mesh"
    bl_label = "Gothic 3 Model (.xcmsh)"
    bl_description = "Статична модель з архівів гри (предмети, будівлі, декор, локації) з текстурами й колізією"
    bl_options = {"REGISTER", "UNDO"}

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.MESH_CATEGORIES))
    name: StringProperty(name="Модель", description="Назва .xcmsh у грі, напр. G3_Object_Barrel_02", search=_search)
    with_collision: BoolProperty(name="Разом з колізією", description="Колізія як каркасний об'єкт <назва>_COL (і опукла <назва>_CV у предметів), якщо в гри вона є", default=True)
    scale: FloatProperty(name="Масштаб", description="Gothic 3 рахує в сантиметрах; 0.01 = метри Blender", default=0.01, min=1e-6)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=440)

    def execute(self, context):
        if not self.name:
            self.report({"ERROR"}, "Не вказано назву моделі")
            return {"CANCELLED"}
        try:
            out = core.run("mesh", core.game_dir(), self.name, os.path.join(core.cache_dir(), "meshes"))
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        stem = os.path.splitext(os.path.basename(out["obj"]))[0]
        obs = _import_obj(out["obj"], self.scale)
        name = next((n for n, _, _ in catalog.meshes() if n.lower() == stem.lower()), stem)
        for ob in obs:
            ob.name = name
            ob["g3_entry"] = out["entry"]
        materials.apply_to_objects(obs, out["materials"], os.path.dirname(out["obj"]))
        if out.get("uvx"):
            for ob in obs:
                got = add_uv_layers(ob, out["uvx"])
                if isinstance(got, str):
                    self.report({"WARNING"}, f"додаткові UV ({os.path.basename(out['uvx'])}) не накладено: {got}")
        note = ""
        if self.with_collision and obs:
            try:
                col, c = import_collision(name, self.scale, obs[0])
                hull, h = import_collision(name, self.scale, obs[0], convex=True)
            except core.CoreError as e:
                self.report({"WARNING"}, f"колізію не прочитано: {e}")
            else:
                note = f"; колізія {c['triangles']} трикутників" if col else "; колізії-сітки в гри немає"
                if hull:
                    note += f", опукла {h['triangles']}"
        for w in out["warnings"]:
            self.report({"WARNING"}, w)
        self.report({"INFO"}, f"{name}: {out['vertices']} вершин, {out['triangles']} трикутників, {len(out['materials'])} матеріалів{note}")
        return {"FINISHED"}


class G3_OT_import_collision(bpy.types.Operator):
    bl_idname = "gothic3.import_collision"
    bl_label = "Gothic 3 Collision (.xnvmsh)"
    bl_description = "Лише колізія моделі як каркасна сітка, по матеріалу на поверхню (камінь, дерево, земля…)"
    bl_options = {"REGISTER", "UNDO"}

    category: EnumProperty(name="Категорія", items=catalog.enum_items(catalog.MESH_CATEGORIES))
    name: StringProperty(name="Модель", search=_search)
    scale: FloatProperty(name="Масштаб", default=0.01, min=1e-6)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=440)

    def execute(self, context):
        try:
            col, out = import_collision(self.name, self.scale)
            hull, h = import_collision(self.name, self.scale, convex=True)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        if col is None and hull is None:
            self.report({"ERROR"}, f"{self.name}: у гри немає колізії для цієї моделі")
            return {"CANCELLED"}
        parts = []
        if col:
            parts.append(f"{out['triangles']} трикутників")
        if hull:
            parts.append(f"опукла {h['triangles']}")
        self.report({"INFO"}, f"{self.name}: колізія, {', '.join(parts)}")
        return {"FINISHED"}


_classes = (G3_OT_import_mesh, G3_OT_import_collision)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
