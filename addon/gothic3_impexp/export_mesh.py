"""File > Export > Gothic 3 Model → Mod: the selected objects become one Gothic 3 static mesh.

Geometry goes to gothic3-core as raw arrays already in game space; the core welds vertices, computes
tangents, writes `.xcmsh`, materials and images, collision, and either installs it as a mod (a patch
volume over the game's archives) or builds a mod package (volumes + INSTALL.bat / ROLLBACK.bat).

Axes: Blender (x, y, z) in metres = game (x, z, y) in centimetres ÷ scale — one reflection, the
inverse of the importer's, so triangle order stays as it is (Blender counter-clockwise → Direct3D
clockwise, as Gothic 3's own meshes). V is flipped to Direct3D's V down.
"""

import json
import os
import re
import shutil
import tempfile

import bpy
import numpy as np
from bpy.props import BoolProperty, EnumProperty, FloatProperty, IntProperty, StringProperty

from . import core

SHAPE_MATERIALS = ("none", "wood", "metal", "water", "stone", "earth", "ice", "leather", "clay", "glass", "flesh", "snow", "debris", "foliage", "magic", "grass", "sand")
# What the surfaces mean in the game (clay is what grass and forest floor use).
SHAPE_HINTS = {"clay": "трава, лісова підстилка", "debris": "гравій, річкове каміння", "earth": "земля, стежки", "foliage": "солома, листя"}

# A collision object is named after its model with _COL (triangle mesh) or _CV (convex hull) at the end; Blender may
# add .001 when there are copies.
_COLLISION = re.compile(r"_(col|cv)(\.\d+)?$", re.IGNORECASE)


def collision_kind(name):
    """'col' for a *_COL object, 'cv' for a *_CV object, else None."""
    m = _COLLISION.search(name)
    return m.group(1).lower() if m else None


# Boxes, capsules and spheres of the collision: an entity placing the model holds them itself (the placement sidecar).
_PRIM = re.compile(r"_col_(box|capsule|sphere)(\.\d+)?$", re.IGNORECASE)


def prim_kind(name):
    """'box', 'capsule' or 'sphere' for a *_COL_BOX / *_COL_CAPSULE / *_COL_SPHERE object, else None."""
    m = _PRIM.search(name)
    return m.group(1).lower() if m else None


def primitive(ob, scale):
    """A *_COL_BOX / _CAPSULE / _SPHERE object as the game's shape (centimetres, game axes): its local bounds, placed and
    turned as the object is. A capsule's length is the object's own Z (Blender's capsules and cylinders stand on Z)."""
    kind = prim_kind(ob.name)
    loc, rot, sca = ob.matrix_world.decompose()
    lo = [min(c[k] for c in ob.bound_box) for k in range(3)]
    hi = [max(c[k] for c in ob.bound_box) for k in range(3)]
    from mathutils import Vector
    centre = ob.matrix_world @ Vector([(lo[k] + hi[k]) / 2 for k in range(3)])
    half = [(hi[k] - lo[k]) / 2 * abs(sca[k]) for k in range(3)]
    r = rot.to_matrix()
    game = lambda v: [v[0] / scale, v[2] / scale, v[1] / scale]
    axis = lambda k: [r[0][k], r[2][k], r[1][k]]
    m = ob.active_material
    out = {"kind": kind, "center": game(centre), "axes": [axis(0), axis(1), axis(2)],
           "material": {"name": m.name, **({"shape": m.g3_shape} if getattr(m, "g3_shape", "auto") != "auto" else {})} if m else None}
    if kind == "box":
        out["half"] = [half[0] / scale, half[1] / scale, half[2] / scale]
    elif kind == "capsule":
        radius = max(half[0], half[1])
        out.update(axes=[axis(0), axis(2), axis(1)], radius=radius / scale, height=max(0.0, 2 * half[2] - 2 * radius) / scale)
    else:
        out["radius"] = max(half) / scale
    return out


# The far version the world's distance layer shows is named after its model with _LOWPOLY at the end.
_FAR = re.compile(r"_lowpoly(\.\d+)?$", re.IGNORECASE)


