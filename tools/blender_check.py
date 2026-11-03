"""A run through the add-on in Blender: names, textures, frame effects, an item with its convex body, a model whose
collision has no _COL, an actor. Writes only into the folder given after `--` (mod packages, never into the game).

    blender -b --factory-startup --python tools/blender_check.py -- <folder>

The add-on must be enabled-able in that Blender (installed, or linked into its scripts/addons); the game folder is
G3_GAME or the Steam default. Ends with "RESULT DONE fails: []" when all is well."""
import bpy, os, sys, json, tempfile, traceback
S = sys.argv[sys.argv.index("--") + 1]
import addon_utils
addon_utils.enable("gothic3_impexp", default_set=True)
from gothic3_impexp import prefs as P, export_actor, export_mesh, effects, core, materials
from mathutils import Vector
pr = P.prefs()
pr.game_dir = os.environ.get("G3_GAME", r"C:\Program Files (x86)\Steam\steamapps\common\Gothic 3")
pr.cache_dir = os.path.join(S, "chk_cache")
pr.mods_dir = os.path.join(S, "chk_mods")
OUT = os.path.join(S, "chk_out")
os.makedirs(OUT, exist_ok=True)
fails = []


def R(*a):
    print("RESULT", *a)


def check(cond, what):
    R("OK " if cond else "FAIL", what)
    if not cond:
        fails.append(what)


def clear():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o)


