"""Blender material set-up from what the game's `.xshmat` says (the OBJ/glTF importers know only
diffuse and normal): alpha test with its threshold, alpha blend, additive, and the specular map."""

import os

import bpy


def _bsdf(mat):
    return next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None) if mat and mat.use_nodes else None


def _base_image_node(mat, bsdf):
    s = bsdf.inputs.get("Base Color")
    if s and s.is_linked and s.links[0].from_node.type == "TEX_IMAGE":
        return s.links[0].from_node
    return None


def apply(mat, info, tex_dir):
    """`info` = one material from gothic3-core (`blend`, `mask`, `specular`)."""
    bsdf = _bsdf(mat)
    if bsdf is None:
        return
    nt = mat.node_tree
    img = _base_image_node(mat, bsdf)
    blend, mask = info.get("blend", 0), info.get("mask", 0)
    mat["g3_blend"] = blend
    if img is not None and blend in (1, 2, 7):
        alpha = bsdf.inputs["Alpha"]
        if blend == 1:
            # Alpha test: the game cuts at MaskReference/255.
            cmp = nt.nodes.new("ShaderNodeMath")
            cmp.operation = "GREATER_THAN"
            cmp.inputs[1].default_value = (mask or 128) / 255.0
            cmp.location = (img.location.x + 250, img.location.y - 250)
            nt.links.new(img.outputs["Alpha"], cmp.inputs[0])
            nt.links.new(cmp.outputs[0], alpha)
            mat["g3_mask"] = mask or 128
        else:
            nt.links.new(img.outputs["Alpha"], alpha)
        if hasattr(mat, "surface_render_method"):
            mat.surface_render_method = "DITHERED" if blend == 1 else "BLENDED"
    spec = info.get("specular")
    if spec:
        path = os.path.join(tex_dir, spec)
        if os.path.isfile(path):
            t = nt.nodes.new("ShaderNodeTexImage")
            t.image = bpy.data.images.load(path, check_existing=True)
            t.image.colorspace_settings.name = "Non-Color"
            t.location = (bsdf.location.x - 600, bsdf.location.y - 500)
            target = bsdf.inputs.get("Specular IOR Level") or bsdf.inputs.get("Specular")
            if target is not None:
                nt.links.new(t.outputs["Color"], target)
            t["g3_specular"] = True


def _base(name):
    """`X.001` → `X` (Blender's numbering of a second datablock with the same name)."""
    head, dot, tail = name.rpartition(".")
    return head if dot and len(tail) == 3 and tail.isdigit() else name


def reuse_existing(objects):
    """Materials the OBJ importer made again (`G3_..._A.016`) give way to the one already in the file for that game
    material: objects stay separate, they share one material. Returns the materials still to set up."""
    fresh = []
    for ob in objects:
        for slot in ob.material_slots:
            m = slot.material
            if m is None:
                continue
            base = _base(m.name)
            old = bpy.data.materials.get(base)
            if old is not None and old is not m and old.get("g3_material") == base:
                slot.material = old
                if m.users == 0:
                    imgs = [n.image for n in (m.node_tree.nodes if m.node_tree else ()) if n.type == "TEX_IMAGE" and n.image]
                    bpy.data.materials.remove(m)
                    for i in imgs:
                        if i.users == 0:
                            bpy.data.images.remove(i)
            elif m not in fresh and m.get("g3_material") is None:
                fresh.append(m)
    return fresh


def apply_to_objects(objects, infos, tex_dir):
    by_name = {i["name"].split(".")[0].lower(): i for i in infos}
    for m in reuse_existing(objects):
        info = by_name.get(_base(m.name).lower())
        if info:
            apply(m, info, tex_dir)
            m["g3_material"] = _base(m.name)


# Collision surfaces (`eEShapeMaterial`): what a material sounds like underfoot.
SHAPES = ("none", "wood", "metal", "water", "stone", "earth", "ice", "leather", "clay", "glass", "flesh", "snow", "debris", "foliage", "magic", "grass", "sand")
SHAPE_COLORS = {"wood": (0.55, 0.35, 0.15), "metal": (0.6, 0.65, 0.7), "water": (0.2, 0.4, 0.9), "stone": (0.5, 0.5, 0.5), "earth": (0.45, 0.3, 0.2),
                "clay": (0.3, 0.6, 0.2), "debris": (0.65, 0.6, 0.5), "sand": (0.9, 0.8, 0.5), "snow": (0.95, 0.95, 1.0), "foliage": (0.7, 0.75, 0.3),
                "leather": (0.6, 0.4, 0.3), "grass": (0.4, 0.8, 0.3), "ice": (0.7, 0.9, 1.0)}


def setup_shape_materials(ob):
    """Collision imported as `G3_Shape_<surface>` groups: mark each material with its surface and a viewport colour."""
    for slot in ob.material_slots:
        m = slot.material
        if m is None or not _base(m.name).lower().startswith("g3_shape_"):
            continue
        old = bpy.data.materials.get(_base(m.name))
        if old is not None and old is not m and old.get("g3_shape_material"):
            slot.material = old
            if m.users == 0:
                bpy.data.materials.remove(m)
            continue
        m["g3_shape_material"] = True
        s = _base(m.name)[9:].lower()
        if s in SHAPES:
            m.g3_shape = s
            c = SHAPE_COLORS.get(s, (0.8, 0.2, 0.8))
            m.diffuse_color = (*c, 1.0)


class G3_PT_material_shape(bpy.types.Panel):
    bl_label = "Gothic 3"
    bl_space_type = "PROPERTIES"
    bl_region_type = "WINDOW"
    bl_context = "material"

    @classmethod
    def poll(cls, context):
        return context.material is not None

    def draw(self, context):
        self.layout.prop(context.material, "g3_shape")


def register():
    bpy.types.Material.g3_shape = bpy.props.EnumProperty(
        name="Поверхня колізії",
        description="Звук кроків і ефекти на цій поверхні в грі. Авто — з назви матеріалу (як у гри на рельєфі), інакше типова з діалогу експорту",
        items=[("auto", "Авто", "З назви матеріалу")] + [(s, s, "") for s in SHAPES],
        default="auto",
    )
    bpy.utils.register_class(G3_PT_material_shape)


def unregister():
    bpy.utils.unregister_class(G3_PT_material_shape)
    del bpy.types.Material.g3_shape
