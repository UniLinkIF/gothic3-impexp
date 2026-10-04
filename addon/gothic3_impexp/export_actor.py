"""File > Export > Gothic 3 Actor: a skinned mesh on a Gothic 3 skeleton becomes an actor (`.xact`).

Select the mesh(es) bound to a skeleton imported as Gothic 3 Character (Armature modifier; weights in vertex groups
named after the bones). The core keeps the base actor's skeleton and everything else and swaps in this mesh, its
skin and its materials — new armour, a reshaped creature. A head brought in with a body is its own actor: export it
alone and it replaces that head. Same name as the base = replace it.
"""

import json
import os
import tempfile

import bpy
import numpy as np
from bpy.props import EnumProperty, FloatProperty, StringProperty

from . import core
from .export_mesh import _material_spec
from .prefs import prefs


def _armature_of(ob):
    for m in ob.modifiers:
        if m.type == "ARMATURE" and m.object is not None and "g3_actor" in m.object:
            return m.object
    p = ob.parent
    return p if p is not None and p.type == "ARMATURE" and "g3_actor" in p else None


def base_of(ob):
    return ob.get("g3_base") or _armature_of(ob)["g3_actor"]


def gather_skinned(context, objects, scale, tmp):
    """Rest-pose geometry of `objects` in game space, per-vertex bone weights (up to 8) and materials."""
    mods = [m for o in objects for m in o.modifiers if m.type == "ARMATURE"]
    saved = [(m, m.show_viewport) for m in mods]
    for m, _ in saved:
        m.show_viewport = False
    try:
        dg = context.evaluated_depsgraph_get()
        dg.update()
        pos, cv, nrm, uv, mat, weights = [], [], [], [], [], []
        bones, bone_of, materials, slot_of = [], {}, [], {}
        vbase = 0
        for ob in objects:
            ev = ob.evaluated_get(dg)
            me = ev.to_mesh()
            try:
                me.transform(ob.matrix_world)
                me.calc_loop_triangles()
                nt, nv = len(me.loop_triangles), len(me.vertices)
                if nt == 0:
                    continue
                loops = np.empty(nt * 3, np.int32)
                me.loop_triangles.foreach_get("loops", loops)
                mi = np.empty(nt, np.int32)
                me.loop_triangles.foreach_get("material_index", mi)
                vidx = np.empty(len(me.loops), np.int32)
                me.loops.foreach_get("vertex_index", vidx)
                co = np.empty(nv * 3, np.float32)
                me.vertices.foreach_get("co", co)
                cn = np.empty(len(me.loops) * 3, np.float32)
                me.corner_normals.foreach_get("vector", cn)
                u = np.zeros(len(me.loops) * 2, np.float32)
                if me.uv_layers.active:
                    me.uv_layers.active.data.foreach_get("uv", u)
                co, cn, u = co.reshape(-1, 3), cn.reshape(-1, 3), u.reshape(-1, 2)
                pos.append((co / scale)[:, [0, 2, 1]])
                cv.append(vidx[loops] + vbase)
                nrm.append(cn[loops][:, [0, 2, 1]])
                t = u[loops]
                uv.append(np.stack([t[:, 0], 1.0 - t[:, 1]], axis=1))
                slots = [s.material for s in ob.material_slots] or [None]
                remap = []
                for m in slots:
                    key = m.name if m else "Default"
                    if key not in slot_of:
                        slot_of[key] = len(materials)
                        materials.append(_material_spec(m, tmp))
                    remap.append(slot_of[key])
                mat.append(np.array(remap, np.uint32)[np.clip(mi, 0, len(remap) - 1)])
                groups = {g.index: g.name for g in ob.vertex_groups}
                src = me if len(me.vertices) == nv else ob.data
                for v in src.vertices:
                    w = sorted(((g.weight, groups.get(g.group)) for g in v.groups if g.weight > 0 and groups.get(g.group)), reverse=True)[:8]
                    row = []
                    for weight, name in w:
                        if name not in bone_of:
                            bone_of[name] = len(bones)
                            bones.append(name)
                        row.append((bone_of[name], weight))
                    weights.append(row)
                vbase += nv
            finally:
                ev.to_mesh_clear()
    finally:
        for m, show in saved:
            m.show_viewport = show
    if not pos:
        return None
    packed = np.zeros((len(weights), 16), np.uint32)
    for i, row in enumerate(weights):
        for k, (b, w) in enumerate(row):
            packed[i, 2 * k] = b
            packed[i, 2 * k + 1] = np.float32(w).view(np.uint32)
    return (np.concatenate(pos).astype(np.float32), np.concatenate(cv).astype(np.uint32), np.concatenate(nrm).astype(np.float32),
            np.concatenate(uv).astype(np.float32), np.concatenate(mat).astype(np.uint32), packed, bones, materials)