try:
    # 1. Collision names: only an ending counts.
    check([export_mesh.collision_kind(n) for n in ["Stone_Column", "Wall_Colored", "X_COL", "X_COL.001", "X_cv", "Barrel_CV.012", "Body_Collar"]]
          == [None, None, "col", "col", "cv", "cv", None], "collision_kind")

    # 1b. Name searches with no game folder set give nothing instead of an error in the dialog.
    from gothic3_impexp import world, import_mesh, import_actor
    game = pr.game_dir
    pr.game_dir = os.path.join(S, "no game here")
    got = {}
    for label, fn, op in [("world", world._search_layer, None), ("mesh", import_mesh._search, type("O", (), {"category": "all"})()), ("actor", import_actor._search_actor, None)]:
        try:
            got[label] = fn(op, bpy.context, "ardea")
        except Exception as e:
            got[label] = repr(e)
    pr.game_dir = game
    check(all(v == [] for v in got.values()), f"searches without a game folder: {got}")

    # 2. Images: JPG on disk, painted unsaved, packed.
    def mk(name, fmt, ext):
        im = bpy.data.images.new(name, 8, 8)
        im.pixels = [0.5, 0.2, 0.1, 1.0] * 64
        p = os.path.join(OUT, name + ext)
        im.filepath_raw = p
        im.file_format = fmt
        im.save()
        bpy.data.images.remove(im)
        return bpy.data.images.load(p)
    tmp = tempfile.mkdtemp(dir=OUT)
    jpg = mk("tjpg", "JPEG", ".jpg")
    painted = bpy.data.images.new("painted", 8, 8)
    painted.pixels = [1, 0, 0, 1] * 64
    packed = mk("tpk", "PNG", ".png")
    packed.pack()
    for label, img, want in [("jpg", jpg, (0.5, 0.2, 0.1)), ("painted", painted, (1.0, 0.0, 0.0)), ("packed", packed, (0.5, 0.2, 0.1))]:
        p = export_mesh._image_png(img, tmp)
        chk = bpy.data.images.load(p)
        px = tuple(round(x, 1) for x in chk.pixels[:3])
        check(os.path.isfile(p) and px == tuple(round(x, 1) for x in want), f"image {label}: {px}")

    # 3. Frame effects: two on one frame survive store -> as_times.
    a = bpy.data.actions.new("fx_test")
    effects.store(bpy.context.scene, a, [(0.5, "eff_a"), (0.5, "eff_b"), (1.0, "eff_c")])
    got = effects.as_times(bpy.context.scene, a)
    check(sorted(n for _, n in got) == ["eff_a", "eff_b", "eff_c"], f"effects on one frame: {got}")

    # 4. Item import brings _COL and _CV; export as a package writes both.
    clear()
    r = bpy.ops.gothic3.import_mesh(name="G3_Armor_Helm_Myrtana_01")
    names = sorted(o.name for o in bpy.data.objects)
    check("G3_Armor_Helm_Myrtana_01_CV" in names, f"helm import: {names}")
    hull = bpy.data.objects.get("G3_Armor_Helm_Myrtana_01_CV")
    if hull:
        import bmesh
        bm = bmesh.new(); bm.from_mesh(hull.data)
        vol = bm.calc_volume(signed=True); bm.free()
        check(vol > 0, f"hull faces point out (signed volume {vol:.5f})")
    model = bpy.data.objects["G3_Armor_Helm_Myrtana_01"]
    bpy.ops.object.select_all(action="DESELECT")
    model.select_set(True)
    bpy.context.view_layer.objects.active = model
    seen = {}
    orig = core.run

    def spy(cmd, *args):
        out = orig(cmd, *args)
        if cmd in ("export", "export-actor"):
            seen["spec"] = json.load(open(args[1], encoding="utf-8"))
            seen["out"] = out
        return out
    core.run = spy
    pkg = os.path.join(OUT, "helm_pkg")
    r = bpy.ops.gothic3.export_mesh(name="G3_Armor_Helm_Myrtana_01", mode="package", folder=pkg, title="Helm (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    check(r == {"FINISHED"} and rep.get("convex") and "*_CV objects" in rep["convex"], f"helm export convex: {rep.get('convex')}")
    check(len(seen.get("spec", {}).get("collision", {}).get("convex", [])) == 1, "the _CV child went along unselected")
    files = sorted(os.listdir(pkg))
    check({"INSTALL.bat", "ROLLBACK.bat", "_compiledPhysic.vol", "_compiledPhysic.empty"} <= set(files), f"package: {files}")
    # 4a. The placement sidecar in the package and the cache: the helm's shapes, its convex body among them.
    side = [os.path.join(pkg, "G3_Armor_Helm_Myrtana_01.placement.json"), os.path.join(pr.cache_dir, "placement", "G3_Armor_Helm_Myrtana_01.placement.json")]
    kinds = [s["ShapeType"] for s in json.load(open(side[0], encoding="utf-8"))["eCCollisionShape_PS"]["shapes"]] if os.path.isfile(side[0]) else []
    check(6 in kinds and os.path.isfile(side[1]), f"placement sidecar: shape types {kinds}")

    # 4b. The same as one mod file (the default): a ZIP with mod.json and the files under their archives.
    import zipfile
    r = bpy.ops.gothic3.export_mesh(name="G3_Armor_Helm_Myrtana_01", mode="mod", folder=OUT, title="Helm (test)")
    mf = os.path.join(OUT, "Helm (test).g3mod")
    ok = r == {"FINISHED"} and os.path.isfile(mf)
    if ok:
        with zipfile.ZipFile(mf) as z:
            m = json.loads(z.read("mod.json"))
            names = z.namelist()
        ok = m["game"] == "gothic3" and any(n.startswith("files/_compiledPhysic/") for n in names) and all(f"files/{f}" in names for f in m["files"])
    check(ok, f"mod file: {mf if os.path.isfile(mf) else 'missing'}")

    # 5. A model whose collision PhysX keeps as an open mesh (mass -1): its _COL imports now.
    clear()
    bpy.ops.gothic3.import_mesh(name="G3_Myrtana_Faring_Castle_Wall_Stairs_01")
    check(any(o.name.endswith("_COL") for o in bpy.data.objects), f"open-mesh collision import: {[o.name for o in bpy.data.objects]}")

    # 6. Actor export ignores where the character stands.
    clear()
    bpy.ops.gothic3.import_character(name="G3_Wolf_Body_01", with_head=False)
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    g0 = export_actor.gather_skinned(bpy.context, meshes, 0.01, tempfile.mkdtemp(dir=OUT))
    arm.location.x = 5.0
    bpy.context.view_layer.update()
    g1 = export_actor.gather_skinned(bpy.context, meshes, 0.01, tempfile.mkdtemp(dir=OUT))
    dx = float(abs(g0[0][:, 0].mean() - g1[0][:, 0].mean()))
    check(dx < 0.01 and g0[8] == 0, f"actor skin moves {dx:.3f} cm with the armature; unweighted {g0[8]}")
    # 6b. A new actor typed with Cyrillic and a slash comes out as a file name in the package.
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    seen.clear()
    core.run = spy
    r = bpy.ops.gothic3.export_actor(name="Вовк/новий", mode="package", folder=os.path.join(OUT, "wolf_pkg"), title="Wolf (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    check(r == {"FINISHED"} and rep.get("actor") == "Vovk_novyi" and not rep.get("replaced") and os.path.isfile(os.path.join(OUT, "wolf_pkg", "_compiledAnimation.vol")), f"new actor: {rep.get('actor')!r}")

    # 7. Cyrillic name reaches the core as it is.
    clear()
    bpy.ops.mesh.primitive_cube_add()
    c = bpy.context.active_object
    c.name = "Млин"
    check(export_mesh._default_name(bpy.context) == "Млин", f"default name {export_mesh._default_name(bpy.context)!r}")

    # 8. A world layer as context: Ardea's dynamic objects with the town's houses 15 m around and the ground; the
    #    anvil where the game has it (x 89775, z -12670 cm), meshes linked, materials shared.
    clear()
    r = bpy.ops.gothic3.import_world(name="G3_Myrtana_01_Ardea_Dynamic_Objects_01_shyb", radius=15.0, with_ground=True)
    col = next((c for c in bpy.data.collections if c.get("g3_world_layer")), None)
    obs = list(col.objects) if col else []
    anvil = next((o for o in obs if o.name.lower().startswith("g3_object_interact_anvil_01")), None)
    at = tuple(round(x, 1) for x in anvil.matrix_world.translation) if anvil else None
    statics = sum(1 for o in obs if o.get("g3_source", "").lower().endswith("_cstat"))
    ground = sum(1 for o in obs if o.get("g3_source") == "landscape")
    dup = [m.name for m in bpy.data.materials if materials._base(m.name) != m.name and bpy.data.materials.get(materials._base(m.name))]
    linked = len({o.data.name for o in obs}) < len(obs)
    check(r == {"FINISHED"} and at is not None and abs(at[0] - 897.8) < 0.2 and abs(at[1] + 126.7) < 0.2 and statics > 20 and ground > 0 and linked and not dup,
          f"world layer: {len(obs)} objects, {statics} static, {ground} ground, anvil at {at}, duplicate materials {dup[:3]}")
    if os.environ.get("G3_RENDER"):
        sc = bpy.context.scene
        cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
        sc.collection.objects.link(cam)
        cam.location = (anvil.location.x + 18, anvil.location.y - 18, anvil.location.z + 12)
        cam.rotation_euler = (Vector(anvil.location) - cam.location).to_track_quat("-Z", "Y").to_euler()
        sc.camera = cam
        sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
        sun.rotation_euler = (0.8, 0.2, 0.6)
        sc.collection.objects.link(sun)
        sc.render.resolution_x, sc.render.resolution_y = 1280, 720
        sc.render.filepath = os.path.join(OUT, "world_ardea.png")
        bpy.ops.render.render(write_still=True)
    # 9. New materials by the look of their nodes: opaque, alpha test (Math node), blend (Alpha straight from the
    #    image), glow (an emission image); exported as a package, each takes the game's template of its look.
    clear()
    looks = {}
    img = bpy.data.images.new("look_px", 8, 8, alpha=True)
    img.pixels = [0.8, 0.4, 0.1, 0.5] * 64
    img.filepath_raw = os.path.join(OUT, "look_px.png")
    img.file_format = "PNG"
    img.save()
    for i, kind in enumerate(["opaque", "alpha", "blend", "glow"]):
        bpy.ops.mesh.primitive_plane_add(location=(i * 3, 0, 0))
        ob = bpy.context.active_object
        ob.name = f"LookTest_{kind}"
        m = bpy.data.materials.new(f"LookTest_{kind}")
        m.use_nodes = True
        nt = m.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        t = nt.nodes.new("ShaderNodeTexImage")
        t.image = img
        nt.links.new(t.outputs["Color"], bsdf.inputs["Emission Color" if kind == "glow" else "Base Color"])
        if kind == "glow":
            bsdf.inputs["Emission Strength"].default_value = 2.0
        if kind == "alpha":
            cmp = nt.nodes.new("ShaderNodeMath")
            cmp.operation = "GREATER_THAN"
            cmp.inputs[1].default_value = 0.4
            nt.links.new(t.outputs["Alpha"], cmp.inputs[0])
            nt.links.new(cmp.outputs[0], bsdf.inputs["Alpha"])
        if kind == "blend":
            nt.links.new(t.outputs["Alpha"], bsdf.inputs["Alpha"])
        ob.data.materials.append(m)
        s = export_mesh._material_spec(m, tempfile.mkdtemp(dir=OUT))
        looks[kind] = (s.get("blend"), s.get("alpha_test"), bool(s.get("diffuse")))
    check(looks == {"opaque": (None, None, True), "alpha": (None, 102, True), "blend": ("blend", None, True), "glow": ("glow", None, True)}, f"material looks: {looks}")
    bpy.ops.object.select_all(action="SELECT")
    seen.clear()
    core.run = spy
    r = bpy.ops.gothic3.export_mesh(name="LookTest_Stand", mode="package", folder=os.path.join(OUT, "looks_pkg"), title="Looks (test)", collision="none")
    core.run = orig
    mats = seen.get("out", {}).get("report", {}).get("materials", [])
    want = ["(opaque)", "(alpha test)", "(blend)", "(glow)"]
    check(r == {"FINISHED"} and len(mats) == 4 and all(any(m.endswith(w) for m in mats) for w in want), f"looks exported: {mats}")

    # 10. A building lit by lightmap pages: its lightmap coordinates come in as a UV layer, a wall's second texture set as
    #     another; exported back unchanged, every part keeps the game's pages.
    clear()
    bpy.ops.gothic3.import_mesh(name="G3_Myrtana_Trelis_Channel_Wall_01", with_collision=False)
    wall = bpy.data.objects.get("G3_Myrtana_Trelis_Channel_Wall_01")
    check(wall is not None and wall.data.uv_layers.get("G3_UV2") is not None and wall.data.uv_layers.active.name == "UVMap", f"second texture set: {[l.name for l in wall.data.uv_layers] if wall else None}")
    clear()
    bpy.ops.gothic3.import_mesh(name="G3_Architecture_Arena_Manege_01", with_collision=False)
    arena = bpy.data.objects.get("G3_Architecture_Arena_Manege_01")
    layers = [l.name for l in arena.data.uv_layers] if arena else []
    check("G3_Lightmap" in layers, f"lightmap coordinates: {layers}")
    bpy.ops.object.select_all(action="DESELECT")
    arena.select_set(True)
    bpy.context.view_layer.objects.active = arena
    seen.clear()
    core.run = spy
    r = bpy.ops.gothic3.export_mesh(name="G3_Architecture_Arena_Manege_01", mode="package", folder=os.path.join(OUT, "arena_pkg"), title="Arena (test)", collision="none")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    streams = sorted(u["stream"] for u in seen.get("spec", {}).get("uv_extra", []))
    check(r == {"FINISHED"} and streams == [73] and rep.get("pages_kept", 0) > 0 and not any("per vertex" in w for w in rep.get("warnings", [])),
          f"pages kept: {rep.get('pages_kept')} over {rep.get('lightmaps')} lightmaps, streams {streams}, warnings {rep.get('warnings')}")

    # 11. A far version: a *_LOWPOLY object goes to the distance layer's mesh, not into the model.
    check([export_mesh.is_far(n) for n in ["Tower_LOWPOLY", "Tower_lowpoly.002", "Lowpoly_Tower", "Tower"]] == [True, True, False, False], "is_far")
    clear()
    bpy.ops.mesh.primitive_cube_add(location=(0, 0, 0))
    tower = bpy.context.active_object
    tower.name = "FarTest_Tower"
    bpy.ops.mesh.primitive_plane_add(location=(0, 0, 0))
    bpy.context.active_object.name = "FarTest_Tower_LOWPOLY"
    bpy.ops.object.select_all(action="SELECT")
    bpy.context.view_layer.objects.active = tower
    seen.clear()
    core.run = spy
    r = bpy.ops.gothic3.export_mesh(name="FarTest_Tower", mode="package", folder=os.path.join(OUT, "far_pkg"), title="Far (test)", collision="none")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    low = seen.get("spec", {}).get("lowpoly") or {}
    check(r == {"FINISHED"} and rep.get("triangles") == 12 and low.get("corners") == 6 and rep.get("lods") == ["_lowpoly"]
          and any(f.endswith("FarTest_Tower_LOWPOLY.xcmsh") for f in rep.get("files", [])), f"far version: {rep.get('lods')} {rep.get('files')}")

    # 12. The modder's texture folders: a picture the game lacks (the helm's specular map, named in its material) comes
    #     in from there; a Blender image whose file moved is found there by name on export.
    tdir = os.path.join(OUT, "my textures", "helm")
    os.makedirs(tdir, exist_ok=True)
    pic = bpy.data.images.new("tdir_px", 4, 4)
    pic.pixels = [0.9, 0.9, 0.9, 1.0] * 16
    for fname in ("G3_Animation_Head_V01_Helmet_Specular_01_S1.png", "Moved_Wood.png"):
        pic.filepath_raw = os.path.join(tdir, fname)
        pic.file_format = "PNG"
        pic.save()
    bpy.data.images.remove(pic)
    pr.texture_dirs.clear()
    pr.texture_dirs.add().path = os.path.join(OUT, "my textures")
    clear()
    # The helm's material from step 4 would be shared again; set it up anew.
    for m in list(bpy.data.materials):
        bpy.data.materials.remove(m)
    bpy.ops.gothic3.import_mesh(name="G3_Armor_Helm_Myrtana_01", with_collision=False)
    helm = bpy.data.objects.get("G3_Armor_Helm_Myrtana_01")
    specs = [n.image.filepath for m in (helm.data.materials if helm else []) if m and m.node_tree for n in m.node_tree.nodes if n.type == "TEX_IMAGE" and n.get("g3_specular") and n.image]
    check(any(s.lower().endswith("g3_animation_head_v01_helmet_specular_01_s1.user.png") for s in specs), f"texture from the modder's folder: {specs}")
    gone = bpy.data.images.new("Moved_Wood.png", 4, 4)
    gone.source = "FILE"
    gone.filepath = os.path.join(OUT, "moved away", "Moved_Wood.png")
    gone.reload()
    got = export_mesh._image_png(gone, tempfile.mkdtemp(dir=OUT))
    check(got and os.path.normcase(got) == os.path.normcase(os.path.join(tdir, "Moved_Wood.png")), f"moved image found: {got}")
    pr.texture_dirs.clear()

except Exception:
    traceback.print_exc()
    fails.append("exception")
R("DONE", "fails:", fails)
