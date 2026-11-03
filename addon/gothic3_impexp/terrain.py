"""File > Import > Gothic 3 Terrain and File > Export > Gothic 3 Terrain → Mod: landscape cells as one editable mesh.

The cells come in as one object (`G3 Terrain`), welded across their borders so the ground can be sculpted as a whole.
What a cell carries rides along on the mesh: the game's materials, the uv map, the colours as corner colour attributes
(`g3_diffuse`: its alpha blends the ground layers; `g3_light`: its alpha is the baked brightness), in Varant three more
uv sets (`g3_uv15`, `g3_uv21`, `g3_uv18`), and on every face the cell it belongs to (`g3_cell`). Faces can be moved,
cut, added and painted; a new face takes the cell of the face it was made from.

The export sends each cell back on its own. The core writes only the cells that changed (mesh, collision from their
own triangles, lightmap); a cell as it came out is left to the game.

Axes as everywhere: Blender (x, y, z) in metres = game (x, z, y) in centimetres × scale.
"""

import json
import os
import shutil
import tempfile

import bpy
import numpy as np
from bpy.props import BoolProperty, EnumProperty, FloatProperty, StringProperty

from . import core, materials

_cells = {}


def _cell_names():
    g = core.game_dir()
    if g not in _cells:
        _cells[g] = core.run("terrain-cells", g)
    return _cells[g]


def _search(self, context, edit_text):
    try:
        cells = _cell_names()
    except core.CoreError:
        return []
    words = edit_text.lower().split()
    return [c["name"] for c in cells if all(w in c["name"].lower() for w in words)][:300]


def _argb_to_rgba(x):
    """u32 ARGB per corner → float RGBA in 0..1 (the byte values, as `color_srgb` takes them)."""
    x = x.astype(np.uint32)
    return np.stack([(x >> 16) & 255, (x >> 8) & 255, x & 255, x >> 24], axis=1).astype(np.float32) / 255.0


def _rgba_to_argb(c):
    b = np.clip(np.round(c * 255.0), 0, 255).astype(np.uint32)
    return (b[:, 3] << 24) | (b[:, 0] << 16) | (b[:, 1] << 8) | b[:, 2]


def _read_cell(path):
    d = np.fromfile(path, dtype=np.uint32)
    nv, nt, ne = int(d[0]), int(d[1]), int(d[2])
    streams = [int(s) for s in d[3:3 + ne]]
    at = 3 + ne
    f = d.view(np.float32)
    pos = f[at:at + nv * 3].reshape(-1, 3); at += nv * 6
    uv = f[at:at + nv * 2].reshape(-1, 2); at += nv * 2
    extra = []
    for _ in streams:
        extra.append(f[at:at + nv * 2].reshape(-1, 2)); at += nv * 2
    dif = d[at:at + nv]; at += nv
    spe = d[at:at + nv]; at += nv
    tris = d[at:at + nt * 3].reshape(-1, 3).astype(np.int64); at += nt * 3
    mat = d[at:at + nt].astype(np.int64)
    return streams, pos, uv, extra, dif, spe, tris, mat


def _material(info, tex_dir):
    """The Blender material of a game material: the one already in the file (a model or a world layer brought it in),
    else a new one with its images."""
    name = info["name"]
    m = bpy.data.materials.get(name)
    if m is not None and m.get("g3_material") == name:
        return m
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    for key, socket, normal in (("diffuse", "Base Color", False), ("normal", "Normal", True)):
        p = os.path.join(tex_dir, info[key]) if info.get(key) else None
        if not p or not os.path.isfile(p):
            continue
        t = nt.nodes.new("ShaderNodeTexImage")
        t.image = bpy.data.images.load(p, check_existing=True)
        t.location = (bsdf.location.x - 600, bsdf.location.y - (300 if normal else 0))
        if normal:
            t.image.colorspace_settings.name = "Non-Color"
            nm = nt.nodes.new("ShaderNodeNormalMap")
            nm.location = (bsdf.location.x - 250, bsdf.location.y - 300)
            nt.links.new(t.outputs["Color"], nm.inputs["Color"])
            nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
        else:
            nt.links.new(t.outputs["Color"], bsdf.inputs["Base Color"])
    materials.apply(m, info, tex_dir)
    m["g3_material"] = name
    return m


