//! Export of a static model from Blender: geometry in game space (the add-on mirrors Blender's axes once, so
//! triangles keep their order), materials as PNGs, optional collision — to Gothic 3 files, then a mod.
//!
//! * The mesh replaces the game's mesh of the same name (same archive path, and its `_lod1`… beside it), or is new
//!   (`_compiledMesh/gothic3_impexp/<name>.xcmsh`). The game is read without any mod's volumes, so a second
//!   export replaces the shipped mesh again, not our last version.
//! * A replaced mesh takes its vertex colours from the nearest vertex of the mesh it replaces (same material
//!   first): the landscape's layer blend and the baked brightness. Every lightmap made for that mesh (one per
//!   placed instance) is written again for the new vertices the same way.
//! * A material whose name the game has is used as it is; another one becomes `<name>.xshmat` from a template
//!   (opaque, opaque with specular, alpha test), its images `<name>_Diffuse_01` / `_Normal_01` / `_Specular_01`.
//! * Collision, one cooked mesh (part) per surface — or, replacing the game's, the game's parts in the game's order,
//!   since the world's entities name each part by its index with a surface of their own: the model itself when the game's collision is the
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

#[derive(Deserialize, Default)]
pub struct MatSpec {
    pub name: String,
    #[serde(default)] pub diffuse: Option<String>,
    #[serde(default)] pub normal: Option<String>,
    #[serde(default)] pub specular: Option<String>,
    #[serde(default)] pub alpha_test: Option<u8>,
    #[serde(default)] pub shape: Option<String>,
    /// See-through other than a cut-off: "blend" (by the diffuse's alpha: glass, water, haze) or "glow" (additive).
    #[serde(default)] pub blend: Option<String>,
}

impl MatSpec {
    pub fn look(&self) -> g3_write::Look {
        match self.blend.as_deref() { Some("glow") => g3_write::Look::Glow, Some("blend") => g3_write::Look::Blend, _ if self.alpha_test.is_some() => g3_write::Look::AlphaTest, _ => g3_write::Look::Opaque }
    }
}

/// The maps of a new material: image name per role of its template.
pub struct Made { pub template: &'static g3_write::Template, pub diffuse: Option<String>, pub normal: Option<String>, pub specular: Option<String> }

/// A new material `mname` made from the template of `m`'s look (static or, for an actor, skinned): its pictures as
/// `<mname>_Diffuse_01` / `_Normal_01` / `_Specular_01`, the `.xshmat`, and warnings for what does not fit. A normal or
/// specular map the template samples and the Blender material lacks becomes a flat / dull picture, so none of the
/// template's own shows through; a missing diffuse keeps the template's.
pub(crate) fn make_material(g: &G3Ctx, m: &MatSpec, mname: &str, actor: bool, now: u64, files: &mut Vec<(String, String, Vec<u8>)>, warnings: &mut Vec<String>) -> Result<Made> {
    let (t, dropped) = g3_write::template_for(actor, m.look(), m.normal.is_some(), m.specular.is_some());
    for r in dropped { warnings.push(format!("material {}: the game's {} materials{} have no {r} map; it is left out", m.name, g3_write::look_name(t), if actor { " for actors" } else { "" })); }
    let alpha = matches!(m.look(), g3_write::Look::AlphaTest | g3_write::Look::Blend);
    let mut made = Made { template: t, diffuse: None, normal: None, specular: None };
    for (role, png, suffix) in [("diffuse", &m.diffuse, "_Diffuse_01"), ("normal", &m.normal, "_Normal_01"), ("specular", &m.specular, "_Specular_01")] {
        if !t.roles.contains(&role) { continue; }
        let (px, w, h) = match png {
            Some(p) => {
                let img = image::open(p).with_context(|| format!("material {}: {p}", m.name))?.to_rgba8();
                let (w, h) = img.dimensions();
                (if role == "normal" { g3_write::normal_to_game(img.as_raw()) } else { img.into_raw() }, w, h)
            }
            None => match g3_write::neutral(role) { Some(c) => (c.repeat(16), 4, 4), None => continue },
        };
        let tname = format!("{mname}{suffix}");
        files.push(("_compiledImage".to_string(), format!("gothic3_impexp/{tname}.ximg"), g3_write::ximg(&px, w, h, if alpha && role == "diffuse" { Dxt::Dxt5 } else { Dxt::Dxt1 }, now)?));
        *match role { "diffuse" => &mut made.diffuse, "normal" => &mut made.normal, _ => &mut made.specular } = Some(tname);
    }
    if m.diffuse.is_none() { warnings.push(format!("material {}: no Base Color image — the template's texture stays", m.name)); }
    let tk = g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).with_context(|| format!("template {} not in the game", t.material))?;
    let tex: Vec<(&str, String)> = [("diffuse", &made.diffuse), ("normal", &made.normal), ("specular", &made.specular)].into_iter().filter_map(|(r, x)| x.clone().map(|x| (r, x))).collect();
    files.push(("_compiledMaterial".to_string(), format!("gothic3_impexp/{mname}.xshmat"), g3_write::material(&g.read(&tk)?, t, &tex, m.alpha_test)?));
    Ok(made)
}

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
    /// Convex hulls (`*_CV` objects), one per part: geometry files like the collision's.
    #[serde(default)] pub convex: Vec<ConvexSpec>,
}