def is_far(name):
    return _FAR.search(name) is not None


def _linked_image(socket):
    """The image feeding `socket`, through a Normal Map node if there is one."""
    if socket is None or not socket.is_linked:
        return None
    node = socket.links[0].from_node
    if node.type == "NORMAL_MAP":
        return _linked_image(node.inputs.get("Color"))
    if node.type == "TEX_IMAGE":
        return node.image
    return None


def _image_png(img, tmp):
    """A PNG on disk for `img`: its own file when it is one, otherwise a saved copy."""
    if img is None:
        return None
    path = bpy.path.abspath(img.filepath) if img.filepath else ""
    if path and os.path.isfile(path) and not img.packed_file and not img.is_dirty and path.lower().endswith(".png"):
        return path
    # Its pixels as Blender holds them (a JPG/TGA on disk, a packed or a painted unsaved image) into a fresh PNG: a
    # copy of the image would have no pixels of its own to save.
    w, h = img.size
    if not w or not h:
        # Its file moved: the same name in the texture folders of the preferences.
        found = core.find_texture(img.filepath or img.name) if (img.filepath or img.name) else None
        if found is None:
            raise core.CoreError(f"Текстура {img.name}: немає даних зображення (файл не знайдено й у теках текстур з налаштувань)")
        if found.lower().endswith(".png"):
            return found
        img = bpy.data.images.load(found, check_existing=True)
        w, h = img.size
        if not w or not h:
            raise core.CoreError(f"Текстура {img.name}: {found} не читається")
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    if img.is_float and img.colorspace_settings.name != "Non-Color":
        # Float images hold linear values; a PNG wants them encoded (sRGB).
        rgb = px.reshape(-1, 4)[:, :3]
        np.clip(rgb, 0.0, 1.0, out=rgb)
        rgb[:] = np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * np.power(rgb, 1 / 2.4) - 0.055)
    out = os.path.join(tmp, f"{len(os.listdir(tmp))}_{bpy.path.clean_name(img.name)}.png")
    c = bpy.data.images.new("g3_export_png", w, h, alpha=True)
    try:
        c.colorspace_settings.name = "Non-Color"
        c.pixels.foreach_set(px)
        c.filepath_raw = out
        c.file_format = "PNG"
        c.save()
    finally:
        bpy.data.images.remove(c)
    return out


def _material_spec(mat, tmp):
    spec = {"name": mat.name if mat else "Default", "diffuse": None, "normal": None}
    shape = getattr(mat, "g3_shape", "auto") if mat else "auto"
    if shape != "auto":
        spec["shape"] = shape
    if mat and mat.use_nodes:
        bsdf = next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf:
            spec["diffuse"] = _image_png(_linked_image(bsdf.inputs.get("Base Color")), tmp)
            spec["normal"] = _image_png(_linked_image(bsdf.inputs.get("Normal")), tmp)
            s = bsdf.inputs.get("Specular IOR Level") or bsdf.inputs.get("Specular")
            spec["specular"] = _image_png(_linked_image(s), tmp)
            spec["blend"] = _blend_kind(mat, bsdf)
            if spec["blend"] == "glow" and spec["diffuse"] is None:
                spec["diffuse"] = _image_png(_linked_image(bsdf.inputs.get("Emission Color")), tmp)
            spec["alpha_test"] = None if spec["blend"] else _alpha_cutoff(mat, bsdf)
    return spec


def _blend_kind(mat, bsdf):
    """'glow' (the game's additive blend mode 7: an emission image, or a game material that was additive), 'blend'
    (see-through by the texture's alpha: Alpha linked straight to it, not through the alpha-test Math node, or a game
    material with blend mode 2), or None (opaque or alpha test)."""
    kind = mat.get("g3_blend")
    if kind == 7:
        return "glow"
    e, strength = bsdf.inputs.get("Emission Color"), bsdf.inputs.get("Emission Strength")
    if e is not None and e.is_linked and _linked_image(e) is not None and (strength is None or strength.is_linked or strength.default_value > 0):
        return "glow"
    a = bsdf.inputs.get("Alpha")
    if a is not None and a.is_linked and kind != 1 and not (a.links[0].from_node.type == "MATH" and a.links[0].from_node.operation == "GREATER_THAN"):
        return "blend"
    return None