def build_object(out, scale, name="G3 Terrain"):
    """The cells written by `gothic3-core terrain` as one welded mesh object."""
    cells = out["cells"]
    tex_dir = os.path.dirname(cells[0]["file"]) if cells else ""
    all_streams = sorted({s for c in cells for s in c["uv_streams"]}, key=[15, 21, 18, 73].index)
    P, UV, DIF, SPE, T, M, C = [], [], [], [], [], [], []
    EX = {s: [] for s in all_streams}
    base = 0
    for ci, c in enumerate(cells):
        streams, pos, uv, extra, dif, spe, tris, mat = _read_cell(c["file"])
        P.append(pos)
        T.append(tris + base)
        # Per corner, in the game's order.
        UV.append(uv[tris.ravel()])
        DIF.append(dif[tris.ravel()])
        SPE.append(spe[tris.ravel()])
        for s in all_streams:
            EX[s].append(extra[streams.index(s)][tris.ravel()] if s in streams else np.zeros((tris.size, 2), np.float32))
        M.append(mat)
        C.append(np.full(len(tris), ci, np.int32))
        base += len(pos)
    pos = np.concatenate(P)
    # One vertex per position, across the cells' borders too.
    _, first, inv = np.unique(pos.view(np.dtype((np.void, 12))).ravel(), return_index=True, return_inverse=True)
    order = np.argsort(first)
    remap = np.empty_like(order)
    remap[order] = np.arange(len(order))
    vid = remap[inv.ravel()]
    verts = pos[first[order]][:, [0, 2, 1]] * scale
    tris = vid[np.concatenate(T)]
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts.tolist(), [], tris.tolist())
    uvl = me.uv_layers.new(name="UVMap")
    uv = np.concatenate(UV)
    uvl.data.foreach_set("uv", np.stack([uv[:, 0], 1.0 - uv[:, 1]], axis=1).ravel())
    for s in all_streams:
        me.uv_layers.new(name=f"g3_uv{s}").data.foreach_set("uv", np.concatenate(EX[s]).ravel())
    me.uv_layers.active = uvl
    for attr, data in (("g3_diffuse", DIF), ("g3_light", SPE)):
        ca = me.color_attributes.new(attr, "BYTE_COLOR", "CORNER")
        ca.data.foreach_set("color_srgb", _argb_to_rgba(np.concatenate(data)).ravel())
    me.attributes.new("g3_cell", "INT", "FACE").data.foreach_set("value", np.concatenate(C))
    mats = [_material(i, tex_dir) for i in out["materials"]]
    for m in mats:
        me.materials.append(m)
    me.polygons.foreach_set("material_index", np.concatenate(M).astype(np.int32))
    me.polygons.foreach_set("use_smooth", np.ones(len(me.polygons), bool))
    # No validate(): it would drop the few faces the game's cells have twice (31 in all), and such a cell would
    # not come back as it went out.
    me.update()
    ob = bpy.data.objects.new(name, me)
    ob["g3_terrain"] = json.dumps([{"name": c["name"], "uv_streams": c["uv_streams"]} for c in cells])
    ob["g3_scale"] = scale
    return ob


