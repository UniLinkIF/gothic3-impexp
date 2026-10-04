//! Export of a static model from Blender: geometry in game space (the add-on mirrors Blender's axes once, so
//! triangles keep their order), materials as PNGs, optional collision — to Gothic 3 files, then a mod.
//!
//! * The mesh replaces the game's mesh of the same name (same archive path), or is new
//!   (`_compiledMesh/gothic3_impexp/<name>.xcmsh`).
//! * A material whose name the game has is used as it is; another one becomes `<name>.xshmat` from a template
//!   (opaque, opaque with specular, alpha test), its images `<name>_Diffuse_01` / `_Normal_01` / `_Specular_01`.
//! * Collision: the selected `*_COL` objects, else the mesh itself for a new model; a replaced model keeps the
//!   game's collision unless `*_COL` objects come along.

use crate::g3::G3Ctx;
use crate::g3_write::{self, Dxt};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
pub struct MatSpec { pub name: String, pub diffuse: Option<String>, pub normal: Option<String>, #[serde(default)] pub specular: Option<String>, #[serde(default)] pub alpha_test: Option<u8> }

#[derive(Deserialize)]
pub struct CollisionSpec { pub geometry: Option<String>, pub corners: Option<usize>, #[serde(default)] pub material: Option<String>, #[serde(default)] pub mode: Option<String> }

#[derive(Deserialize)]
pub struct Spec { pub name: String, pub geometry: String, pub corners: usize, pub materials: Vec<MatSpec>, pub collision: Option<CollisionSpec> }

#[derive(serde::Serialize, Default)]
pub struct Report { pub name: String, pub replaced: bool, pub vertices: usize, pub triangles: usize, pub materials: Vec<String>, pub collision: Option<String>, pub files: Vec<String>, pub warnings: Vec<String> }

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

/// The Gothic 3 files of the model: (archive, path, bytes).
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

    // Parts per material: one vertex per (position, normal, uv).
    let mut parts: Vec<crate::xcmsh_write::Part> = mat_names.iter().map(|m| crate::xcmsh_write::Part { material: m.clone(), positions: vec![], normals: vec![], uvs: vec![], triangles: vec![] }).collect();
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
    r.vertices = parts.iter().map(|p| p.positions.len()).sum();
    r.triangles = spec.corners / 3;
    // A mesh of ours already installed counts as new (its path stays ours, collision is built again).
    let existing = crate::staticmesh::mesh_key(g, &name).filter(|k| !k.contains("/gothic3_impexp/"));
    r.replaced = existing.is_some();
    let path = match &existing { Some(k) => g.path_of(k).unwrap_or(k).to_string(), None => format!("gothic3_impexp/{name}.xcmsh") };
    let file_stem = path.rsplit('/').next().unwrap().trim_end_matches(".xcmsh").to_string();
    files.push(("_compiledMesh".to_string(), path, crate::xcmsh_write::write(&parts)));

    // Collision.
    if let Some(c) = &spec.collision {
        if c.mode.as_deref() != Some("none") {
            let shape = c.material.as_deref().and_then(|m| g3_write::SHAPE_MATERIALS.iter().position(|x| *x == m)).unwrap_or(4) as u8;
            let tris_of = |p: &[[f32; 3]]| -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
                let mut map: HashMap<[u32; 3], u32> = HashMap::new();
                let mut v = vec![];
                let t = p.chunks_exact(3).map(|c| [0, 1, 2].map(|k| *map.entry(c[k].map(f32::to_bits)).or_insert_with(|| { v.push(c[k]); v.len() as u32 - 1 }))).collect();
                (v, t)
            };
            let src = match (&c.geometry, c.corners) {
                (Some(geo), Some(n)) => Some((read_geometry(geo, n)?.0, "the *_COL objects")),
                _ if existing.is_none() => Some((pos.clone(), "the model itself")),
                _ => None,
            };
            match src {
                Some((cp, from)) => {
                    let (v, t) = tris_of(&cp);
                    let col_path = match g.find("_compiledphysic", &format!("{file_stem}_COL.xnvmsh")) { Some(k) => g.path_of(&k).unwrap_or(&k).to_string(), None => format!("gothic3_impexp/{file_stem}_COL.xnvmsh") };
                    files.push(("_compiledPhysic".to_string(), col_path, g3_write::xnvmsh(&[(v, t.clone(), shape)])?));
                    r.collision = Some(format!("{} triangles from {from} ({})", t.len(), g3_write::SHAPE_MATERIALS[shape as usize]));
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