def _alpha_cutoff(mat, bsdf):
    """The alpha-test cut-off (0..255) when the material's Alpha is driven by its texture, else None.
    A 'greater than' Math node in between gives the threshold (as Gothic 3 Model import builds it)."""
    a = bsdf.inputs.get("Alpha")
    if a is None or not a.is_linked:
        return None
    node = a.links[0].from_node
    if node.type == "MATH" and node.operation == "GREATER_THAN":
        return max(1, min(255, round(node.inputs[1].default_value * 255)))
    return int(mat.get("g3_mask", 128))


# UV layers that carry the game's further coordinate sets (see import_mesh.UV_LAYERS): stream type → name.
UV_EXTRA = {t: f"g3_uv{t}" for t in (15, 21, 18, 73)}


def _corner_uv(me, layer):
    u = np.empty(len(me.loops) * 2, np.float32)
    layer.data.foreach_get("uv", u)
    return u.reshape(-1, 2)


def gather(context, objects, scale, tmp):
    """Corner arrays of all `objects` (modifiers applied, world space) in game space, plus materials, plus the further
    coordinate sets ({stream type: per corner}, the game's numbers) that any object carries as g3_uv15 / 21 / 18 / 73."""
    dg = context.evaluated_depsgraph_get()
    pos, nrm, uv, mat = [], [], [], []
    extra, carried = {t: [] for t in UV_EXTRA}, set()
    materials, slot_of = [], {}
    for ob in objects:
        ev = ob.evaluated_get(dg)
        me = ev.to_mesh()
        try:
            me.transform(ob.matrix_world)
            if ob.matrix_world.determinant() < 0:
                me.flip_normals()
            me.calc_loop_triangles()
            nt = len(me.loop_triangles)
            if nt == 0:
                continue
            loops = np.empty(nt * 3, np.int32)
            me.loop_triangles.foreach_get("loops", loops)
            mi = np.empty(nt, np.int32)
            me.loop_triangles.foreach_get("material_index", mi)
            vidx = np.empty(len(me.loops), np.int32)
            me.loops.foreach_get("vertex_index", vidx)
            co = np.empty(len(me.vertices) * 3, np.float32)
            me.vertices.foreach_get("co", co)
            co = co.reshape(-1, 3)
            cn = np.empty(len(me.loops) * 3, np.float32)
            me.corner_normals.foreach_get("vector", cn)
            cn = cn.reshape(-1, 3)
            # The first texture set: the active layer, unless that is one of the further sets.
            first = me.uv_layers.active if me.uv_layers.active and me.uv_layers.active.name not in UV_EXTRA.values() else next((l for l in me.uv_layers if l.name not in UV_EXTRA.values()), None)
            u = _corner_uv(me, first) if first else np.zeros((len(me.loops), 2), np.float32)
            for t, name in UV_EXTRA.items():
                layer = me.uv_layers.get(name)
                if layer is None:
                    # No lightmap texel (-1); a texture set the object lacks is 0.
                    extra[t].append(np.full((nt * 3, 2), -1.0 if t == 73 else 0.0, np.float32))
                    continue
                carried.add(t)
                x = _corner_uv(me, layer)[loops]
                extra[t].append(x)
            p = co[vidx[loops]] / scale
            n = cn[loops]
            t = u[loops]
            pos.append(p[:, [0, 2, 1]])
            nrm.append(n[:, [0, 2, 1]])
            uv.append(np.stack([t[:, 0], 1.0 - t[:, 1]], axis=1))
            # Object material slots → one global list, shared by name.
            slots = [s.material for s in ob.material_slots] or [None]
            remap = []
            for m in slots:
                key = m.name if m else "Default"
                if key not in slot_of:
                    slot_of[key] = len(materials)
                    materials.append(_material_spec(m, tmp))
                remap.append(slot_of[key])
            mat.append(np.array(remap, np.uint32)[np.clip(mi, 0, len(remap) - 1)])
        finally:
            ev.to_mesh_clear()
    if not pos:
        return None
    sets = {t: np.concatenate(extra[t]).astype(np.float32) for t in UV_EXTRA if t in carried}
    return (np.concatenate(pos).astype(np.float32), np.concatenate(nrm).astype(np.float32),
            np.concatenate(uv).astype(np.float32), np.concatenate(mat).astype(np.uint32), materials, sets)


