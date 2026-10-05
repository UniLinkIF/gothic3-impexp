//! A static mesh (`.xcmsh`) and its collision (`.xnvmsh`) as OBJ for Blender's OBJ importer.
//!
//! Gothic 3 is left-handed, Y up, in centimetres. The OBJ is written as (x, y, -z) and imported with forward -Z,
//! up Y and scale 0.01, so Blender gets metres in its own axes. Positions are welded (one OBJ vertex per position);
//! every corner keeps its own uv and normal, so seams stay. Triangle order is taken as stored, unless most
//! triangles face away from their own normals, in which case all are turned.

use crate::g3::G3Ctx;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(serde::Serialize, Clone, Debug)]
pub struct Material { pub name: String, pub diffuse: Option<String>, pub normal: Option<String>, pub specular: Option<String>, pub blend: u32, pub mask: u8 }

#[derive(serde::Serialize)]
pub struct MeshOut { pub entry: String, pub obj: String, pub vertices: usize, pub triangles: usize, pub materials: Vec<Material>, pub warnings: Vec<String> }

pub fn mesh_key(g: &G3Ctx, name: &str) -> Option<String> {
    let n = name.trim_end_matches(".xcmsh");
    g.find("_compiledmesh", &format!("{n}.xcmsh"))
}

/// The material called `name` (`X.xshmat`) with its maps as PNGs in `cache`.
pub fn material(g: &G3Ctx, name: &str, cache: &Path, warnings: &mut Vec<String>) -> Material {
    let base = name.trim_end_matches(".xshmat");
    let mut m = Material { name: base.to_string(), diffuse: None, normal: None, specular: None, blend: 0, mask: 128 };
    let Some(key) = g.find("_compiledmaterial", &format!("{base}.xshmat")) else { warnings.push(format!("material {base}: not in the game")); return m };
    let parsed = match g.read(&key).and_then(|d| crate::g3_res::parse_xshmat(&d)) { Ok(p) => p, Err(e) => { warnings.push(format!("material {base}: {e:#}")); return m } };
    m.blend = parsed.blend.unwrap_or(0);
    let (d, n, s) = crate::textures::maps(&parsed.textures);
    let mut get = |t: &Option<String>, normal: bool| -> Option<String> {
        let t = t.as_ref()?;
        match crate::textures::png(g, t, cache, normal) {
            Ok(Some(p)) => Some(p.file_name().unwrap().to_string_lossy().into_owned()),
            Ok(None) => { warnings.push(format!("material {base}: texture {t} not in the game")); None }
            Err(e) => { warnings.push(format!("material {base}: {t}: {e:#}")); None }
        }
    };
    m.diffuse = get(&d, false);
    m.normal = get(&n, true);
    m.specular = get(&s, false);
    m
}

/// True when the stored order has to be turned for counter-clockwise fronts once mirrored to (x, y, -z).
pub fn needs_turn(pos: &[[f32; 3]], nrm: &[[f32; 3]], tris: impl Iterator<Item = [u32; 3]>) -> bool {
    let m = |p: [f32; 3]| [p[0], p[1], -p[2]];
    let (mut agree, mut disagree) = (0usize, 0usize);
    for t in tris {
        let [a, b, c] = t.map(|i| m(pos[i as usize]));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let s: [f32; 3] = t.iter().fold([0.0; 3], |acc, &i| { let q = m(nrm[i as usize]); [acc[0] + q[0], acc[1] + q[1], acc[2] + q[2]] });
        let d = n[0] * s[0] + n[1] * s[1] + n[2] * s[2];
        if d > 0.0 { agree += 1 } else if d < 0.0 { disagree += 1 }
    }
    disagree > agree
}