class G3_OT_import_terrain(bpy.types.Operator):
    bl_idname = "gothic3.import_terrain"
    bl_label = "Gothic 3 Terrain (landscape cells)"
    bl_description = "Клітинки рельєфу як одна сітка для редагування: форма, матеріали, кольори (змішування шарів, освітлення); експорт — Gothic 3 Terrain → Mod"
    bl_options = {"REGISTER", "UNDO"}

    cells: StringProperty(name="Клітинки", description="Назви клітинок через пробіл, напр. G3_Myrtana_Landscape_Cell_244 (досить кінця назви, напр. myrtana_landscape_cell_244)", search=_search, options={"SKIP_SAVE"})
    around_cursor: BoolProperty(name="Навколо 3D-курсора", description="Усі клітинки в радіусі від 3D-курсора (поставте його на місце, імпортоване як шар світу)", default=False)
    radius: FloatProperty(name="Радіус, м", default=50.0, min=0.0, max=2000.0)
    scale: FloatProperty(name="Масштаб", description="Gothic 3 рахує в сантиметрах; 0.01 = метри Blender", default=0.01, min=1e-6)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=460)

    def execute(self, context):
        if self.around_cursor:
            c = context.scene.cursor.location
            query = f"@{c.x / self.scale},{c.y / self.scale},{self.radius / self.scale}"
        else:
            query = self.cells.strip()
        if not query:
            self.report({"ERROR"}, "Вкажіть клітинки або увімкніть «Навколо 3D-курсора»")
            return {"CANCELLED"}
        out_dir = os.path.join(core.cache_dir(), "terrain")
        try:
            out = core.run("terrain", core.game_dir(), query, out_dir)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        ob = build_object(out, self.scale)
        context.collection.objects.link(ob)
        for o in context.selected_objects:
            o.select_set(False)
        ob.select_set(True)
        context.view_layer.objects.active = ob
        for w in out["warnings"][:5]:
            self.report({"WARNING"}, w)
        names = ", ".join(c["name"] for c in out["cells"])
        self.report({"INFO"}, f"Рельєф: {len(out['cells'])} клітинок ({names}), {len(ob.data.vertices)} вершин")
        return {"FINISHED"}


def gather_cells(context, ob, scale, tmp):
    """Per cell of terrain object `ob`: a cell spec for the core (geometry and corner data written into `tmp`)."""
    cells = json.loads(ob["g3_terrain"])
    dg = context.evaluated_depsgraph_get()
    ev = ob.evaluated_get(dg)
    me = ev.to_mesh()
    try:
        me.transform(ob.matrix_world)
        me.calc_loop_triangles()
        nt = len(me.loop_triangles)
        loops = np.empty(nt * 3, np.int32)
        me.loop_triangles.foreach_get("loops", loops)
        poly = np.empty(nt, np.int32)
        me.loop_triangles.foreach_get("polygon_index", poly)
        mi = np.empty(nt, np.int32)
        me.loop_triangles.foreach_get("material_index", mi)
        cell_attr = me.attributes.get("g3_cell")
        if cell_attr is None or cell_attr.domain != "FACE":
            raise core.CoreError(f"{ob.name}: немає атрибута g3_cell — це не рельєф з Import → Gothic 3 Terrain")
        cell = np.empty(len(me.polygons), np.int32)
        cell_attr.data.foreach_get("value", cell)
        cell = np.clip(cell[poly], 0, len(cells) - 1)
        vidx = np.empty(len(me.loops), np.int32)
        me.loops.foreach_get("vertex_index", vidx)
        co = np.empty(len(me.vertices) * 3, np.float32)
        me.vertices.foreach_get("co", co)
        co = co.reshape(-1, 3)
        cn = np.empty(len(me.loops) * 3, np.float32)
        me.corner_normals.foreach_get("vector", cn)
        cn = cn.reshape(-1, 3)

        def loop_layer(layer, width, prop):
            a = np.empty(len(me.loops) * width, np.float32)
            layer.data.foreach_get(prop, a)
            return a.reshape(-1, width)
        uv = loop_layer(me.uv_layers["UVMap"], 2, "uv") if "UVMap" in me.uv_layers else np.zeros((len(me.loops), 2), np.float32)
        colors = {}
        for attr in ("g3_diffuse", "g3_light"):
            ca = me.color_attributes.get(attr)
            if ca is not None and ca.domain == "CORNER":
                colors[attr] = loop_layer(ca, 4, "color_srgb" if ca.data_type == "BYTE_COLOR" else "color")
        slots = [s.material for s in ob.material_slots] or [None]
        from .export_mesh import _material_spec
        mats = [_material_spec(m, tmp) for m in slots]
        specs = []
        for ci, c in enumerate(cells):
            sel = np.nonzero(cell == ci)[0]
            if len(sel) == 0:
                continue
            lp = loops.reshape(-1, 3)[sel].ravel()
            geo = os.path.join(tmp, f"cell{ci}.bin")
            with open(geo, "wb") as f:
                for a in (co[vidx[lp]] / scale, cn[lp]):
                    f.write(np.ascontiguousarray(a[:, [0, 2, 1]], np.float32).tobytes())
                t = uv[lp]
                f.write(np.ascontiguousarray(np.stack([t[:, 0], 1.0 - t[:, 1]], axis=1), np.float32).tobytes())
                f.write(np.clip(mi[sel], 0, len(mats) - 1).astype(np.uint32).tobytes())
            corner = os.path.join(tmp, f"cell{ci}.corners")
            streams = [s for s in c["uv_streams"] if f"g3_uv{s}" in me.uv_layers]
            has_colors = len(colors) == 2
            with open(corner, "wb") as f:
                if has_colors:
                    for attr in ("g3_diffuse", "g3_light"):
                        f.write(_rgba_to_argb(colors[attr][lp]).astype(np.uint32).tobytes())
                for s in streams:
                    f.write(np.ascontiguousarray(loop_layer(me.uv_layers[f"g3_uv{s}"], 2, "uv")[lp], np.float32).tobytes())
            specs.append({"name": c["name"], "geometry": geo, "corners": len(lp), "materials": mats,
                          "corner_data": {"file": corner, "colors": has_colors, "uv_streams": streams}})
        return specs
    finally:
        ev.to_mesh_clear()