def _scene_has_light(scene):
    """A lamp that renders, or a world whose background gives light."""
    if any(o.type == "LIGHT" and not o.hide_render for o in scene.objects):
        return True
    w = scene.world
    if w is None or not w.use_nodes:
        return w is not None and any(c > 0.0 for c in w.color)
    bg = next((n for n in w.node_tree.nodes if n.type == "BACKGROUND"), None)
    if bg is None:
        return False
    col, strength = bg.inputs["Color"], bg.inputs["Strength"]
    return strength.default_value > 0.0 and (col.is_linked or any(c > 0.0 for c in col.default_value[:3]))


def bake_light(context, objects, tmp, size, samples):
    """The scene's light on `objects`, baked by Cycles (diffuse lighting only: lamps, sky, bounces; no colour) into one
    picture: copies of the objects get an atlas UV layer and are baked while the originals stay out of the render.
    Returns the spec's `bake`: the picture, and per corner (in gather()'s order) its place in it. The scene, its
    materials and the objects are left as they were."""
    scene = context.scene
    if not _scene_has_light(scene):
        raise core.CoreError("Для запікання потрібне світло в сцені: лампа або небо (World) з силою > 0")
    dg = context.evaluated_depsgraph_get()
    copies, hidden, added, temp_mats = [], [], [], []
    img = bpy.data.images.new("g3_bake", size, size, alpha=False)
    engine, samples_before = scene.render.engine, getattr(scene.cycles, "samples", None)
    selected, active = list(context.selected_objects), context.view_layer.objects.active
    try:
        for ob in objects:
            me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg))
            cp = bpy.data.objects.new(f"{ob.name}_g3bake", me)
            cp.matrix_world = ob.matrix_world
            scene.collection.objects.link(cp)
            copies.append((ob, cp))
        for ob in objects:
            if not ob.hide_render:
                ob.hide_render = True
                hidden.append(ob)
        for o in context.selected_objects:
            o.select_set(False)
        for _, cp in copies:
            cp.data.uv_layers.active = cp.data.uv_layers.new(name="G3_Bake")
            cp.select_set(True)
        context.view_layer.objects.active = copies[0][1]
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.lightmap_pack(PREF_CONTEXT="ALL_FACES", PREF_PACK_IN_ONE=True, PREF_NEW_UVLAYER=False, PREF_MARGIN_DIV=0.3)
        bpy.ops.object.mode_set(mode="OBJECT")
        # The picture as the bake target in every material of the copies (a stand-in where a slot has none).
        for _, cp in copies:
            if not cp.data.materials:
                cp.data.materials.append(None)
            for i, m in enumerate(cp.data.materials):
                if m is None or not m.use_nodes:
                    m = bpy.data.materials.new("g3_bake_stand_in")
                    m.use_nodes = True
                    temp_mats.append(m)
                    cp.data.materials[i] = m
                if any(a[0] is m for a in added):
                    continue
                n = m.node_tree.nodes.new("ShaderNodeTexImage")
                n.image = img
                added.append((m, n, m.node_tree.nodes.active))
                m.node_tree.nodes.active = n
        scene.render.engine = "CYCLES"
        scene.cycles.samples = samples
        bpy.ops.object.bake(type="DIFFUSE", pass_filter={"DIRECT", "INDIRECT"}, margin=4, use_clear=True)
        path = os.path.join(tmp, "bake.png")
        img.filepath_raw = path
        img.file_format = "PNG"
        img.save()
        # Each corner's place, in the order gather() takes the corners (a mirrored object is turned the same way).
        uvs = []
        for ob, cp in copies:
            me = cp.data
            if ob.matrix_world.determinant() < 0:
                me.flip_normals()
            me.calc_loop_triangles()
            nt = len(me.loop_triangles)
            if nt == 0:
                continue
            loops = np.empty(nt * 3, np.int32)
            me.loop_triangles.foreach_get("loops", loops)
            uvs.append(_corner_uv(me, me.uv_layers["G3_Bake"])[loops])
        geo = os.path.join(tmp, "bake_uv.bin")
        with open(geo, "wb") as f:
            f.write(np.ascontiguousarray(np.concatenate(uvs).astype(np.float32)).tobytes())
        return {"image": path, "geometry": geo}
    finally:
        if context.object and context.object.mode != "OBJECT":
            bpy.ops.object.mode_set(mode="OBJECT")
        for m, n, before in added:
            m.node_tree.nodes.remove(n)
            m.node_tree.nodes.active = before
        for _, cp in copies:
            me = cp.data
            bpy.data.objects.remove(cp)
            bpy.data.meshes.remove(me)
        for m in temp_mats:
            bpy.data.materials.remove(m)
        bpy.data.images.remove(img)
        for ob in hidden:
            ob.hide_render = False
        scene.render.engine = engine
        if samples_before is not None:
            scene.cycles.samples = samples_before
        for o in selected:
            if o.name in bpy.data.objects:
                o.select_set(True)
        context.view_layer.objects.active = active


