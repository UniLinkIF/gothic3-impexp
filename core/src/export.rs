//! Export of a static model from Blender: geometry in game space (the add-on mirrors Blender's axes once, so
//! triangles keep their order), materials as PNGs, optional collision — to Gothic 3 files, then a mod.
//!
//! * The mesh replaces the game's mesh of the same name (same archive path, and its `_lod1`… beside it), or is new
//!   (`_compiledMesh/gothic3_impexp/<name>.xcmsh`). The game is read without our own mod volumes, so a second
//!   export replaces the shipped mesh again, not our last version.
//! * A replaced mesh takes its vertex colours from the nearest vertex of the mesh it replaces (same material
//!   first): the landscape's layer blend and the baked brightness. Every lightmap made for that mesh (one per
//!   placed instance) is written again for the new vertices the same way.
//! * A material whose name the game has is used as it is; another one becomes `<name>.xshmat` from a template
//!   (opaque, opaque with specular, alpha test), its images `<name>_Diffuse_01` / `_Normal_01` / `_Specular_01`.
//! * Collision, one cooked mesh per surface (shape material): the model itself when the game's collision is the
//!   model's own triangles (the landscape: `<name>.xnvmsh` beside the mesh; an imported `*_COL` of it is only a
//!   view), when it is new without `*_COL` objects, or when asked (`mode: model`); else the `*_COL` objects; else
//!   the game's collision stays. A surface comes from the Blender
//!   material (`shape`), a `G3_Shape_<surface>` name, the game's own pairing of that material on the landscape,
//!   words in its name, or the dialog's default, in that order.

use crate::g3::G3Ctx;
use crate::g3_write::{self, Dxt};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct MatSpec { pub name: String, #[serde(default)] pub diffuse: Option<String>, #[serde(default)] pub normal: Option<String>, #[serde(default)] pub specular: Option<String>, #[serde(default)] pub alpha_test: Option<u8>, #[serde(default)] pub shape: Option<String> }

#[derive(Deserialize)]
pub struct CollisionSpec {
    pub geometry: Option<String>,
    pub corners: Option<usize>,
    /// The default surface.
    #[serde(default)] pub material: Option<String>,
    /// "auto", "model" (always from the model) or "none".
    #[serde(default)] pub mode: Option<String>,
    /// The collision objects' materials, indexed by the geometry's per-triangle material.
    #[serde(default)] pub materials: Vec<MatSpec>,
}

#[derive(Deserialize)]
pub struct Spec { pub name: String, pub geometry: String, pub corners: usize, pub materials: Vec<MatSpec>, pub collision: Option<CollisionSpec> }

#[derive(serde::Serialize, Default)]
pub struct Report { pub name: String, pub replaced: bool, pub vertices: usize, pub triangles: usize, pub materials: Vec<String>, pub collision: Option<String>, pub lightmaps: usize, pub lods: Vec<String>, pub files: Vec<String>, pub warnings: Vec<String> }

/// Corner arrays: positions f32×3, normals f32×3, uvs f32×2 (V down), material u32 per triangle.
fn read_geometry(path: &str, corners: usize) -> Result<(Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>)> {
    let d = std::fs::read(path).with_context(|| format!("read {path}"))?;
    if corners % 3 != 0 { bail!("{corners} corners is not whole triangles"); }
    let want = corners * 32 + corners / 3 * 4;
    if d.len() != want { bail!("geometry is {} bytes, expected {want}", d.len()); }
    let f = |i: usize| f32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
    let pos = (0..corners).map(|c| [f(c * 3), f(c * 3 + 1), f(c * 3 + 2)]).collect();
    let o = corners * 3;
    let nrm = (0..corners).map(|c| [f(o + c * 3), f(o + c * 3 + 1), f(o + c * 3 + 2)]).collect();
    let o = corners * 6;
    let uv = (0..corners).map(|c| [f(o + c * 2), f(o + c * 2 + 1)]).collect();
    let o = corners * 8;
    let mat = (0..corners / 3).map(|t| u32::from_le_bytes(d[(o + t) * 4..(o + t) * 4 + 4].try_into().unwrap())).collect();
    Ok((pos, nrm, uv, mat))
}

fn clean(s: &str) -> String {
    let base = match s.rsplit_once('.') { Some((a, b)) if b.len() == 3 && b.bytes().all(|c| c.is_ascii_digit()) => a, _ => s };
    base.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn hash(bytes: &[u8]) -> u64 { bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3)) }