class G3_OT_export_terrain(bpy.types.Operator):
    bl_idname = "gothic3.export_terrain"
    bl_label = "Gothic 3 Terrain → Mod"
    bl_description = "Змінені клітинки рельєфу (з Import → Gothic 3 Terrain) назад у гру: форма, кольори, матеріали, колізія, освітлення; незмінені лишаються гри"

    mode: EnumProperty(name="Куди", items=core.MODES, default="mod")
    folder: StringProperty(name="Тека", subtype="DIR_PATH", description="Куди покласти файл мода чи пакет (типово: файл — у теку Mods гри, пакет — у теку модів з налаштувань)")
    title: StringProperty(name="Назва мода", default="Terrain", options={"SKIP_SAVE"})
    collision: EnumProperty(name="Колізія", items=(
        ("auto", "З трикутників", "Колізія клітинки = її трикутники, частина на елемент, як у гри"),
        ("none", "Ні", "Лишити колізію гри як є"),
    ), default="auto")

    @classmethod
    def poll(cls, context):
        return any(o.get("g3_terrain") for o in context.selected_objects)

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self, width=460)

    def execute(self, context):
        terrains = [o for o in context.selected_objects if o.get("g3_terrain") and o.type == "MESH"]
        tmp = tempfile.mkdtemp(prefix="g3_terrain_")
        try:
            cells = []
            for ob in terrains:
                cells += gather_cells(context, ob, float(ob.get("g3_scale", 0.01)), tmp)
            spec = os.path.join(tmp, "terrain.json")
            with open(spec, "w", encoding="utf-8") as f:
                json.dump({"cells": cells, "collision": self.collision}, f)
            out, where = core.deliver("export-terrain", spec, self.mode, self.title or "Terrain", self.title, self.folder)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        r = out["report"]
        for w in r["warnings"][:8]:
            self.report({"WARNING"}, w)
        if self.mode == "install":
            from . import ui
            ui.refresh()
        if r.get("distant"):
            self.report({"INFO"}, f"далекий вигляд землі слідом: {', '.join(r['distant'])}")
        self.report({"INFO"}, f"Рельєф: змінено {len(r['changed'])} ({', '.join(r['changed'])}), без змін {len(r['unchanged'])} → {where}")
        return {"FINISHED"}


_classes = (G3_OT_import_terrain, G3_OT_export_terrain)


def register():
    for c in _classes:
        bpy.utils.register_class(c)


def unregister():
    for c in reversed(_classes):
        bpy.utils.unregister_class(c)