#[derive(Deserialize)]
pub struct ConvexSpec { pub geometry: String, pub corners: usize }

#[derive(Deserialize)]
pub struct Spec { pub name: String, pub geometry: String, pub corners: usize, pub materials: Vec<MatSpec>, pub collision: Option<CollisionSpec> }

#[derive(serde::Serialize, Default)]
pub struct Report { pub name: String, pub replaced: bool, pub vertices: usize, pub triangles: usize, pub materials: Vec<String>, pub collision: Option<String>, pub convex: Option<String>, pub lightmaps: usize, pub lods: Vec<String>, pub files: Vec<String>, pub warnings: Vec<String> }

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

/// A name the game's files can carry: Latin letters, digits and `_`, without Blender's `.001`. Cyrillic is
/// transliterated; anything else that is lost gets a short hash of the whole name, so two names never become one.
pub(crate) fn clean(s: &str) -> String {
    let base = match s.rsplit_once('.') { Some((a, b)) if b.len() == 3 && b.bytes().all(|c| c.is_ascii_digit()) => a, _ => s };
    let mut out = String::new();
    let mut lost = false;
    for c in base.chars() {
        if c.is_ascii_alphanumeric() || c == '_' { out.push(c); continue; }
        let lower: String = c.to_lowercase().collect();
        match translit(&lower) {
            Some(t) if c.is_uppercase() => { let mut cs = t.chars(); if let Some(f) = cs.next() { out.push(f.to_ascii_uppercase()); out.extend(cs); } }
            Some(t) => out.push_str(t),
            None => { lost |= !c.is_ascii(); out.push('_'); }
        }
    }
    if lost { out += &format!("_{:06x}", hash(base.as_bytes()) & 0xffffff); }
    out
}

/// Ukrainian (and Russian) letters in Latin, as Ukraine's official transliteration writes them.
fn translit(c: &str) -> Option<&'static str> {
    Some(match c {
        "а" => "a", "б" => "b", "в" => "v", "г" => "h", "ґ" => "g", "д" => "d", "е" => "e", "є" => "ie", "ж" => "zh", "з" => "z",
        "и" => "y", "і" => "i", "ї" => "i", "й" => "i", "к" => "k", "л" => "l", "м" => "m", "н" => "n", "о" => "o", "п" => "p",
        "р" => "r", "с" => "s", "т" => "t", "у" => "u", "ф" => "f", "х" => "kh", "ц" => "ts", "ч" => "ch", "ш" => "sh",
        "щ" => "shch", "ь" | "ъ" | "'" | "’" => "", "ю" => "iu", "я" => "ia", "ё" => "io", "ы" => "y", "э" => "e",
        _ => return None,
    })
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
struct Nearest<'a> { pts: &'a [[f32; 3]], ids: Vec<u32>, cell: f32, grid: HashMap<[i32; 3], Vec<u32>>, lo: [i32; 3], hi: [i32; 3] }

