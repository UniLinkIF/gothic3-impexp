"""A run through the add-on in Blender: names, textures, frame effects, an item with its convex body, a model whose
collision has no _COL, an actor. Writes only into the folder given after `--` (mod packages, never into the game).

    blender -b --factory-startup --python tools/blender_check.py -- <folder>

The add-on must be enabled-able in that Blender (installed, or linked into its scripts/addons); the game folder is
G3_GAME or the Steam default. Ends with "RESULT DONE fails: []" when all is well."""
import bpy, os, sys, json, tempfile, traceback
S = sys.argv[sys.argv.index("--") + 1]
import addon_utils
addon_utils.enable("gothic3_impexp", default_set=True)
from gothic3_impexp import prefs as P, export_actor, export_mesh, effects, core
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
        if cmd == "export":
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

    # 7. Cyrillic name reaches the core as it is.
    clear()
    bpy.ops.mesh.primitive_cube_add()
    c = bpy.context.active_object
    c.name = "Млин"
    check(export_mesh._default_name(bpy.context) == "Млин", f"default name {export_mesh._default_name(bpy.context)!r}")
except Exception:
    traceback.print_exc()
    fails.append("exception")
R("DONE", "fails:", fails)