/// The surface the game gives each landscape material (always the same one; read from the shipped cells).
const LANDSCAPE_SHAPES: &[(&str, &str)] = &[
    ("g3_architecture_bricks_03_b", "stone"), ("g3_architecture_groundplates_01_a", "stone"), ("g3_architecture_sandwall_01_a", "stone"),
    ("g3_desertdungeon_rock01_to_rock02_01_a", "stone"), ("g3_nature_ground_nordmar_ocean_01_a", "stone"), ("g3_nature_myrtana_rock_01_a", "stone"),
    ("g3_nature_myrtana_rock_02_a", "stone"), ("g3_nature_rock_transition_01_a", "stone"), ("g3_nature_trans_myrtana_stone_2_nordmar_stone_01_a", "stone"),
    ("g3_nature_trans_spainrock_2_desertrock_a", "stone"), ("g3_nature_trans_spainrock_2_myrtanarock_a", "stone"),
    ("g3_nature_trans_spainrock_2_varantdungeonwall_a", "stone"), ("g3_nature_trans_spainrock_a", "stone"), ("g3_nature_wall_stone_02_f", "stone"),
    ("g3_nature_wall_stone_02_g", "stone"), ("g3_nature_wall_stone_02_z", "stone"), ("g3_nordmar_mountain_01_a", "stone"),
    ("g3_nordmar_mountain_2_myrtana_mountain_01_a", "stone"), ("g3_varant_dungeon_ground_2_wall_01", "stone"), ("g3_varant_dungeon_wall_01", "stone"),
    ("g3_architecture_ground_city_04_a", "earth"), ("g3_architecture_ground_city_04_b", "earth"), ("g3_architecture_groundstone_03_a", "earth"),
    ("g3_architecture_sandground_01_a", "earth"), ("g3_desert_earthground_01_b", "earth"), ("g3_desert_earthlayer_02_a", "earth"),
    ("g3_desert_plainsand_to_gravel_01_a", "earth"), ("g3_desert_plainsand_to_path_01_a", "earth"), ("g3_desert_plainsand_to_drysand_01_a", "earth"),
    ("g3_myrtana_dungeon_stone_02_f_2_plainsand_02_a", "earth"), ("g3_nature_dunes_plainsand_02_d", "earth"), ("g3_nature_earthground_01_x", "earth"),
    ("g3_nature_earthground_01_y", "earth"), ("g3_nature_ground_earth_01_a", "earth"), ("g3_nature_ground_path_01_a", "earth"),
    ("g3_nature_ground_path_02_a", "earth"), ("g3_nature_ground_path_02_b", "earth"), ("g3_nature_ground_path_03_b", "earth"),
    ("g3_nature_trans_groundgrass_wetsand_a", "earth"), ("g3_nature_trans_spaingrass_2_wetsand_a", "earth"), ("g3_nature_trans_wetsand_2_myrtanashore_a", "earth"),
    ("g3_nordmar_cityground_01_a", "earth"), ("g3_nordmar_ground_path_01_a", "earth"),
    ("g3_nature_ground_forest_01_01_a", "clay"), ("g3_nature_ground_forest_01_01_b", "clay"), ("g3_nature_ground_forest_01_01_c", "clay"),
    ("g3_nature_ground_forest_01_01_d", "clay"), ("g3_nature_ground_grass_01_a", "clay"), ("g3_nature_ground_grass_02_a", "clay"),
    ("g3_nature_ground_grass_03_a", "clay"), ("g3_nature_trans_spaingrass_2_myrtanagrass_a", "clay"), ("g3_nature_trans_spaingrass_a", "clay"),
    ("g3_nature_ground_snow_01_b", "snow"),
    ("g3_nature_ground_gravel_01_a", "debris"), ("g3_nature_ground_gravel_02_a", "debris"), ("g3_nature_ground_riverstone_02_a", "debris"),
    ("g3_nature_ground_riverstone_03_a", "debris"), ("g3_nature_trans_spainrock_2_myrtanagrass_a", "debris"), ("g3_nature_trans_spainrock_2_spaingrass_a", "debris"),
    ("g3_nature_forest_snowground_01_a", "foliage"), ("g3_nature_trans_grass_snow_01_a", "grass"),
    ("g3_desert_ground_01_a", "sand"), ("g3_desert_plainsand_to_rock_01_a", "sand"), ("g3_desert_plainsand_to_wetsand_01_a", "sand"),
];