impl<'a> Nearest<'a> {
    fn new(pts: &'a [[f32; 3]], ids: impl Iterator<Item = u32>) -> Self {
        let ids: Vec<u32> = ids.collect();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &i in &ids { let p = pts[i as usize]; for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        let vol: f32 = (0..3).map(|k| (hi[k] - lo[k]).max(1.0)).product();
        let cell = ((vol / ids.len().max(1) as f32).cbrt() * 2.0).max(1.0);
        let mut grid: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for &i in &ids { grid.entry(Self::key(pts[i as usize], cell)).or_default().push(i); }
        let (klo, khi) = if ids.is_empty() { ([0; 3], [-1; 3]) } else { (Self::key(lo, cell), Self::key(hi, cell)) };
        Nearest { pts, ids, cell, grid, lo: klo, hi: khi }
    }
    fn key(p: [f32; 3], cell: f32) -> [i32; 3] { p.map(|x| (x / cell).floor() as i32) }
    fn dist(&self, i: u32, p: [f32; 3]) -> f32 { let q = self.pts[i as usize]; (0..3).map(|k| (q[k] - p[k]).powi(2)).sum() }
    fn find(&self, p: [f32; 3]) -> Option<u32> {
        if self.ids.is_empty() { return None; }
        let c = Self::key(p, self.cell);
        // Rings beyond this one hold no cell of the grid.
        let last = (0..3).map(|k| (c[k] - self.lo[k]).abs().max((c[k] - self.hi[k]).abs())).max().unwrap_or(0);
        let mut best: Option<(f32, u32)> = None;
        for r in 0..=last {
            // Ring r holds every point closer than r cells; once the best is nearer than that, it is final.
            if let Some((b, _)) = best { if (r - 1) as f32 * self.cell > b.sqrt() { break; } }
            // Only the part of the ring over the grid; when even that is more cells than there are points (a point far
            // from a small part), looking at every point is cheaper.
            let span = |k: usize| ((-r).max(self.lo[k] - c[k]), r.min(self.hi[k] - c[k]));
            let (sx, sy, sz) = (span(0), span(1), span(2));
            let cells = [sx, sy, sz].iter().map(|(a, b)| (b - a + 1).max(0) as u64).product::<u64>();
            if cells > self.ids.len() as u64 * 4 {
                return self.ids.iter().map(|&i| (self.dist(i, p), i)).chain(best).min_by(|a, b| a.0.total_cmp(&b.0)).map(|b| b.1);
            }
            for x in sx.0..=sx.1 { for y in sy.0..=sy.1 { for z in sz.0..=sz.1 {
                if x.abs().max(y.abs()).max(z.abs()) != r { continue; }
                let Some(v) = self.grid.get(&[c[0] + x, c[1] + y, c[2] + z]) else { continue };
                for &i in v {
                    let d = self.dist(i, p);
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

/// The files of the model: (archive, path, bytes). `g` is the game without any mod's volumes.
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
        // Name: the material's own plus a short hash of its pictures, so two mods' "Material" never collide.
        let mut h = hash(base.as_bytes());
        for p in [&m.diffuse, &m.normal, &m.specular].into_iter().flatten() { h ^= hash(&std::fs::read(p).unwrap_or_default()).rotate_left(7); }
        let mname = format!("G3IE_{base}_{:06x}", h & 0xffffff);
        let made = make_material(g, m, &mname, false, now, &mut files, &mut r.warnings)?;
        r.materials.push(format!("{}: new {mname} ({})", m.name, g3_write::look_name(made.template)));
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
            // (corner positions, surface per triangle, material name per triangle, where they came from)
            let src: Option<(Vec<[f32; 3]>, Vec<u8>, Vec<String>, &str)> = match (&c.geometry, c.corners) {
                (Some(geo), Some(n)) if mode != "model" && !beside => {
                    let (cp, _, _, cm) = read_geometry(geo, n)?;
                    let shapes: Vec<u8> = c.materials.iter().map(|m| shape_of(m, default)).collect();
                    let tri: Vec<u8> = cm.iter().map(|&i| shapes.get(i as usize).copied().unwrap_or(default)).collect();
                    let names = cm.iter().map(|&i| c.materials.get(i as usize).map(|m| clean(&m.name).to_lowercase()).unwrap_or_default()).collect();
                    Some((cp, tri, names, "the *_COL objects"))
                }
                _ if existing.is_none() || beside || mode == "model" => {
                    let shapes: Vec<u8> = spec.materials.iter().map(|m| shape_of(m, default)).collect();
                    let tri: Vec<u8> = tri_mat.iter().map(|&i| shapes.get(i as usize).copied().unwrap_or(default)).collect();
                    let names = tri_mat.iter().map(|&i| spec.materials.get(i as usize).map(|m| clean(&m.name).to_lowercase()).unwrap_or_default()).collect();
                    Some((pos.clone(), tri, names, "the model itself"))
                }
                _ => None,
            };
            match src {
                Some((cp, tri_shape, tri_name, from)) => {
                    // The parts (cooked meshes) of the collision. The world's entities name each part of the game's
                    // collision by its index in the file, with their own surface (`eCCollisionShape`: file, resource
                    // index, material): a part past the game's count is never loaded, and a part moved to another
                    // index gets that index's surface. So a collision replacing the game's keeps its parts in its
                    // order — a landscape cell's are its mesh's elements, an object's its surfaces — and what fits
                    // none goes with the first. A new one has a part per surface.
                    let game_x = match &game_col { Some(k) => Some(crate::g3_res::parse_xnvmsh(&g.read(k)?).with_context(|| format!("{k}: the collision it replaces"))?), None => None };
                    let tri_group: Vec<u32> = match &game_x {
                        Some(gx) => {
                            let part_shape: Vec<Option<u8>> = (0..gx.streams.len()).map(|i| gx.shapes.get(i).map(|s| s[0])).collect();
                            // A landscape cell's parts are its game mesh's elements, in order.
                            let part_name: Vec<String> = match (&existing, beside) {
                                (Some(k), true) => crate::g3_res::decode_xcmsh(&g.read(k)?)?.geo.submeshes.iter().map(|s| clean(s.material.trim_end_matches(".xshmat")).to_lowercase()).collect(),
                                _ => vec![],
                            };
                            let mut lost: Vec<u8> = vec![];
                            let group = tri_shape.iter().zip(&tri_name).map(|(&s, n)| {
                                let i = part_name.iter().take(part_shape.len()).position(|p| p == n)
                                    .or_else(|| part_shape.iter().position(|&p| p == Some(s)))
                                    .unwrap_or_else(|| { if !lost.contains(&s) { lost.push(s); } 0 });
                                i as u32
                            }).collect();
                            let first = part_shape[0].map_or("its first part", |p| g3_write::SHAPE_MATERIALS[p as usize]);
                            for s in lost { r.warnings.push(format!("collision: the game's collision of this model has no {} part — it goes with the first ({first}); the world gives each part its own surface", g3_write::SHAPE_MATERIALS[s as usize])); }
                            group
                        }
                        None => tri_shape.iter().map(|&s| s as u32).collect(),
                    };
                    let mut groups: Vec<(u32, u8, Vec<[f32; 3]>, Vec<[u32; 3]>, HashMap<[u32; 3], u32>)> = vec![];
                    for (t, c3) in cp.chunks_exact(3).enumerate() {
                        let (key, s) = (tri_group[t], tri_shape[t]);
                        let gi = match groups.iter().position(|x| x.0 == key) { Some(i) => i, None => { groups.push((key, s, vec![], vec![], HashMap::new())); groups.len() - 1 } };
                        let gr = &mut groups[gi];
                        let ids = [0, 1, 2].map(|k| *gr.4.entry(c3[k].map(f32::to_bits)).or_insert_with(|| { gr.2.push(c3[k]); gr.2.len() as u32 - 1 }));
                        // Gothic 3's collision turns the other way round from its render meshes (PhysX faces): swap 1 and 2.
                        gr.3.push([ids[0], ids[2], ids[1]]);
                    }
                    if let Some(gx) = &game_x {
                        // Every part the world refers to, with the game's surface; one left empty holds a copy of a
                        // triangle of another (a part cannot be empty, and a missing one drops the whole collision).
                        let any = groups.first().map(|g| (g.2.clone(), g.3[0]));
                        for i in 0..gx.streams.len() as u32 {
                            let s = gx.shapes.get(i as usize).map_or(default, |s| s[0]);
                            match groups.iter_mut().find(|g| g.0 == i) {
                                Some(gr) => gr.1 = s,
                                None => if let Some((v, t)) = &any {
                                    groups.push((i, s, t.map(|k| v[k as usize]).to_vec(), vec![[0, 1, 2]], HashMap::new()));
                                    r.warnings.push(format!("collision: nothing of the model is {} — that part of the game's collision is left as a single triangle", g3_write::SHAPE_MATERIALS[s as usize]));
                                },
                            }
                        }
                    }
                    groups.sort_by_key(|g| g.0);
                    let meshes: Vec<(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)> = groups.into_iter().map(|(_, s, v, t, _)| (v, t, s)).collect();
                    let (col_archive, col_path) = match &game_col {
                        Some(k) => (archive_of(k).to_string(), g.path_of(k).unwrap_or(k).to_string()),
                        None => ("_compiledPhysic".to_string(), format!("gothic3_impexp/{file_stem}_COL.xnvmsh")),
                    };
                    let summary: Vec<String> = meshes.iter().map(|(_, t, s)| format!("{} {}", g3_write::SHAPE_MATERIALS[*s as usize], t.len())).collect();
                    // In the game's own file when it has one (its head and strings), else our plain form.
                    let mut x = g3_write::xnv_of(&meshes, Some(&g.root))?;
                    if let Some(gx) = &game_x {
                        x.shapes = gx.shapes.clone();
                        // The entities also keep each part's box from the game's file.
                        for (i, ((_, ours), (_, game))) in x.streams.iter().zip(&gx.streams).enumerate() {
                            let grows = (0..3).any(|k| ours.0[k] < game.0[k] - 50.0 || ours.1[k] > game.1[k] + 50.0);
                            if grows && game.0[0] <= game.1[0] { r.warnings.push(format!("collision: part {i} reaches more than 50 cm past the box the world's entities keep for it")); }
                        }
                    }
                    let bytes = match &game_col { Some(k) => g3_write::xnvmsh_in(&g.read(k)?, &x)?, None => g3_write::xnvmsh_write(&x) };
                    files.push((col_archive, col_path, bytes));
                    r.collision = Some(format!("{} triangles from {from}: {}", cp.len() / 3, summary.join(", ")));
                }
                None => r.collision = Some("the game's own collision stays".into()),
            }

            // The convex collision (`*_CV`): the body an item has when it lies, falls and rolls. The game's is replaced
            // (its parts in its order: one, in two files two) from the `*_CV` objects, one hull each, or else from the
            // model's own vertices; a new model gets one only from `*_CV` objects.
            let game_cv = crate::staticmesh::convex_key(g, &name);
            let mut hulls: Vec<Vec<[f32; 3]>> = vec![];
            if mode != "model" {
                for h in &c.convex { hulls.push(read_geometry(&h.geometry, h.corners)?.0); }
            }
            if hulls.is_empty() && game_cv.is_some() { hulls.push(pos.clone()); }
            if !hulls.is_empty() {
                let from = if mode != "model" && !c.convex.is_empty() { "the *_CV objects" } else { "the model itself" };
                let gx = match &game_cv { Some(k) => Some(crate::g3_res::parse_xnvmsh(&g.read(k)?).with_context(|| format!("{k}: the convex collision it replaces"))?), None => None };
                let parts = gx.as_ref().map_or(hulls.len(), |x| x.streams.len().max(1));
                if hulls.len() > parts {
                    r.warnings.push(format!("convex collision: the game's has {parts} part(s) — the last takes the hull of the other {} *_CV objects too", hulls.len() - parts + 1));
                    let rest: Vec<[f32; 3]> = hulls.drain(parts - 1..).flatten().collect();
                    hulls.push(rest);
                }
                // One point per position (a mesh's corners repeat them).
                for h in &mut hulls {
                    let mut seen = std::collections::HashSet::new();
                    h.retain(|p| seen.insert(p.map(f32::to_bits)));
                }
                let cooked = g3_write::convex_streams(&g.root, &hulls)?;
                let boxed = |h: &[[f32; 3]]| { let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]); for v in h { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } } (lo, hi) };
                let mut x = crate::g3_res::Xnv { streams: cooked.into_iter().zip(&hulls).map(|(s, h)| (s, boxed(h))).collect(), all: g3_write::EMPTY_BOX, convex: 1, shapes: vec![] };
                let (cv_archive, cv_path, bytes) = match (&game_cv, &gx) {
                    (Some(k), Some(gx)) => {
                        // Parts with no *_CV object for them keep the game's.
                        for (i, (st, b)) in gx.streams.iter().enumerate().skip(x.streams.len()) {
                            r.warnings.push(format!("convex collision: part {i} of the game's stays as it is"));
                            x.streams.push((st.clone(), *b));
                        }
                        x.shapes = gx.shapes.clone();
                        x.convex = gx.convex;
                        // The game's files mostly keep the box of all parts here.
                        if gx.all.0[0] <= gx.all.1[0] { x.all = x.streams.iter().fold(g3_write::EMPTY_BOX, |a, (_, b)| ([0, 1, 2].map(|k| a.0[k].min(b.0[k])), [0, 1, 2].map(|k| a.1[k].max(b.1[k])))); }
                        (archive_of(k).to_string(), g.path_of(k).unwrap_or(k).to_string(), g3_write::xnvmsh_in(&g.read(k)?, &x)?)
                    }
                    _ => ("_compiledPhysic".to_string(), format!("gothic3_impexp/{file_stem}_CV.xnvmsh"), g3_write::xnvmsh_write(&x)),
                };
                files.push((cv_archive, cv_path, bytes));
                r.convex = Some(format!("{} part(s) from {from}: {} points", hulls.len(), hulls.iter().map(Vec::len).sum::<usize>()));
            }
        }
    }
    r.files = files.iter().map(|(a, p, _)| format!("{a}/{p}")).collect();
    Ok((r, files))
}

pub fn run(g: &G3Ctx, game: &Path, spec_path: &str, mode: &str, arg: &str, title: &str, author: &str) -> Result<serde_json::Value> {
    let spec: Spec = serde_json::from_str(&std::fs::read_to_string(spec_path)?).context("export spec")?;
    let (report, files) = build(g, &spec)?;
    let mut out = crate::mods::deliver(game, mode, arg, title, author, &files)?;
    out["report"] = serde_json::to_value(report)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nearest vertex is the nearest one, and finding it stays quick for points far away from a small part.
    #[test]
    fn the_nearest_vertex_is_found_quickly_from_afar() {
        let mut s = 7u32;
        let mut rnd = || { s ^= s << 13; s ^= s >> 17; s ^= s << 5; (s % 10000) as f32 / 1000.0 };
        let pts: Vec<[f32; 3]> = (0..2000).map(|_| [rnd(), rnd(), rnd()]).collect();
        let near = Nearest::new(&pts, 0..pts.len() as u32);
        let started = std::time::Instant::now();
        for q in (0..300).map(|i| if i % 3 == 0 { [rnd() * 400.0, -rnd() * 300.0, rnd()] } else { [rnd(), rnd(), rnd()] }) {
            let want = (0..pts.len()).min_by(|&a, &b| near.dist(a as u32, q).total_cmp(&near.dist(b as u32, q))).unwrap() as u32;
            assert_eq!(near.dist(near.find(q).unwrap(), q), near.dist(want, q), "{q:?}");
        }
        assert!(started.elapsed().as_secs_f32() < 2.0, "{:?}", started.elapsed());
    }

    /// A spec holding the game mesh `key` itself, as the add-on would send it.
    pub fn spec_of(g: &G3Ctx, key: &str, dir: &Path, collision: &str) -> Spec {
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
        let mats = || geo.submeshes.iter().map(|s| MatSpec { name: s.material.trim_end_matches(".xshmat").to_string(), ..Default::default() }).collect();
        Spec { name: crate::stem_of(key), geometry: geof.to_string_lossy().into_owned(), corners: t.len() * 3, materials: mats(),
            collision: Some(CollisionSpec { geometry: None, corners: None, material: Some("stone".into()), mode: Some(collision.into()), materials: vec![], convex: vec![] }) }
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

    /// The world's entities name each part of a collision by its index in the file, with their own surface
    /// (`eCCollisionShape`: file, resource index, material). A building whose collision comes from its own
    /// triangles keeps the game's parts in the game's order: the mill's are stone, then wood. Its gravel floor has
    /// no part of its own, so it goes with the first one; nothing is left out.
    #[test]
    fn a_replaced_collision_keeps_the_parts_the_world_refers_to() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_objects_myrtana_buildings_01/g3_myrtana_house_mill_water_01.xcmsh";
        let (r, files) = build(&g, &spec_of(&g, key, &std::env::temp_dir().join("g3ie_test_mill_col"), "model")).unwrap();
        let col = files.iter().find(|f| f.1.to_lowercase().ends_with("_col.xnvmsh")).expect("collision");
        let x = crate::g3_res::parse_xnvmsh(&col.2).unwrap();
        let gx = crate::g3_res::parse_xnvmsh(&g.read("_compiledphysic/g3_myrtana_house_mill_water_01_col.xnvmsh").unwrap()).unwrap();
        assert_eq!(x.streams.len(), gx.streams.len(), "{:?}", r.collision);
        assert_eq!(x.shapes, gx.shapes, "{:?}", r.collision);
        let tris = crate::g3_res::xnvmsh_triangles(&col.2).unwrap().len();
        // The cooking drops the model's few degenerate triangles.
        assert!(tris <= r.triangles && tris * 1000 >= r.triangles * 999, "{tris} of {}: {:?}", r.triangles, r.collision);
        assert!(r.warnings.iter().any(|w| w.contains("debris")), "{:?}", r.warnings);
    }

    /// A new model's materials take the template of their look, with our pictures and a flat normal where the Blender
    /// material has none; an actor's take the skinned ones.
    #[test]
    fn new_materials_take_the_template_of_their_look() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_looks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = |n: &str, a: u8| { let p = dir.join(format!("{n}.png")); image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 90, 40, a])).save(&p).unwrap(); Some(p.to_string_lossy().into_owned()) };
        let m = |name: &str, normal: bool, specular: bool, alpha: Option<u8>, blend: Option<&str>| MatSpec { name: name.into(), diffuse: png(name, if alpha.is_some() || blend.is_some() { 100 } else { 255 }), normal: if normal { png(&format!("{name}_n"), 255) } else { None }, specular: if specular { png(&format!("{name}_s"), 255) } else { None }, alpha_test: alpha, blend: blend.map(String::from), ..Default::default() };
        let mats = vec![m("Plank", true, false, None, None), m("Iron", true, true, None, None), m("Leaves", false, false, Some(90), None), m("Glass", true, false, None, Some("blend")), m("Fire", false, false, None, Some("glow"))];
        // One triangle per material.
        let mut b = vec![];
        let n = mats.len();
        for t in 0..n { for k in 0..3 { for x in [t as f32 * 100.0 + k as f32 * 30.0, (k % 2) as f32 * 50.0, (k / 2) as f32 * 40.0] { b.extend(x.to_le_bytes()); } } }
        for _ in 0..n * 3 { for x in [0f32, 1.0, 0.0] { b.extend(x.to_le_bytes()); } }
        for t in 0..n * 3 { for x in [(t % 3) as f32 * 0.5, (t % 2) as f32] { b.extend(x.to_le_bytes()); } }
        for t in 0..n as u32 { b.extend(t.to_le_bytes()); }
        let geo = dir.join("geometry.bin");
        std::fs::write(&geo, b).unwrap();
        let spec = Spec { name: "G3IE_Test_Lamp_Stand".into(), geometry: geo.to_string_lossy().into_owned(), corners: n * 3, materials: mats, collision: None };
        let (r, files) = build(&g, &spec).unwrap();
        let read = |stem: &str| { let f = files.iter().find(|f| f.1.ends_with(".xshmat") && f.1.contains(stem)).unwrap_or_else(|| panic!("{stem}: {:?}", r.files)); crate::g3_res::parse_xshmat(&f.2).unwrap() };
        assert_eq!(read("Plank").blend, Some(0));
        assert_eq!(read("Iron").textures.len(), 3);
        assert_eq!(read("Leaves").blend, Some(1));
        assert_eq!(read("Glass").blend, Some(2));
        assert_eq!(read("Fire").blend, Some(7));
        assert!(r.materials.iter().any(|x| x.starts_with("Iron") && x.ends_with("(specular)")), "{:?}", r.materials);
        assert!(r.warnings.iter().any(|w| w.contains("Glass") && w.contains("normal")), "{:?}", r.warnings);
        // A leaf without a normal map gets a flat one, not the straw roof's.
        let leaves = read("Leaves");
        assert!(leaves.textures.iter().all(|t| t.starts_with("G3IE_Leaves_")), "{:?}", leaves.textures);
        let flat = files.iter().find(|f| f.1.contains("G3IE_Leaves_") && f.1.ends_with("_Normal_01.ximg")).expect("flat normal");
        let (_, _, px) = crate::textures::decode(&flat.2).unwrap();
        assert!(px.chunks_exact(4).all(|p| (p[0] as i32 - 128).abs() < 3 && (p[1] as i32 - 128).abs() < 3 && p[2] > 250), "{:?}", &px[..8]);
        // The same materials on an actor: skinned templates.
        let mut warn = vec![];
        let mut out = vec![];
        for mat in &spec.materials {
            let made = make_material(&g, mat, &format!("G3IE_Actor_{}", mat.name), true, 0, &mut out, &mut warn).unwrap();
            let x = &out.iter().rev().find(|f| f.1.ends_with(".xshmat")).unwrap().2;
            assert_eq!(crate::g3_res::enum_property(x, "TransformationType").unwrap(), Some(2), "{}", mat.name);
            assert_eq!(crate::g3_res::parse_xshmat(x).unwrap().blend, Some(made.template.blend));
        }
        assert!(warn.iter().any(|w| w.contains("Glass") && w.contains("for actors")), "{warn:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Names become file names without two of them ever becoming one: Cyrillic in Latin, anything else hashed.
    #[test]
    fn names_stay_apart() {
        assert_eq!(clean("Млин"), "Mlyn");
        assert_eq!(clean("Хата_Щука.001"), "Khata_Shchuka");
        assert_eq!(clean("Barrel 02"), "Barrel_02");
        assert_ne!(clean("日本"), clean("中国"));
        assert!(clean("日本").starts_with("___"));
    }

    /// About a thousand models keep their collision as `_compiledPhysic/<name>.xnvmsh` (no `_COL`): a replaced one
    /// writes that file, with the game's parts, not a new `_COL` nothing refers to.
    #[test]
    fn a_collision_without_col_in_its_name_is_replaced_where_it_is() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = crate::staticmesh::mesh_key(&g, "G3_Myrtana_Faring_Castle_Wall_Stairs_01").unwrap();
        let (r, files) = build(&g, &spec_of(&g, &key, &std::env::temp_dir().join("g3ie_test_stairs"), "model")).unwrap();
        let col = files.iter().find(|f| f.0 == "_compiledPhysic").unwrap_or_else(|| panic!("no collision: {:?}", r.files));
        assert!(col.1.eq_ignore_ascii_case("G3_Myrtana_Faring_Castle_Wall_Stairs_01.xnvmsh") || col.1.to_lowercase().ends_with("/g3_myrtana_faring_castle_wall_stairs_01.xnvmsh"), "{}", col.1);
        let gx = crate::g3_res::parse_xnvmsh(&g.read("_compiledphysic/g3_myrtana_faring_castle_wall_stairs_01.xnvmsh").unwrap()).unwrap();
        let x = crate::g3_res::parse_xnvmsh(&col.2).unwrap();
        assert_eq!((x.streams.len(), &x.shapes), (gx.streams.len(), &gx.shapes), "{:?}", r.collision);
    }

    /// An item replaced by its own model gets its convex body (`*_CV`) again, cooked by the game: a hull over the same
    /// box as the game's, in the game's file.
    #[test]
    fn a_replaced_item_gets_its_convex_body_again() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = crate::staticmesh::mesh_key(&g, "G3_Armor_Helm_Myrtana_01").unwrap();
        let (r, files) = build(&g, &spec_of(&g, &key, &std::env::temp_dir().join("g3ie_test_helm_cv"), "auto")).unwrap();
        let cv = files.iter().find(|f| f.1.to_lowercase().ends_with("_cv.xnvmsh")).unwrap_or_else(|| panic!("no convex collision: {:?}", r.files));
        assert_eq!(cv.0, "_compiledPhysic");
        let (x, gx) = (crate::g3_res::parse_xnvmsh(&cv.2).unwrap(), crate::g3_res::parse_xnvmsh(&g.read(&crate::staticmesh::convex_key(&g, "G3_Armor_Helm_Myrtana_01").unwrap()).unwrap()).unwrap());
        assert_eq!((x.streams.len(), x.convex, &x.shapes), (gx.streams.len(), gx.convex, &gx.shapes));
        let (ours, game) = (crate::nxs::read_convex_hull(&x.streams[0].0).unwrap(), crate::nxs::read_convex_hull(&gx.streams[0].0).unwrap());
        assert!(ours.1.len() >= 4, "{} hull triangles", ours.1.len());
        let bx = |v: &[[f32; 3]]| (0..3).map(|k| (v.iter().map(|p| p[k]).fold(f32::MAX, f32::min), v.iter().map(|p| p[k]).fold(f32::MIN, f32::max))).collect::<Vec<_>>();
        for (a, b) in bx(&ours.0).iter().zip(bx(&game.0)) { assert!((a.0 - b.0).abs() < 0.03 && (a.1 - b.1).abs() < 0.03, "box {:?} vs the game's {:?}", bx(&ours.0), bx(&game.0)); }
        assert!(r.convex.as_deref().is_some_and(|c| c.contains("the model itself")), "{:?}", r.convex);
    }

    /// A landscape cell's parts are its mesh's elements: one painted with a material of its own still writes the
    /// game's eight parts, the new material's triangles in the part of its surface (grass: clay).
    #[test]
    fn a_cell_with_a_new_material_keeps_its_parts() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_myrtana_landscape_01/lod/g3_myrtana_landscape_cell_244.xcmsh";
        let mut spec = spec_of(&g, key, &std::env::temp_dir().join("g3ie_test_cell_new"), "auto");
        let i = spec.materials.iter().position(|m| m.name == "G3_Nature_Ground_Grass_01_A").unwrap();
        spec.materials[i].name = "My_Meadow_Grass".into();
        let (r, files) = build(&g, &spec).unwrap();
        let col = files.iter().find(|f| f.1.to_lowercase().ends_with("cell_244.xnvmsh")).unwrap();
        let x = crate::g3_res::parse_xnvmsh(&col.2).unwrap();
        let gx = crate::g3_res::parse_xnvmsh(&g.read(&key.replace(".xcmsh", ".xnvmsh")).unwrap()).unwrap();
        assert_eq!(x.streams.len(), gx.streams.len(), "{:?}", r.collision);
        assert_eq!(x.shapes, gx.shapes);
        assert_eq!(crate::g3_res::xnvmsh_triangles(&col.2).unwrap().len(), 14226, "{:?} {:?}", r.collision, r.warnings);
    }
}