pub fn export_obj(g: &G3Ctx, name: &str, out_dir: &Path) -> Result<MeshOut> {
    let key = mesh_key(g, name).with_context(|| format!("{name}: no such mesh in Gothic 3"))?;
    let stem = key.rsplit('/').next().unwrap().trim_end_matches(".xcmsh").to_string();
    let geo = crate::g3_res::decode_xcmsh(&g.read(&key)?).with_context(|| format!("decode {key}"))?.geo;
    std::fs::create_dir_all(out_dir)?;
    let mut warnings = vec![];
    let materials: Vec<Material> = geo.submeshes.iter().map(|s| material(g, &s.material, out_dir, &mut warnings)).collect();
    let has_n = geo.normals.len() == geo.positions.len();
    let has_uv = geo.uvs.len() == geo.positions.len();
    let turn = has_n && needs_turn(&geo.positions, &geo.normals, geo.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]));

    let mut obj = format!("# Gothic 3 {stem}: game axes mirrored in Z, centimetres\nmtllib {stem}.mtl\no {stem}\n");
    let mut weld: HashMap<[u32; 3], usize> = HashMap::new();
    let mut vid = Vec::with_capacity(geo.positions.len());
    for p in &geo.positions {
        let n = weld.len();
        let i = *weld.entry(p.map(f32::to_bits)).or_insert_with(|| { let _ = writeln!(obj, "v {} {} {}", p[0], p[1], -p[2]); n });
        vid.push(i + 1);
    }
    if has_uv { for t in &geo.uvs { let _ = writeln!(obj, "vt {} {}", t[0], 1.0 - t[1]); } }
    if has_n { for n in &geo.normals { let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], -n[2]); } }
    let corner = |i: u32| -> String {
        let (v, k) = (vid[i as usize], i as usize + 1);
        match (has_uv, has_n) { (true, true) => format!("{v}/{k}/{k}"), (true, false) => format!("{v}/{k}"), (false, true) => format!("{v}//{k}"), _ => format!("{v}") }
    };
    let mut tris = 0;
    for (s, m) in geo.submeshes.iter().zip(&materials) {
        let _ = writeln!(obj, "usemtl {}", m.name);
        let idx = &geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize];
        for c in idx.chunks_exact(3) {
            let (a, b, d) = if turn { (c[0], c[2], c[1]) } else { (c[0], c[1], c[2]) };
            let _ = writeln!(obj, "f {} {} {}", corner(a), corner(b), corner(d));
            tris += 1;
        }
    }
    let mut mtl = String::new();
    for m in &materials {
        let _ = writeln!(mtl, "newmtl {}\nKd 0.8 0.8 0.8", m.name);
        if let Some(d) = &m.diffuse { let _ = writeln!(mtl, "map_Kd {d}"); }
        if let Some(n) = &m.normal { let _ = writeln!(mtl, "map_Bump -bm 1 {n}"); }
    }
    let obj_path = out_dir.join(format!("{stem}.obj"));
    std::fs::write(&obj_path, obj)?;
    std::fs::write(out_dir.join(format!("{stem}.mtl")), mtl)?;
    Ok(MeshOut { entry: key, obj: obj_path.to_string_lossy().into_owned(), vertices: weld.len(), triangles: tris, materials, warnings })
}

/// Where the game keeps the collision of mesh `name`: `_compiledPhysic/<name>_COL.xnvmsh` for objects, or next
/// to the mesh as `<name>.xnvmsh` (the landscape cells, whose collision is their own triangles).
pub fn collision_key(g: &G3Ctx, name: &str) -> Option<String> {
    let n = name.trim_end_matches(".xcmsh").trim_end_matches("_COL");
    g.find("_compiledphysic", &format!("{n}_COL.xnvmsh")).or_else(|| mesh_key(g, n).map(|k| k.replace(".xcmsh", ".xnvmsh")).filter(|k| g.size_of(k).is_some()))
}

/// The shape material of stream `i` (the landscape lists one per stream; objects' tables need not match).
pub fn stream_shape(x: &crate::g3_res::Xnv, i: usize) -> &'static str {
    let v = x.shapes.get(i).map(|s| s[0] as usize).unwrap_or(0);
    crate::g3_write::SHAPE_MATERIALS.get(v).copied().unwrap_or("none")
}

/// The collision of mesh `name` as an OBJ, one group per cooked mesh with its shape material as the OBJ material
/// (`G3_Shape_<material>`), or an error when the game has none.
pub fn collision_obj(g: &G3Ctx, name: &str, out: &Path) -> Result<serde_json::Value> {
    let n = name.trim_end_matches(".xcmsh").trim_end_matches("_COL");
    let key = collision_key(g, n).with_context(|| format!("{n}: Gothic 3 has no collision mesh for it"))?;
    let x = crate::g3_res::parse_xnvmsh(&g.read(&key)?)?;
    let mut obj = String::from("# Gothic 3 collision: game axes mirrored in Z, centimetres\n");
    let mut weld: HashMap<[u32; 3], usize> = HashMap::new();
    let mut faces = String::new();
    let mut tris = 0;
    let mut shapes = vec![];
    for (i, (st, _)) in x.streams.iter().enumerate() {
        let m = crate::nxs::read_trimesh(&mut crate::nxs::Reader { d: st, at: 0 }).with_context(|| format!("{key}: stream {i}"))?;
        let shape = stream_shape(&x, i);
        shapes.push(shape);
        let _ = writeln!(faces, "usemtl G3_Shape_{shape}");
        for t in &m.tris {
            let ids: Vec<usize> = t.iter().map(|&k| {
                let p = m.verts[k as usize].map(|v| v * 100.0);
                let id = weld.len();
                *weld.entry(p.map(f32::to_bits)).or_insert_with(|| { let _ = writeln!(obj, "v {} {} {}", p[0], p[1], -p[2]); id }) + 1
            }).collect();
            // Collision turns the other way round from render meshes: swapped, its faces point out in Blender too.
            let _ = writeln!(faces, "f {} {} {}", ids[0], ids[2], ids[1]);
            tris += 1;
        }
    }
    obj += &faces;
    if let Some(p) = out.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(out, obj)?;
    Ok(serde_json::json!({ "entry": key, "obj": out.to_string_lossy(), "triangles": tris, "shapes": shapes }))
}