/// Words in a material name → surface; the first match wins.
const SHAPE_WORDS: &[(&str, &str)] = &[
    ("water", "water"), ("snow", "snow"), ("gravel", "debris"), ("riverstone", "debris"),
    ("sand", "sand"), ("desert", "sand"), ("dune", "sand"), ("grass", "clay"), ("forest", "clay"), ("moss", "clay"),
    ("wood", "wood"), ("plank", "wood"), ("board", "wood"), ("timber", "wood"), ("barrel", "wood"), ("bark", "wood"),
    ("metal", "metal"), ("iron", "metal"), ("steel", "metal"),
    ("leather", "leather"), ("pelt", "leather"), ("carpet", "leather"), ("cloth", "leather"), ("fabric", "leather"),
    ("glass", "glass"), ("straw", "foliage"), ("leaf", "foliage"), ("leaves", "foliage"), ("bush", "foliage"),
    ("path", "earth"), ("earth", "earth"), ("mud", "earth"), ("dirt", "earth"), ("ground", "earth"), ("soil", "earth"),
    ("rock", "stone"), ("stone", "stone"), ("wall", "stone"), ("brick", "stone"), ("mountain", "stone"), ("marble", "stone"),
    ("ice", "ice"),
];

/// The surface of a material: its own `shape`, a `G3_Shape_<surface>` name, the landscape pairing, words.
fn shape_of(m: &MatSpec, default: u8) -> u8 {
    let own = m.shape.as_deref().filter(|s| !s.is_empty() && *s != "auto").and_then(g3_write::shape_index);
    let base = clean(&m.name).to_lowercase();
    own.or_else(|| base.strip_prefix("g3_shape_").and_then(g3_write::shape_index))
        .or_else(|| LANDSCAPE_SHAPES.iter().find(|(n, _)| *n == base).and_then(|(_, s)| g3_write::shape_index(s)))
        .or_else(|| SHAPE_WORDS.iter().find(|(w, _)| base.contains(w)).and_then(|(_, s)| g3_write::shape_index(s)))
        .unwrap_or(default)
}

/// Nearest point among some of `pts`, on a uniform grid.
struct Nearest<'a> { pts: &'a [[f32; 3]], empty: bool, cell: f32, grid: HashMap<[i32; 3], Vec<u32>> }

impl<'a> Nearest<'a> {
    fn new(pts: &'a [[f32; 3]], ids: impl Iterator<Item = u32> + Clone) -> Self {
        let (mut lo, mut hi, mut n) = ([f32::MAX; 3], [f32::MIN; 3], 0usize);
        for i in ids.clone() { let p = pts[i as usize]; n += 1; for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        let vol: f32 = (0..3).map(|k| (hi[k] - lo[k]).max(1.0)).product();
        let cell = ((vol / n.max(1) as f32).cbrt() * 2.0).max(1.0);
        let mut grid: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for i in ids { grid.entry(Self::key(pts[i as usize], cell)).or_default().push(i); }
        Nearest { pts, empty: n == 0, cell, grid }
    }
    fn key(p: [f32; 3], cell: f32) -> [i32; 3] { p.map(|x| (x / cell).floor() as i32) }
    fn find(&self, p: [f32; 3]) -> Option<u32> {
        if self.empty { return None; }
        let c = Self::key(p, self.cell);
        let mut best: Option<(f32, u32)> = None;
        for r in 0i32..4096 {
            // Ring r holds every point closer than r cells; once the best is nearer than that, it is final.
            if let Some((b, _)) = best { if (r - 1) as f32 * self.cell > b.sqrt() { break; } }
            for x in -r..=r { for y in -r..=r { for z in -r..=r {
                if x.abs().max(y.abs()).max(z.abs()) != r { continue; }
                let Some(v) = self.grid.get(&[c[0] + x, c[1] + y, c[2] + z]) else { continue };
                for &i in v {
                    let q = self.pts[i as usize];
                    let d = (0..3).map(|k| (q[k] - p[k]).powi(2)).sum::<f32>();
                    if best.map_or(true, |(b, _)| d < b) { best = Some((d, i)); }
                }
            } } }
        }
        best.map(|b| b.1)
    }
}

/// For every vertex of `parts`, the nearest vertex of `orig`: of the same material when it has one.
fn nearest_original(orig: &crate::g3_res::G3Mesh, parts: &[crate::xcmsh_write::Part]) -> Vec<Vec<u32>> {
    let geo = &orig.geo;
    let mut by_mat: HashMap<String, Vec<u32>> = HashMap::new();
    for (e, s) in geo.submeshes.iter().enumerate() {
        let (first, n) = orig.elements[e];
        by_mat.entry(clean(s.material.trim_end_matches(".xshmat")).to_lowercase()).or_default().extend(first..first + n);
    }
    let all = Nearest::new(&geo.positions, 0..geo.positions.len() as u32);
    let mats: HashMap<String, Nearest> = by_mat.iter().map(|(k, v)| (k.clone(), Nearest::new(&geo.positions, v.iter().copied()))).collect();
    parts.iter().map(|p| {
        let near = mats.get(&clean(&p.material).to_lowercase()).unwrap_or(&all);
        p.positions.iter().map(|&q| near.find(q).or_else(|| all.find(q)).unwrap_or(0)).collect()
    }).collect()
}

fn archive_of(key: &str) -> &'static str {
    match key.split('/').next().unwrap_or("") { "_compiledphysic" => "_compiledPhysic", "lightmaps" => "Lightmaps", _ => "_compiledMesh" }
}