#[cfg(test)]
mod game_cook_tests {
    use super::*;

    /// The exported cell's collision is cooked by the game's library: every stream reads back with its
    /// triangles, materials set like the game's own (flag 1), and the triangles are the model's.
    #[test]
    fn exported_collision_is_cooked_by_the_game() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_myrtana_landscape_01/lod/g3_myrtana_landscape_cell_238.xcmsh";
        let (_, files) = build(&g, &tests::spec_of(&g, key, &std::env::temp_dir().join("g3ie_test_cook"), "model")).unwrap();
        let col = files.iter().find(|f| f.1.to_lowercase().ends_with(".xnvmsh")).unwrap();
        assert!(col.2.starts_with(b"GENOMFLE"), "in the game's own file");
        let x = crate::g3_res::parse_xnvmsh(&col.2).unwrap();
        let mut tris = 0;
        for (st, _) in &x.streams {
            let m = crate::nxs::read_trimesh(&mut crate::nxs::Reader { d: st, at: 0 }).unwrap();
            assert_eq!(m.flags & 1, 1, "materials as the game cooks them");
            tris += m.tris.len();
        }
        let game = crate::g3_res::decode_xcmsh(&g.read(key).unwrap()).unwrap();
        assert_eq!(tris, game.geo.indices.len() / 3);
    }
}

