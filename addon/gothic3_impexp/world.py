"""File > Import > Gothic 3 World Layer: what the game places in one world layer (`.node`), and within a radius
around it the town's houses and rocks (the static cells) and the ground, as context to build against.

Placing objects in the game is a world editor's job; a layer comes in here only so new models can be made to fit
where they will stand. Each mesh is loaded once and linked at each of its placements.
"""

import os

import bpy
from bpy.props import BoolProperty, FloatProperty, StringProperty
from mathutils import Matrix, Vector

from . import core, materials

_layers = {}


def _search_layer(self, context, edit_text):
    try:
        g = core.game_dir()
        if g not in _layers:
            _layers[g] = core.run("world-layers", g)
    except core.CoreError:
        return []
    words = edit_text.lower().split()
    return [n for n in _layers[g] if all(w in n.lower() for w in words)][:300]


# Blender (x, y, z) = game (x, z, y) × scale: the mesh import mirrors Z and the OBJ importer turns -Z forward, which
# together swap Y and Z.
_P = Matrix(((1, 0, 0), (0, 0, 1), (0, 1, 0)))


def game_matrix_to_blender(m, scale):
    """A layer matrix (row vectors: world = local · M, translation in [12..15]) → Blender's."""
    r = Matrix(((m[0], m[4], m[8]), (m[1], m[5], m[9]), (m[2], m[6], m[10])))  # column form (Mᵀ)
    out = (_P @ r @ _P).to_4x4()
    out.translation = (_P @ Vector((m[12], m[13], m[14]))) * scale
    return out


def _load(info, scale):
    """The mesh of one OBJ written by the core (materials set up and shared) and the turn the importer put on it."""
    before = set(bpy.data.objects)
    bpy.ops.wm.obj_import(filepath=info["obj"], global_scale=scale, forward_axis="NEGATIVE_Z", up_axis="Y")
    new = [o for o in bpy.data.objects if o not in before]
    if not new:
        return None
    materials.apply_to_objects(new, info["materials"], os.path.dirname(info["obj"]))
    me, fix = new[0].data, new[0].matrix_world.copy()
    for o in new:
        bpy.data.objects.remove(o, do_unlink=True)
    return me, fix


def place_trees(trees, mesh, scale, parent, name):
    """SpeedTree stand-ins at their placements, in a collection of their own under `parent`, marked as for the view only
    (`g3_view_only`: the exports leave them out). `mesh(stem)` gives (mesh, fix) as `_load` does. Returns how many."""
    if not trees:
        return 0
    col = bpy.data.collections.new(f"{name} SpeedTree (огляд)")
    parent.children.link(col)
    col["g3_view_only"] = True
    n = 0
    for t in trees:
        got = mesh(t["mesh"])
        if got is None:
            continue
        ob = bpy.data.objects.new(f"SPT_{t['mesh']}", got[0])
        ob.matrix_world = game_matrix_to_blender(t["world"], scale) @ got[1]
        ob["g3_view_only"] = True
        ob["g3_source"] = t["source"]
        col.objects.link(ob)
        n += 1
    return n


class G3_OT_import_world(bpy.types.Operator):
    bl_idname = "gothic3.import_world"
    bl_label = "Gothic 3 World Layer (.node)"
    bl_description = "Розстановка шару світу як оточення (і будинки, скелі й рельєф навколо): будуйте нове під місце, де воно стоятиме"
    bl_options = {"REGISTER", "UNDO"}

    name: StringProperty(name="Шар", description="Напр. ardea_dynamic_objects, faring_vegetation, geldern_items", search=_search_layer)
    radius: FloatProperty(name="Навколо, м", description="Будинки, скелі (статичні клітинки) і рельєф у цьому радіусі від середини шару; 0 = лише шар", default=40.0, min=0.0, max=500.0)
    with_ground: BoolProperty(name="Рельєф", description="Клітинки рельєфу під цим місцем", default=True)
    with_trees: BoolProperty(name="Дерева", description="Дерева (SpeedTree) статичних клітинок у радіусі — заглушки лише для огляду: стовбур і схрещені картки листя гри, висотою як у рецепті", default=True)
    scale: FloatProperty(name="Масштаб", description="Gothic 3 рахує в сантиметрах; 0.01 = метри Blender", default=0.01, min=1e-6)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=440)

    def execute(self, context):
        if not self.name:
            self.report({"ERROR"}, "Не вказано шар світу")
            return {"CANCELLED"}
        try:
            lay = core.run("world", core.game_dir(), self.name, str(self.radius * 100.0), os.path.join(core.cache_dir(), "meshes"), *([] if self.with_ground else ["noground"]), *([] if self.with_trees else ["notrees"]))
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        col = bpy.data.collections.new(lay["layer"])
        context.scene.collection.children.link(col)
        col["g3_world_layer"] = lay["layer"]
        loaded = {}

        def mesh(stem):
            k = stem.lower()
            if k not in loaded:
                info = lay["meshes"].get(k)
                loaded[k] = _load(info, self.scale) if info else None
            return loaded[k]

        placed = 0
        for it in lay["items"]:
            got = mesh(it["mesh"])
            if got is None:
                continue
            me, fix = got
            ob = bpy.data.objects.new(it["mesh"], me)
            ob.matrix_world = game_matrix_to_blender(it["world"], self.scale) @ fix
            ob["g3_source"] = it["source"]
            col.objects.link(ob)
            placed += 1
        ground = 0
        if self.with_ground:
            for stem in lay["landscape"]:
                got = mesh(stem)
                if got is None:
                    continue
                ob = bpy.data.objects.new(stem, got[0])
                ob.matrix_world = got[1]
                ob["g3_source"] = "landscape"
                col.objects.link(ob)
                ground += 1
        trees = place_trees(lay.get("trees", []), mesh, self.scale, col, lay["layer"])
        for w in lay["warnings"][:5]:
            self.report({"WARNING"}, w)
        kinds = sum(1 for v in loaded.values() if v)
        self.report({"INFO"}, f"{lay['layer']}: {placed} моделей ({kinds} різних), клітинок рельєфу {ground}, дерев {trees} (лише огляд)")
        return {"FINISHED"}


_classes = (G3_OT_import_world,)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