/// Lightmaps of `orig` (at archive path `tpath`) written again for `near` (new vertex → original vertex).
fn lightmaps(g: &G3Ctx, orig: &crate::g3_res::G3Mesh, tpath: &str, near: &[Vec<u32>], r: &mut Report, files: &mut Vec<(String, String, Vec<u8>)>) -> Result<()> {
    let stem = tpath.rsplit('/').next().unwrap().trim_end_matches(".xcmsh").to_lowercase();
    let want = format!("/data/_compiledmesh/{}", tpath.to_lowercase());
    let pre = format!("lightmaps/{stem}_{{");
    for lk in g.keys().into_iter().filter(|k| k.starts_with(&pre)) {
        let d = g.read(&lk)?;
        if crate::lightmap::mesh_path(&d).as_deref() != Some(want.as_str()) { continue; }
        let lm = crate::lightmap::parse(&d).with_context(|| lk.clone())?;
        // Original vertex → (colour, direction), where the lightmap's element matches the mesh's.
        let mut light: Vec<Option<(u32, [f32; 3])>> = vec![None; orig.geo.positions.len()];
        for (e, (cols, dirs)) in lm.elements.iter().enumerate() {
            let Some(&(first, n)) = orig.elements.get(e) else { break };
            if cols.len() != n as usize { continue; }
            for i in 0..n as usize { light[first as usize + i] = Some((cols[i], dirs[i])); }
        }
        let any = light.iter().flatten().next().copied();
        let elements: Vec<(Vec<u32>, Vec<[f32; 3]>)> = near.iter().map(|n| {
            let v: Vec<(u32, [f32; 3])> = n.iter().map(|&i| light[i as usize].or(any).unwrap_or((0xff80_8080, [0.0, 1.0, 0.0]))).collect();
            (v.iter().map(|x| x.0).collect(), v.iter().map(|x| x.1).collect())
        }).collect();
        let path = g.path_of(&lk).unwrap_or(&lk).to_string();
        if lm.elements.len() < lm.count { r.warnings.push(format!("{path}: lit by lightmap pages in the game; written as vertex lighting from what could be read")); }
        files.push(("Lightmaps".to_string(), path, crate::lightmap::write(&lm, &elements)));
        r.lightmaps += 1;
    }
    Ok(())
}

