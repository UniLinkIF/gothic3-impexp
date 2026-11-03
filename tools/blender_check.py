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
    # 8a. Its trees: stand-ins in a collection of their own, marked for the view only, standing up (taller than wide),
    #     leaves cut at alpha 64; the model export leaves them out.
    tcol = next((c for c in bpy.data.collections if c.get("g3_view_only")), None)
    tobs = list(tcol.objects) if tcol else []
    tall = all(o.dimensions.z > 1.0 and o.get("g3_view_only") for o in tobs)
    leaf = next((m for m in bpy.data.materials if m.name.startswith("SPT_Leaves_")), None)
    check(len(tobs) >= 3 and tall and leaf is not None and leaf.get("g3_mask") == 64, f"trees: {len(tobs)} stand-ins, upright {tall}, leaf mask {leaf.get('g3_mask') if leaf else None}")
    bpy.ops.object.select_all(action="DESELECT")
    tobs[0].select_set(True)
    bpy.context.view_layer.objects.active = tobs[0]
    seen.clear()
    core.run = spy
    try:
        r = bpy.ops.gothic3.export_mesh(name="Tree_Test", mode="package", folder=os.path.join(OUT, "tree_pkg"), title="Tree (test)")
    except RuntimeError:
        r = {"CANCELLED"}
    core.run = orig
    check(r == {"CANCELLED"} and "out" not in seen, f"a tree stand-in is not exported: {r}")
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
    # 8b. A wolf's ambient loop goes back as the next free variant of that action (the game has 01-05).
    clear()
    bpy.ops.gothic3.import_character(name="G3_Wolf_Body_01", with_head=False)
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    bpy.context.view_layer.objects.active = arm
    arm.select_set(True)
    r1 = bpy.ops.gothic3.import_motion(clip="Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_02_%_00_P0_0")
    seen.clear()
    def spy_m(cmd, *args):
        out = orig(cmd, *args)
        if cmd == "export-motion":
            seen["out"] = out
            seen["spec"] = json.load(open(args[1], encoding="utf-8"))
        return out
    core.run = spy_m
    r = bpy.ops.gothic3.export_motion(clip="Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_02_%_00_P0_0", target="variant", mode="package", folder=os.path.join(OUT, "wolf_variant"), title="Wolf variant (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    check(r1 == {"FINISHED"} and r == {"FINISHED"} and rep.get("new") and rep.get("clip") == "Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_06_%_00_P0_0" and rep.get("template", "").endswith("_02_%_00_P0_0"),
          f"motion as a new variant: {rep.get('clip')} ({rep.get('note')})")
    # 8c. A new clip named from the game's words only: the lists hold the game's words; a dance for the wolf.
    from gothic3_impexp import export_motion
    items = [i[0] for i in export_motion._word_items("action")(None, None)]
    seen.clear()
    core.run = spy_m
    r = bpy.ops.gothic3.export_motion(clip="Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_02_%_00_P0_0", target="words", stance="Stand", left="None", right="None", pose="P0", action_word="Dance", phase="Loop", nio="N", direction="Fwd", variant=1, mode="package", folder=os.path.join(OUT, "wolf_dance"), title="Wolf dance (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    check(r == {"FINISHED"} and "Dance" in items and "Breakdance" not in items and len(items) > 100 and rep.get("clip") == "Wolf_Stand_None_None_P0_Dance_Loop_N_Fwd_01_%_00_P0_0", f"motion from the game's words: {rep.get('clip')} ({len(items)} actions)")

    # 9. Terrain: two neighbouring cells in as one welded mesh; back unchanged, nothing is written; a hill raised in
    #    the middle of one, only that cell comes back (mesh, collision, lightmap).
    from gothic3_impexp import terrain
    import numpy as np
    clear()
    r = bpy.ops.gothic3.import_terrain(cells="myrtana_landscape_cell_244 myrtana_landscape_cell_245")
    ob = next((o for o in bpy.data.objects if o.get("g3_terrain")), None)
    cells = json.loads(ob["g3_terrain"]) if ob else []
    me = ob.data if ob else None
    cell = np.empty(len(me.polygons), np.int32) if me else None
    if me:
        me.attributes["g3_cell"].data.foreach_get("value", cell)
    check(r == {"FINISHED"} and [c["name"].lower() for c in cells] == ["g3_myrtana_landscape_cell_244", "g3_myrtana_landscape_cell_245"] and set(np.unique(cell)) == {0, 1}
          and "g3_diffuse" in me.color_attributes and "g3_light" in me.color_attributes and len(me.materials) >= 8,
          f"terrain import: {len(me.polygons) if me else 0} faces, cells {cells}")

    def export_spec(o):
        t = tempfile.mkdtemp(dir=OUT)
        sp = os.path.join(t, "terrain.json")
        with open(sp, "w", encoding="utf-8") as f:
            json.dump({"cells": terrain.gather_cells(bpy.context, o, 0.01, t)}, f)
        return sp
    try:
        core.run("export-terrain", pr.game_dir, export_spec(ob), "package", os.path.join(OUT, "terrain_same"), "T")
        same = "exported"
    except core.CoreError as e:
        same = str(e)
    check("nothing to export" in same, f"unchanged terrain: {same}")
    # A hill of 3 m on cell 244's middle (faces of that cell only).
    co = np.empty(len(me.vertices) * 3, np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    fv = np.array([list(p.vertices) for p in me.polygons if cell[p.index] == 0]).ravel()
    mid = co[fv].mean(axis=0)
    d = np.hypot(co[:, 0] - mid[0], co[:, 1] - mid[1])
    co[:, 2] += np.where(d < 20.0, 3.0 * (1.0 - d / 20.0), 0.0)
    me.vertices.foreach_set("co", co.ravel())
    me.update()
    bpy.ops.object.select_all(action="DESELECT")
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    seen.clear()
    def spy_t(cmd, *args):
        out = orig(cmd, *args)
        if cmd == "export-terrain":
            seen["out"] = out
        return out
    core.run = spy_t
    pkg = os.path.join(OUT, "terrain_pkg")
    r = bpy.ops.gothic3.export_terrain(mode="package", folder=pkg, title="Hill (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    files = sorted(os.listdir(pkg)) if os.path.isdir(pkg) else []
    check(r == {"FINISHED"} and [c.lower() for c in rep.get("changed", [])] == ["g3_myrtana_landscape_cell_244"] and len(rep.get("unchanged", [])) == 1 and rep.get("lightmaps") == 1
          and {"_compiledMesh.vol", "Lightmaps.vol"} <= set(files) and any(f.lower().endswith("cell_244.xnvmsh") for f in rep.get("files", [])) and rep.get("distant"),
          f"terrain hill: {rep.get('changed')} / {rep.get('unchanged')}, distant {rep.get('distant')}, {files}")

    # 9b. A Varant cell keeps its four uv sets; painting its layer blend is a change.
    clear()
    r = bpy.ops.gothic3.import_terrain(cells="varant_landscape_cell_244")
    ob = next((o for o in bpy.data.objects if o.get("g3_terrain")), None)
    me = ob.data
    check(r == {"FINISHED"} and all(f"g3_uv{s}" in me.uv_layers for s in (15, 21, 18)), f"varant uv sets: {[u.name for u in me.uv_layers]}")
    try:
        core.run("export-terrain", pr.game_dir, export_spec(ob), "package", os.path.join(OUT, "terrain_same2"), "T")
        same = "exported"
    except core.CoreError as e:
        same = str(e)
    check("nothing to export" in same, f"unchanged varant cell: {same}")
    ca = me.color_attributes["g3_diffuse"]
    c = np.empty(len(ca.data) * 4, np.float32)
    ca.data.foreach_get("color_srgb", c)
    c[3:400:4] = 0.0
    ca.data.foreach_set("color_srgb", c)
    seen.clear()
    core.run = spy_t
    r = bpy.ops.gothic3.export_terrain(mode="package", folder=os.path.join(OUT, "terrain_paint"), title="Paint (test)")
    core.run = orig
    rep = seen.get("out", {}).get("report", {})
    check(r == {"FINISHED"} and [c.lower() for c in rep.get("changed", [])] == ["g3_varant_landscape_cell_244"], f"painted varant cell: {rep.get('changed')}")
    # 10. A voice line on an animated head: the head's face bones are on the body's skeleton and move its skin; the
    #     line comes as an action on those bones only, with its voice in the sequencer.
    from gothic3_impexp import lipsync
    clear()
    r = bpy.ops.gothic3.import_character(name="G3_Hero_Body_Player", with_head=True, head="G3_Head_Hero_Hero_Animated_01")
    arm = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
    head_ob = next((o for o in bpy.data.objects if o.type == "MESH" and o.get("g3_base", "").lower() == "g3_head_hero_hero_animated_01"), None)
    check(r == {"FINISHED"} and arm and "Hero_Head_Jaw_B" in arm.data.bones and arm.get("g3_head") == "G3_Head_Hero_Hero_Animated_01" and head_ob and "Hero_Head_Jaw_B" in head_ob.vertex_groups,
          f"animated head's face bones: {[b.name for b in arm.data.bones if 'Lip' in b.name or 'Jaw' in b.name][:4] if arm else None}")
    line = next(n for n in core.run("lip-lines", pr.game_dir, "English") if n.startswith("INFO_Ass_Elite_Hard_Bandaro"))
    bpy.context.view_layer.objects.active = arm
    arm.select_set(True)
    seen.clear()
    def spy_l(cmd, *args):
        out = orig(cmd, *args)
        if cmd in ("lipsync", "export-lipsync"):
            seen[cmd] = out
            if cmd == "export-lipsync":
                seen["spec"] = json.load(open(args[1], encoding="utf-8"))
        return out
    core.run = spy_l
    r = bpy.ops.gothic3.import_lipsync(line=line, language="English")
    core.run = orig
    act = arm.animation_data.action if arm.animation_data else None
    paths = {fc.data_path.split('"')[1] for fcs in lipsync._fcurves(act) for fc in fcs} if act else set()
    se = bpy.context.scene.sequence_editor
    strips = list(se.strips if hasattr(se, "strips") else se.sequences) if se else []
    check(r == {"FINISHED"} and act and act.get("g3_lipsync") == line and paths and all(("Lip" in p or "Jaw" in p or "Eye" in p or "UpDown" in p) for p in paths) and any(s.type == "SOUND" for s in strips),
          f"lip-sync: {len(paths)} bones moved ({sorted(paths)[:3]}), strips {[s.type for s in strips]}")
    # 10b. The line's action back as its .xlip: every key read as the nearest of the head's poses gives the line's own
    #      phones at their own times.
    core.run = spy_l
    r = bpy.ops.gothic3.export_lipsync(line=line, language="English", head="G3_Head_Hero_Hero_Animated_01", mode="package", folder=os.path.join(OUT, "lip_pkg"), title="Lip (test)")
    core.run = orig
    want = seen.get("lipsync", {}).get("timeline", [])
    got = seen.get("spec", {}).get("phones", [])
    same = len(want) == len(got) and all(a[1] == b[1] and abs(a[0] - b[0]) < 1e-4 for a, b in zip(want, got))
    wrong = [(a, b) for a, b in zip(want, got) if a[1] != b[1] or abs(a[0] - b[0]) >= 1e-4][:3]
    check(r == {"FINISHED"} and same and os.path.isfile(os.path.join(OUT, "lip_pkg", "Speech_English.vol")), f"lip-sync back: {len(got)} of {len(want)} phones, first wrong {wrong}")
except Exception:
    traceback.print_exc()
    fails.append("exception")
R("DONE", "fails:", fails)