#[cfg(test)]
mod winding_tests {
    use super::*;

    /// Up-facing ground: the game's cell collision has most triangles turned one way (opposite to its render
    /// mesh); a cell exported from its own triangles must turn the same way.
    #[test]
    fn exported_collision_turns_like_the_games() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let key = "_compiledmesh/g3_myrtana_landscape_01/lod/g3_myrtana_landscape_cell_244.xcmsh";
        let up = |t: &[[f32; 3]; 3]| { let (a, b, c) = (t[0], t[1], t[2]); (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]) > 0.0 };
        let game = crate::g3_res::xnvmsh_triangles(&g.read(&key.replace(".xcmsh", ".xnvmsh")).unwrap()).unwrap();
        let (_, files) = build(&g, &tests::spec_of(&g, key, &std::env::temp_dir().join("g3ie_test_wind"), "auto")).unwrap();
        let ours = crate::g3_res::xnvmsh_triangles(&files.iter().find(|f| f.1.to_lowercase().ends_with(".xnvmsh")).unwrap().2).unwrap();
        let (gu, ou) = (game.iter().filter(|t| up(t)).count(), ours.iter().filter(|t| up(t)).count());
        assert!(gu * 2 > game.len() && ou * 2 > ours.len(), "game {gu}/{}, ours {ou}/{}", game.len(), ours.len());
    }
}