/// The files of the model: (archive, path, bytes). `g` is the game without our own mod volumes.
pub fn build(g: &G3Ctx, spec: &Spec) -> Result<(Report, Vec<(String, String, Vec<u8>)>)> {
    let name = clean(&spec.name);
    if name.is_empty() { bail!("model name is empty"); }
    let mut r = Report { name: name.clone(), ..Default::default() };
    let mut files = vec![];
    let now = crate::volume::filetime_now();
    let (pos, nrm, uv, tri_mat) = read_geometry(&spec.geometry, spec.corners)?;

    // Materials: the game's own by name, or new ones from a template.
    let mut mat_names = vec![];
    for m in &spec.materials {
        let base = clean(&m.name);
        if g.find("_compiledmaterial", &format!("{base}.xshmat")).is_some() {
            r.materials.push(format!("{base}: game material"));
            mat_names.push(base);
            continue;
        }
        let t = if m.alpha_test.is_some() { &g3_write::MASKED } else if m.specular.is_some() { &g3_write::OPAQUE_SPECULAR } else { &g3_write::OPAQUE };
        // Name: the material's own plus a short hash of its pictures, so two mods' "Material" never collide.
        let mut h = hash(base.as_bytes());
        for p in [&m.diffuse, &m.normal, &m.specular].into_iter().flatten() { h ^= hash(&std::fs::read(p).unwrap_or_default()).rotate_left(7); }
        let mname = format!("G3IE_{base}_{:06x}", h & 0xffffff);
        let mut tex = vec![];
        let mut image = |png: &Option<String>, role: &'static str, suffix: &str, normal: bool, alpha: bool| -> Result<()> {
            let Some(p) = png else { return Ok(()) };
            let img = image::open(p).with_context(|| format!("material {}: {p}", m.name))?.to_rgba8();
            let (w, h) = img.dimensions();
            let px = if normal { g3_write::normal_to_game(img.as_raw()) } else { img.into_raw() };
            let tname = format!("{mname}{suffix}");
            files.push(("_compiledImage".to_string(), format!("gothic3_impexp/{tname}.ximg"), g3_write::ximg(&px, w, h, if alpha { Dxt::Dxt5 } else { Dxt::Dxt1 }, now)?));
            tex.push((role, tname));
            Ok(())
        };
        image(&m.diffuse, "diffuse", "_Diffuse_01", false, m.alpha_test.is_some())?;
        image(&m.normal, "normal", "_Normal_01", true, false)?;
        if t.roles.contains(&"specular") { image(&m.specular, "specular", "_Specular_01", false, false)?; }
        if m.diffuse.is_none() { r.warnings.push(format!("material {}: no Base Color image — the template's texture stays", m.name)); }
        let tk = g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).with_context(|| format!("template {} not in the game", t.material))?;
        let tex_ref: Vec<(&str, String)> = tex.iter().map(|(a, b)| (*a, b.clone())).collect();
        files.push(("_compiledMaterial".to_string(), format!("gothic3_impexp/{mname}.xshmat"), g3_write::material(&g.read(&tk)?, t, &tex_ref, m.alpha_test)?));
        r.materials.push(format!("{}: new {mname}{}", m.name, if m.alpha_test.is_some() { " (alpha test)" } else if t.roles.contains(&"specular") { " (specular)" } else { "" }));
        mat_names.push(mname);
    }
    if mat_names.is_empty() { mat_names.push("G3_Objects_Wood_01_B".into()); }

    // Parts per material: one vertex per (position, normal, uv). Parts without triangles are left out, so the
    // lightmaps count the same elements as the mesh.
    let mut parts: Vec<crate::xcmsh_write::Part> = mat_names.iter().map(|m| crate::xcmsh_write::Part { material: m.clone(), positions: vec![], normals: vec![], uvs: vec![], triangles: vec![], diffuse: vec![], specular: vec![] }).collect();
    let mut seen: Vec<HashMap<[u32; 8], u32>> = vec![HashMap::new(); parts.len()];
    for t in 0..spec.corners / 3 {
        let mi = (tri_mat[t] as usize).min(parts.len() - 1);
        let p = &mut parts[mi];
        let tri = [0, 1, 2].map(|k| {
            let c = t * 3 + k;
            let key = [pos[c][0].to_bits(), pos[c][1].to_bits(), pos[c][2].to_bits(), nrm[c][0].to_bits(), nrm[c][1].to_bits(), nrm[c][2].to_bits(), uv[c][0].to_bits(), uv[c][1].to_bits()];
            *seen[mi].entry(key).or_insert_with(|| { p.positions.push(pos[c]); p.normals.push(nrm[c]); p.uvs.push(uv[c]); p.positions.len() as u32 - 1 })
        });
        p.triangles.push(tri);
    }
    parts.retain(|p| !p.triangles.is_empty());
    r.vertices = parts.iter().map(|p| p.positions.len()).sum();
    r.triangles = spec.corners / 3;
    let existing = crate::staticmesh::mesh_key(g, &name);
    r.replaced = existing.is_some();
    let path = match &existing { Some(k) => g.path_of(k).unwrap_or(k).to_string(), None => format!("gothic3_impexp/{name}.xcmsh") };
    let file_stem = path.rsplit('/').next().unwrap().trim_end_matches(".xcmsh").to_string();

    // The mesh and, when it replaces one, the levels of detail beside it — the same model, colours and lightmaps
    // taken over from each mesh it replaces.
    let mut targets = vec![(existing.clone(), path.clone())];
    if let Some(k) = &existing {
        for i in 1..=3 {
            let lk = k.replace(".xcmsh", &format!("_lod{i}.xcmsh"));
            if g.size_of(&lk).is_some() { targets.push((Some(lk.clone()), g.path_of(&lk).unwrap_or(&lk).to_string())); r.lods.push(format!("_lod{i}")); }
        }
    }
    for (key, tpath) in &targets {
        let mut tparts: Vec<crate::xcmsh_write::Part> = parts.iter().map(|p| crate::xcmsh_write::Part { material: p.material.clone(), positions: p.positions.clone(), normals: p.normals.clone(), uvs: p.uvs.clone(), triangles: p.triangles.clone(), diffuse: vec![], specular: vec![] }).collect();
        let orig = match key { Some(k) => Some(crate::g3_res::decode_xcmsh(&g.read(k)?).with_context(|| format!("{k}: the mesh it replaces"))?), None => None };
        if let Some(o) = &orig {
            let near = nearest_original(o, &tparts);
            for (p, n) in tparts.iter_mut().zip(&near) {
                p.diffuse = n.iter().map(|&i| o.diffuse[i as usize]).collect();
                p.specular = n.iter().map(|&i| o.specular[i as usize]).collect();
            }
            lightmaps(g, o, tpath, &near, &mut r, &mut files)?;
        }
        files.push(("_compiledMesh".to_string(), tpath.clone(), crate::xcmsh_write::write(&tparts)));
    }

    // Collision: one cooked mesh per surface.
    if let Some(c) = &spec.collision {
        let mode = c.mode.as_deref().unwrap_or("auto");
        if mode != "none" {
            let default = c.material.as_deref().and_then(g3_write::shape_index).unwrap_or(4);
            let game_col = crate::staticmesh::collision_key(g, &name);
            let beside = game_col.as_deref().is_some_and(|k| k.starts_with("_compiledmesh/"));
            // (corner positions, surface per triangle, where they came from)
            let src: Option<(Vec<[f32; 3]>, Vec<u8>, &str)> = match (&c.geometry, c.corners) {
                (Some(geo), Some(n)) if mode != "model" && !beside => {
                    let (cp, _, _, cm) = read_geometry(geo, n)?;
                    let shapes: Vec<u8> = c.materials.iter().map(|m| shape_of(m, default)).collect();
                    let tri = cm.iter().map(|&i| shapes.get(i as usize).copied().unwrap_or(default)).collect();
                    Some((cp, tri, "the *_COL objects"))
                }
                _ if existing.is_none() || beside || mode == "model" => {
                    let shapes: Vec<u8> = spec.materials.iter().map(|m| shape_of(m, default)).collect();
                    let tri = tri_mat.iter().map(|&i| shapes.get(i as usize).copied().unwrap_or(default)).collect();
                    Some((pos.clone(), tri, "the model itself"))
                }
                _ => None,
            };
            match src {
                Some((cp, tri_shape, from)) => {
                    let mut groups: Vec<(u8, Vec<[f32; 3]>, Vec<[u32; 3]>, HashMap<[u32; 3], u32>)> = vec![];
                    for (t, c3) in cp.chunks_exact(3).enumerate() {
                        let s = tri_shape[t];
                        let gi = match groups.iter().position(|x| x.0 == s) { Some(i) => i, None => { groups.push((s, vec![], vec![], HashMap::new())); groups.len() - 1 } };
                        let gr = &mut groups[gi];
                        let ids = [0, 1, 2].map(|k| *gr.3.entry(c3[k].map(f32::to_bits)).or_insert_with(|| { gr.1.push(c3[k]); gr.1.len() as u32 - 1 }));
                        gr.2.push(ids);
                    }
                    let meshes: Vec<(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)> = groups.into_iter().map(|(s, v, t, _)| (v, t, s)).collect();
                    let (col_archive, col_path) = match &game_col {
                        Some(k) => (archive_of(k).to_string(), g.path_of(k).unwrap_or(k).to_string()),
                        None => ("_compiledPhysic".to_string(), format!("gothic3_impexp/{file_stem}_COL.xnvmsh")),
                    };
                    let summary: Vec<String> = meshes.iter().map(|(_, t, s)| format!("{} {}", g3_write::SHAPE_MATERIALS[*s as usize], t.len())).collect();
                    files.push((col_archive, col_path, g3_write::xnvmsh(&meshes)?));
                    r.collision = Some(format!("{} triangles from {from}: {}", cp.len() / 3, summary.join(", ")));
                }
                None => r.collision = Some("the game's own collision stays".into()),
            }
        }
    }
    r.files = files.iter().map(|(a, p, _)| format!("{a}/{p}")).collect();
    Ok((r, files))
}