def _write_geometry(g, tmp, prefix):
    """A gather() result as the core's files: corner arrays, further coordinate sets; the spec's part for them."""
    pos, nrm, uv, mat, materials, sets = g
    geo = os.path.join(tmp, f"{prefix}.bin")
    with open(geo, "wb") as f:
        for a in (pos, nrm, uv, mat):
            f.write(np.ascontiguousarray(a).tobytes())
    out = {"geometry": geo, "corners": len(pos), "materials": materials}
    if sets:
        # The further sets as the core's corner data (as the terrain sends them), in the game's stream order.
        p = os.path.join(tmp, f"{prefix}_corners.bin")
        order = [t for t in (15, 21, 18, 73) if t in sets]
        with open(p, "wb") as f:
            for t in order:
                f.write(np.ascontiguousarray(sets[t], np.float32).tobytes())
        out["corner_data"] = {"file": p, "colors": False, "uv_streams": order}
    return out


def _default_name(context):
    ob = context.active_object
    if ob is None:
        return ""
    entry = ob.get("g3_entry")
    if entry:
        return entry.rsplit("/", 1)[-1].split(".", 1)[0]
    # As it is (the core makes it a file name, Cyrillic transliterated).
    return ob.name.split(".")[0]


class G3_OT_export_mesh(bpy.types.Operator):
    bl_idname = "gothic3.export_mesh"
    bl_label = "Gothic 3 Model → Mod (.xcmsh)"
    bl_description = "Виділені об'єкти → модель Gothic 3: замінити наявну (та сама назва) або додати нову; встановити в гру або зібрати пакет мода"

    name: StringProperty(name="Назва моделі", description="Назва .xcmsh у грі. Наявна назва — заміна (напр. G3_Object_Barrel_02), нова — додати", options={"SKIP_SAVE"})
    mode: EnumProperty(name="Куди", items=core.MODES, default="mod")
    folder: StringProperty(name="Тека", subtype="DIR_PATH", description="Куди покласти файл мода чи пакет (типово: файл — у теку Mods гри, пакет — у теку модів з налаштувань)")
    title: StringProperty(name="Назва мода", description="Як мод зветься в лаунчері чи INSTALL.bat (типово — назва моделі)", options={"SKIP_SAVE"})
    scale: FloatProperty(name="Масштаб", description="Як при імпорті: 0.01 = метри Blender", default=0.01, min=1e-6)
    collision: EnumProperty(name="Колізія", items=(
        ("auto", "Так", "З об'єктів *_COL (опукла *_CV — з об'єктів *_CV або з самої моделі); без них — з самої моделі, якщо вона нова або це рельєф (його колізія = його трикутники); інакше колізія гри лишається"),
        ("model", "З моделі", "Колізія з трикутників самої моделі, навіть якщо є *_COL чи колізія гри"),
        ("none", "Ні", "Лишити колізію гри як є (або без колізії)"),
    ), default="auto")
    bake: BoolProperty(name="Запекти світло в Blender", description="Для змінених частин будівель, які гра освітлює сторінками лайтмап: світло зі сцени Blender (лампи, небо; Cycles) замість перенесення зі старих сторінок", default=False)
    bake_size: IntProperty(name="Розмір запікання", description="Сторона картинки запікання в пікселях", default=1024, min=64, max=8192)
    bake_samples: IntProperty(name="Семпли", description="Семпли Cycles на піксель запікання", default=64, min=1, max=4096)
    shape_material: EnumProperty(name="Поверхня (типово)", description="Для матеріалів, у яких поверхню не задано й не вгадано з назви (панель матеріалу → Gothic 3: поверхня)", items=[(m, m, SHAPE_HINTS.get(m, "")) for m in SHAPE_MATERIALS], default="stone")

    @classmethod
    def poll(cls, context):
        return any(o.type == "MESH" for o in context.selected_objects)

    def invoke(self, context, event):
        if not self.name:
            self.name = _default_name(context)
        return context.window_manager.invoke_props_dialog(self, width=460)

    def draw(self, context):
        col = self.layout.column()
        col.prop(self, "name")
        col.prop(self, "mode", expand=True)
        if self.mode != "install":
            col.prop(self, "folder")
            col.prop(self, "title")
        col.prop(self, "scale")
        col.prop(self, "collision")
        if self.collision != "none":
            col.prop(self, "shape_material")
        col.prop(self, "bake")
        if self.bake:
            row = col.row()
            row.prop(self, "bake_size")
            row.prop(self, "bake_samples")

    def execute(self, context):
        selected = [o for o in context.selected_objects if o.type == "MESH"]
        # Stand-ins brought in to look at (SpeedTree) are never models.
        view_only = [o for o in selected if o.get("g3_view_only")]
        if view_only:
            self.report({"WARNING"}, f"Пропущено {len(view_only)} заглушок лише для огляду (дерева SpeedTree)")
            selected = [o for o in selected if not o.get("g3_view_only")]
        # Objects named *_COL are the collision shape, *_CV the convex hulls (the bodies of items), the rest the model.
        col_objects = [o for o in selected if collision_kind(o.name) == "col"]
        cv_objects = [o for o in selected if collision_kind(o.name) == "cv"]
        # *_LOWPOLY objects are the far version, *_COL_BOX / _CAPSULE / _SPHERE the collision's boxes, capsules, spheres.
        prim_objects = [o for o in selected if prim_kind(o.name)]
        far_objects = [o for o in selected if collision_kind(o.name) is None and not prim_kind(o.name) and is_far(o.name)]
        objects = [o for o in selected if collision_kind(o.name) is None and not prim_kind(o.name) and not is_far(o.name)]
        # A model imported with its collision carries it as a *_COL / *_CV child: it goes along unselected (and so does
        # a *_LOWPOLY child).
        for o in objects:
            for c in o.children_recursive:
                kind = collision_kind(c.name) if c.type == "MESH" else None
                if kind == "col" and c not in col_objects:
                    col_objects.append(c)
                if kind == "cv" and c not in cv_objects:
                    cv_objects.append(c)
                if kind is None and c.type == "MESH" and is_far(c.name) and c not in far_objects:
                    far_objects.append(c)
                if prim_kind(c.name) and c not in prim_objects:
                    prim_objects.append(c)
        if not objects or not self.name:
            self.report({"ERROR"}, "Виділіть меш і вкажіть назву моделі")
            return {"CANCELLED"}
        tmp = tempfile.mkdtemp(prefix="g3_export_")
        try:
            g = gather(context, objects, self.scale, tmp)
            if g is None:
                self.report({"ERROR"}, "У виділених об'єктах немає трикутників")
                return {"CANCELLED"}
            spec = os.path.join(tmp, "spec.json")
            doc = {"name": self.name, **_write_geometry(g, tmp, "model")}
            if self.bake:
                doc["bake"] = bake_light(context, [o for o in objects if o.type == "MESH"], tmp, self.bake_size, self.bake_samples)
            doc["primitives"] = [primitive(o, self.scale) for o in sorted(prim_objects, key=lambda o: o.name)]
            fg = gather(context, far_objects, self.scale, tmp) if far_objects else None
            if fg is not None:
                doc["lowpoly"] = _write_geometry(fg, tmp, "far")
            if self.collision != "none":
                c = {"material": self.shape_material, "mode": self.collision, "convex": []}
                cg = gather(context, col_objects, self.scale, tmp) if col_objects else None
                if cg is not None:
                    cgeo = os.path.join(tmp, "collision.bin")
                    with open(cgeo, "wb") as f:
                        for a in cg[:4]:
                            f.write(np.ascontiguousarray(a).tobytes())
                    c.update(geometry=cgeo, corners=len(cg[0]), materials=cg[4])
                # One hull per *_CV object, in name order (the game's convex collisions have one part, rarely two).
                for i, o in enumerate(sorted(cv_objects, key=lambda o: o.name)):
                    hg = gather(context, [o], self.scale, tmp)
                    if hg is None:
                        continue
                    hgeo = os.path.join(tmp, f"convex{i}.bin")
                    with open(hgeo, "wb") as f:
                        for a in hg[:4]:
                            f.write(np.ascontiguousarray(a).tobytes())
                    c["convex"].append({"geometry": hgeo, "corners": len(hg[0]), "materials": hg[4]})
                doc["collision"] = c
            with open(spec, "w", encoding="utf-8") as f:
                json.dump(doc, f)
            out, where = core.deliver("export", spec, self.mode, self.name, self.title, self.folder)
        except core.CoreError as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        r = out["report"]
        for w in r["warnings"]:
            self.report({"WARNING"}, w)
        what = "замінено" if r["replaced"] else "додано"
        if self.mode == "install":
            from . import ui
            ui.refresh()
        if r.get("collision"):
            self.report({"INFO"}, f"колізія: {r['collision']}")
        if r.get("convex"):
            self.report({"INFO"}, f"опукла колізія: {r['convex']}")
        if r.get("lods"):
            self.report({"INFO"}, f"разом з LOD: {', '.join(r['lods'])}")
        if r.get("lightmaps"):
            self.report({"INFO"}, f"освітлення перенесено на {r['lightmaps']} розміщень у світі")
        if r.get("placement"):
            # For a world editor: what an entity placing this model needs (its collision shapes); a package has it too.
            side = os.path.join(core.cache_dir(), "placement", f"{r['name']}.placement.json")
            os.makedirs(os.path.dirname(side), exist_ok=True)
            with open(side, "w", encoding="utf-8") as f:
                json.dump(r["placement"], f, indent=1, ensure_ascii=False)
            self.report({"INFO"}, f"дані для розміщення в редакторі світу: {side}")
        if r.get("pages_kept"):
            self.report({"INFO"}, f"сторінки лайтмап гри збережено для {r['pages_kept']} частин (форма та сама)")
        if r.get("pages_baked"):
            self.report({"INFO"}, f"світло запечено в Blender для {r['pages_baked']} нових сторінок")
        if r.get("pages_rebaked"):
            self.report({"INFO"}, f"нові сторінки лайтмап (світло перенесено зі старих) для {r['pages_rebaked']} змінених частин")
        self.report({"INFO"}, f"{self.name} ({what}): {r['vertices']} вершин, {r['triangles']} трикутників → {where}")
        return {"FINISHED"}


def register():
    bpy.utils.register_class(G3_OT_export_mesh)


def unregister():
    bpy.utils.unregister_class(G3_OT_export_mesh)