class G3_OT_export_actor(bpy.types.Operator):
    bl_idname = "gothic3.export_actor"
    bl_label = "Gothic 3 Actor → Mod (.xact)"
    bl_description = "Виділені меші на скелеті Gothic 3 → актор гри (нова броня, тіло, голова); скелет і решта — з базового актора"

    name: StringProperty(name="Назва актора", description="Назва базового актора — заміна (напр. G3_Hero_Body_Player); нова назва — новий актор поруч")
    mode: EnumProperty(name="Куди", items=(
        ("install", "Встановити в гру", "Одразу в гру (том-латка); прибрати — панель Gothic 3"),
        ("package", "Пакет мода", "Папка з томами, INSTALL.bat і ROLLBACK.bat"),
    ), default="install")
    folder: StringProperty(name="Папка пакета", subtype="DIR_PATH")
    title: StringProperty(name="Назва мода")
    scale: FloatProperty(name="Масштаб", default=0.01, min=1e-6)

    @classmethod
    def poll(cls, context):
        return any(o.type == "MESH" and _armature_of(o) for o in context.selected_objects)

    def invoke(self, context, event):
        obs = [o for o in context.selected_objects if o.type == "MESH" and _armature_of(o)]
        if not self.name:
            self.name = base_of(obs[0])
        return context.window_manager.invoke_props_dialog(self, width=480)

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "name")
        col.prop(self, "mode", expand=True)
        if self.mode == "package":
            col.prop(self, "folder")
            col.prop(self, "title")

    def execute(self, context):
        objects = [o for o in context.selected_objects if o.type == "MESH" and _armature_of(o)]
        bases = {base_of(o) for o in objects}
        if len(bases) != 1:
            self.report({"ERROR"}, f"Виділіть меші одного актора (зараз: {', '.join(sorted(bases)) or 'жодного'}) — тіло й голову експортують окремо")
            return {"CANCELLED"}
        tmp = tempfile.mkdtemp(prefix="g3_actor_")
        g = gather_skinned(context, objects, self.scale, tmp)
        if g is None:
            self.report({"ERROR"}, "У виділених мешах немає трикутників")
            return {"CANCELLED"}
        pos, cv, nrm, uv, mat, packed, bones, materials = g
        if not bones:
            self.report({"ERROR"}, "Жодна вершина не має ваг кісток (групи вершин з назвами кісток)")
            return {"CANCELLED"}
        geo = os.path.join(tmp, "skin.bin")
        with open(geo, "wb") as f:
            for a in (pos, cv, nrm, uv, mat, packed):
                f.write(np.ascontiguousarray(a).tobytes())
        spec = os.path.join(tmp, "spec.json")
        with open(spec, "w", encoding="utf-8") as f:
            json.dump({"base": bases.pop(), "name": self.name, "geometry": geo, "vertices": len(pos), "corners": len(cv), "bones": bones, "materials": materials}, f)
        try:
            if self.mode == "package":
                folder = bpy.path.abspath(self.folder) if self.folder else os.path.join(prefs().mods_dir or tmp, self.name)
                out = core.run("export-actor", core.game_dir(), spec, "package", folder, self.title or self.name)
            else:
                out = core.run("export-actor", core.game_dir(), spec, "install", self.name)
                from . import ui
                ui.refresh()
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        r = out["report"]
        what = "замінено" if r["replaced"] else "новий"
        self.report({"INFO"}, f"{r['actor']} ({what}): {r['vertices']} вершин, {r['triangles']} трикутників; {'; '.join(r['notes'])}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_actor)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_actor)