pub fn run(g: &G3Ctx, game: &Path, spec_path: &str, mode: &str, arg: &str, title: &str) -> Result<serde_json::Value> {
    let spec: Spec = serde_json::from_str(&std::fs::read_to_string(spec_path)?).context("export spec")?;
    let (report, files) = build(g, &spec)?;
    let mut out = serde_json::json!({ "report": report });
    match mode {
        "install" => { out["volumes"] = serde_json::json!(crate::mods::install(game, arg, &files)?); }
        "package" => { crate::mods::package(Path::new(arg), title, &files)?; out["package"] = serde_json::json!(arg); }
        m => bail!("mode {m}: install or package"),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec holding the game mesh `key` itself, as the add-on would send it.
    fn spec_of(g: &G3Ctx, key: &str, dir: &Path, collision: &str) -> Spec {
        let m = crate::g3_res::decode_xcmsh(&g.read(key).unwrap()).unwrap();
        let geo = &m.geo;
        let (mut p, mut n, mut u, mut t) = (vec![], vec![], vec![], vec![]);
        for (e, s) in geo.submeshes.iter().enumerate() {
            for tri in geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize].chunks_exact(3) {
                for &i in tri { p.extend(geo.positions[i as usize]); n.extend(geo.normals[i as usize]); u.extend(geo.uvs.get(i as usize).copied().unwrap_or([0.0; 2])); }
                t.push(e as u32);
            }
        }
        let mut bytes = vec![];
        for x in p.iter().chain(&n).chain(&u) { bytes.extend(x.to_le_bytes()); }
        for x in &t { bytes.extend(x.to_le_bytes()); }
        std::fs::create_dir_all(dir).unwrap();
        let geof = dir.join("geometry.bin");
        std::fs::write(&geof, bytes).unwrap();
        let mats = || geo.submeshes.iter().map(|s| MatSpec { name: s.material.trim_end_matches(".xshmat").to_string(), diffuse: None, normal: None, specular: None, alpha_test: None, shape: None }).collect();
        Spec { name: crate::stem_of(key), geometry: geof.to_string_lossy().into_owned(), corners: t.len() * 3, materials: mats(),
            collision: Some(CollisionSpec { geometry: None, corners: None, material: Some("stone".into()), mode: Some(collision.into()), materials: vec![] }) }
    }

    /// The landscape cell written back from its own triangles: same colours, its one lightmap rebuilt to the
    /// same numbers, and collision from the model with the game's own surfaces.
    #[test]
    fn a_landscape_cell_replaced_by_itself_keeps_its_colours_light_and_surfaces() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_myrtana_landscape_01/lod/g3_myrtana_landscape_cell_244.xcmsh";
        let dir = std::env::temp_dir().join("g3ie_test_cell");
        let (r, files) = build(&g, &spec_of(&g, key, &dir, "auto")).unwrap();
        assert!(r.replaced);
        assert_eq!(r.lightmaps, 1, "{:?}", r.files);
        let mesh = files.iter().find(|f| f.1.ends_with(".xcmsh")).unwrap();
        let ours = crate::g3_res::decode_xcmsh(&mesh.2).unwrap();
        let game = crate::g3_res::decode_xcmsh(&g.read(key).unwrap()).unwrap();
        // Colours per triangle corner, by position.
        let corner = |m: &crate::g3_res::G3Mesh| -> HashMap<[u32; 3], (u32, u32)> { m.geo.indices.iter().map(|&i| (m.geo.positions[i as usize].map(f32::to_bits), (m.diffuse[i as usize], m.specular[i as usize]))).collect() };
        let (a, b) = (corner(&game), corner(&ours));
        let same = a.iter().filter(|(k, v)| b.get(*k) == Some(v)).count();
        assert!(same * 100 >= a.len() * 99, "{same} of {} corners keep their colours", a.len());
        // Lightmap: every vertex's light equals the light of the game vertex at the same place.
        let lm_game = g.keys().into_iter().find(|k| k.contains("g3_myrtana_landscape_cell_244_{")).unwrap();
        let lg = crate::lightmap::parse(&g.read(&lm_game).unwrap()).unwrap();
        let lo = crate::lightmap::parse(&files.iter().find(|f| f.0 == "Lightmaps").unwrap().2).unwrap();
        assert_eq!(lo.elements.len(), ours.elements.len());
        let light = |m: &crate::g3_res::G3Mesh, l: &crate::lightmap::Lightmap| -> HashMap<[u32; 3], u32> { m.elements.iter().zip(&l.elements).flat_map(|(&(f, n), (c, _))| (0..n as usize).map(move |i| (f as usize + i, c[i]))).map(|(v, c)| (m.geo.positions[v].map(f32::to_bits), c)).collect() };
        let (la, lb) = (light(&game, &lg), light(&ours, &lo));
        let same = la.iter().filter(|(k, v)| lb.get(*k) == Some(v)).count();
        assert!(same * 100 >= la.len() * 99, "{same} of {} lit vertices", la.len());
        // Collision from the model, the game's surfaces per material, written where the game keeps it.
        let col = files.iter().find(|f| f.1.to_lowercase().ends_with("cell_244.xnvmsh")).expect("collision beside the mesh");
        assert_eq!(col.0, "_compiledMesh");
        let x = crate::g3_res::parse_xnvmsh(&col.2).unwrap();
        let mut ours_s: Vec<u8> = x.shapes.iter().map(|s| s[0]).collect(); ours_s.sort(); ours_s.dedup();
        let gx = crate::g3_res::parse_xnvmsh(&g.read(&key.replace(".xcmsh", ".xnvmsh")).unwrap()).unwrap();
        let mut game_s: Vec<u8> = gx.shapes.iter().map(|s| s[0]).collect(); game_s.sort(); game_s.dedup();
        assert_eq!(ours_s, game_s, "{:?}", r.collision);
        assert_eq!(crate::g3_res::xnvmsh_triangles(&col.2).unwrap().len(), 14226);
    }

    /// A building written back: its LOD beside it too, every instance's lightmap, the game's collision kept.
    #[test]
    fn a_building_replaced_takes_its_lod_and_lightmaps_along() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_objects_myrtana_buildings_01/g3_myrtana_house_mill_water_01.xcmsh";
        let (r, files) = build(&g, &spec_of(&g, key, &std::env::temp_dir().join("g3ie_test_mill"), "auto")).unwrap();
        assert_eq!(r.lods, vec!["_lod1"]);
        let meshes: Vec<&String> = files.iter().filter(|f| f.1.ends_with(".xcmsh")).map(|f| &f.1).collect();
        assert_eq!(meshes.len(), 2, "{meshes:?}");
        let instances = g.keys().into_iter().filter(|k| k.starts_with("lightmaps/g3_myrtana_house_mill_water_01_")).count();
        assert!(r.lightmaps > 0 && r.lightmaps <= instances, "{} of {instances}", r.lightmaps);
        assert_eq!(r.collision.as_deref(), Some("the game's own collision stays"));
        for f in files.iter().filter(|f| f.0 == "Lightmaps") {
            let lm = crate::lightmap::parse(&f.2).unwrap();
            assert_eq!(lm.elements.len(), lm.count, "{}", f.1);
        }
    }
}
