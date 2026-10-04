"""File > Import > Gothic 3 Model: a static `.xcmsh` straight from the game's archives, by category, with its
collision.

gothic3-core writes the mesh as OBJ + MTL + PNG into the cache (game axes mirrored in Z, centimetres; positions
welded, every corner keeping its own uv and normal) and Blender's own OBJ importer brings it in. The collision
(`<name>_COL.xnvmsh`, or `<name>.xnvmsh` beside a landscape cell), when the game has one, comes along as a wireframe
child named `<name>_COL`, one material per surface (`G3_Shape_<surface>`).
"""

import os

import bpy
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


def import_collision(name, scale, parent=None):
    path = os.path.join(core.cache_dir(), "meshes", f"{name}_COL.obj")
    try:
        out = core.run("collision", core.game_dir(), name, path)
    except core.CoreError:
        return None, None
    obs = _import_obj(path, scale)
    if not obs:
        return None, None
    col = obs[0]
    col.name = f"{name}_COL"
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
    with_collision: BoolProperty(name="Разом з колізією", description="Колізія як каркасний об'єкт <назва>_COL, якщо в гри вона є", default=True)
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
        note = ""
        if self.with_collision and obs:
            col, c = import_collision(name, self.scale, obs[0])
            note = f"; колізія {c['triangles']} трикутників" if col else "; колізії в гри немає"
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
        col, out = import_collision(self.name, self.scale)
        if col is None:
            self.report({"ERROR"}, f"{self.name}: у гри немає колізії-сітки для цієї моделі")
            return {"CANCELLED"}
        self.report({"INFO"}, f"{self.name}: колізія, {out['triangles']} трикутників")
        return {"FINISHED"}


_classes = (G3_OT_import_mesh, G3_OT_import_collision)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
