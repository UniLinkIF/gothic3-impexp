//! A static mesh (`.xcmsh`) and its collision (`.xnvmsh`) as OBJ for Blender's OBJ importer.
//!
//! Gothic 3 is left-handed, Y up, in centimetres. The OBJ is written as (x, y, -z) and imported with forward -Z,
//! up Y and scale 0.01, so Blender gets metres in its own axes. Positions are welded (one OBJ vertex per position);
//! every corner keeps its own uv and normal, so seams stay. Triangle order is taken as stored, unless most
//! triangles face away from their own normals, in which case all are turned.
//!
//! The further coordinate sets (texture sets 2 to 4, the lightmap's; see `g3_res::EXTRA_UV_STREAMS`) go beside it as
//! `<stem>.uvx`, as the game keeps them, for the add-on to put on as UV layers:
//! ```text
//! "G3UVX1" · u32 sets · u32 stream type per set · u32 faces · per face (OBJ order): u8 1 when it has vertices of its
//!   own (see below) · per set, per corner: f32 u, f32 v
//! ```

use crate::g3::G3Ctx;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

#[derive(serde::Serialize, Clone, Debug)]
pub struct Material { pub name: String, pub diffuse: Option<String>, pub normal: Option<String>, pub specular: Option<String>, pub blend: u32, pub mask: u8 }

#[derive(serde::Serialize)]
pub struct MeshOut { pub entry: String, pub obj: String, pub vertices: usize, pub triangles: usize, pub materials: Vec<Material>, pub warnings: Vec<String>, pub uvx: Option<String> }

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
    let mesh = crate::g3_res::decode_xcmsh(&g.read(&key)?).with_context(|| format!("decode {key}"))?;
    let (geo, m_extra) = (&mesh.geo, &mesh.extra_uvs);
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
    // Blender's importer drops a face whose corners share a vertex, and a second face on the same three vertices (the
    // game's double-sided parts): those get vertices of their own, so every triangle comes in and goes back out.
    let order = |c: &[u32]| if turn { [c[0], c[2], c[1]] } else { [c[0], c[1], c[2]] };
    let mut faces_on: std::collections::HashSet<[usize; 3]> = std::collections::HashSet::new();
    let mut own: Vec<Option<[usize; 3]>> = vec![];
    let mut next = weld.len();
    for c in geo.indices.chunks_exact(3) {
        let f = order(c);
        let mut v = f.map(|i| vid[i as usize]);
        v.sort();
        if v[0] != v[1] && v[1] != v[2] && faces_on.insert(v) { own.push(None); continue; }
        own.push(Some(f.map(|i| { let p = geo.positions[i as usize]; let _ = writeln!(obj, "v {} {} {}", p[0], p[1], -p[2]); next += 1; next })));
    }
    if has_uv { for t in &geo.uvs { let _ = writeln!(obj, "vt {} {}", t[0], 1.0 - t[1]); } }
    if has_n { for n in &geo.normals { let _ = writeln!(obj, "vn {} {} {}", n[0], n[1], -n[2]); } }
    let corner = |i: u32, own: Option<usize>| -> String {
        let (v, k) = (own.unwrap_or(vid[i as usize]), i as usize + 1);
        match (has_uv, has_n) { (true, true) => format!("{v}/{k}/{k}"), (true, false) => format!("{v}/{k}"), (false, true) => format!("{v}//{k}"), _ => format!("{v}") }
    };
    let mut tris = 0;
    // The further coordinate sets, per face corner as the faces go (Blender has no place for them in an OBJ).
    let mut uvx = vec![];
    for (s, m) in geo.submeshes.iter().zip(&materials) {
        let _ = writeln!(obj, "usemtl {}", m.name);
        let idx = &geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize];
        for c in idx.chunks_exact(3) {
            let [a, b, d] = order(c);
            let o = own[tris];
            let _ = writeln!(obj, "f {} {} {}", corner(a, o.map(|x| x[0])), corner(b, o.map(|x| x[1])), corner(d, o.map(|x| x[2])));
            if !m_extra.is_empty() {
                uvx.push(o.is_some() as u8);
                for (_, v) in m_extra { for i in [a, b, d] { for x in v[i as usize] { uvx.extend(x.to_le_bytes()); } } }
            }
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
    let uvx_path = out_dir.join(format!("{stem}.uvx"));
    let uvx = if m_extra.is_empty() { let _ = std::fs::remove_file(&uvx_path); None } else {
        let mut b = b"G3UVX1".to_vec();
        b.extend((m_extra.len() as u32).to_le_bytes());
        for (t, _) in m_extra { b.extend(t.to_le_bytes()); }
        b.extend((tris as u32).to_le_bytes());
        b.extend(uvx);
        std::fs::write(&uvx_path, b)?;
        Some(uvx_path.to_string_lossy().into_owned())
    };
    Ok(MeshOut { entry: key, obj: obj_path.to_string_lossy().into_owned(), vertices: weld.len(), triangles: tris, materials, warnings, uvx })
}

/// `name` without `.xcmsh` and a `_COL` / `_CV` ending (any case).
fn base_name(name: &str) -> &str {
    let n = name.strip_suffix(".xcmsh").unwrap_or(name);
    for end in ["_col", "_cv"] {
        if n.len() > end.len() && n.is_char_boundary(n.len() - end.len()) && n[n.len() - end.len()..].eq_ignore_ascii_case(end) { return &n[..n.len() - end.len()]; }
    }
    n
}

/// Where the game keeps the collision of mesh `name`: `_compiledPhysic/<name>_COL.xnvmsh` for most objects,
/// `_compiledPhysic/<name>.xnvmsh` for about a thousand others (buildings, furniture), or next to the mesh as
/// `<name>.xnvmsh` (the landscape cells, whose collision is their own triangles).
pub fn collision_key(g: &G3Ctx, name: &str) -> Option<String> {
    let n = base_name(name);
    g.find("_compiledphysic", &format!("{n}_COL.xnvmsh")).or_else(|| g.find("_compiledphysic", &format!("{n}.xnvmsh"))).or_else(|| mesh_key(g, n).map(|k| k.replace(".xcmsh", ".xnvmsh")).filter(|k| g.size_of(k).is_some()))
}

/// Where the game keeps the convex collision of mesh `name` (the body of an item that falls and rolls):
/// `_compiledPhysic/<name>_CV.xnvmsh`.
pub fn convex_key(g: &G3Ctx, name: &str) -> Option<String> {
    g.find("_compiledphysic", &format!("{}_CV.xnvmsh", base_name(name)))
}

/// The shape material of stream `i` (the landscape lists one per stream; objects' tables need not match).
pub fn stream_shape(x: &crate::g3_res::Xnv, i: usize) -> &'static str {
    let v = x.shapes.get(i).map(|s| s[0] as usize).unwrap_or(0);
    crate::g3_write::SHAPE_MATERIALS.get(v).copied().unwrap_or("none")
}

/// The triangles (cm, game axes, as stored) of one stream of a `.xnvmsh`: a cooked triangle mesh, an older cooking
/// of one (a few plain files, read loosely), or a convex hull.
pub fn stream_triangles(st: &[u8]) -> Result<Vec<[[f32; 3]; 3]>> {
    let cm = |verts: &[[f32; 3]], tris: &[[u32; 3]]| tris.iter().map(|t| t.map(|k| verts[k as usize].map(|v| v * 100.0))).collect();
    if st.starts_with(b"NXS\x01CVXM") {
        let (v, t) = crate::nxs::read_convex_hull(st)?;
        return Ok(cm(&v, &t));
    }
    match crate::nxs::read_trimesh(&mut crate::nxs::Reader { d: st, at: 0 }) {
        Ok(m) => Ok(cm(&m.verts, &m.tris)),
        Err(e) => crate::g3_res::xnvmsh_triangles(st).map_err(|_| e),
    }
}

/// The collision of mesh `name` as an OBJ, one group per cooked mesh with its shape material as the OBJ material
/// (`G3_Shape_<material>`), or an error when the game has none. `convex`: its `*_CV` hull instead.
pub fn collision_obj(g: &G3Ctx, name: &str, out: &Path, convex: bool) -> Result<serde_json::Value> {
    let n = base_name(name);
    let key = if convex { convex_key(g, n).with_context(|| format!("{n}: Gothic 3 has no convex collision for it")) } else { collision_key(g, n).with_context(|| format!("{n}: Gothic 3 has no collision mesh for it")) }?;
    let x = crate::g3_res::parse_xnvmsh(&g.read(&key)?).with_context(|| format!("{key}: read"))?;
    let mut obj = String::from("# Gothic 3 collision: game axes mirrored in Z, centimetres\n");
    let mut weld: HashMap<[u32; 3], usize> = HashMap::new();
    let mut faces = String::new();
    let mut tris = 0;
    let mut shapes = vec![];
    for (i, (st, _)) in x.streams.iter().enumerate() {
        let ts = stream_triangles(st).with_context(|| format!("{key}: stream {i}"))?;
        let shape = stream_shape(&x, i);
        shapes.push(shape);
        let _ = writeln!(faces, "usemtl G3_Shape_{shape}");
        for t in &ts {
            let ids: Vec<usize> = t.iter().map(|p| {
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

#[cfg(test)]
mod tests {
    /// Every collision file of the game parses, and every stream in it reads: triangle meshes (written back byte for
    /// byte) and convex hulls.
    #[test]
    fn every_collision_of_the_game_reads() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut files, mut meshes, mut old, mut hulls, mut bad) = (0, 0, 0, 0, vec![]);
        // The ones models use: what `collision_key` finds for every mesh, and every `*_CV`.
        let mut keys: std::collections::BTreeSet<String> = g.keys().into_iter().filter(|k| k.ends_with("_cv.xnvmsh")).collect();
        for m in g.keys().into_iter().filter(|k| k.ends_with(".xcmsh")) { keys.extend(super::collision_key(&g, &crate::stem_of(&m))); }
        for k in keys {
            // The editor's helper shapes (cones, capsules) are not models.
            if k.contains("supporter") || k.contains("editorhelper") { continue; }
            let d = g.read(&k).unwrap();
            files += 1;
            let x = match crate::g3_res::parse_xnvmsh(&d) { Ok(x) => x, Err(e) => { bad.push(format!("{k}: {e:#}")); continue } };
            for (i, (st, _)) in x.streams.iter().enumerate() {
                if st.starts_with(b"NXS\x01CVXM") {
                    match crate::nxs::read_convex_hull(st) { Ok(_) => hulls += 1, Err(e) => bad.push(format!("{k} hull {i}: {e:#}")) }
                    continue;
                }
                let mut r = crate::nxs::Reader { d: st, at: 0 };
                match crate::nxs::read_trimesh(&mut r) {
                    Ok(m) if r.at == st.len() && crate::nxs::write_trimesh(&m) == *st => meshes += 1,
                    Ok(_) => bad.push(format!("{k} stream {i}: does not write back the same")),
                    // An older cooking (a few plain files): its triangles still read.
                    Err(_) if super::stream_triangles(st).is_ok_and(|t| !t.is_empty()) => old += 1,
                    Err(e) => bad.push(format!("{k} stream {i}: {e:#}")),
                }
            }
        }
        eprintln!("{files} files: {meshes} triangle meshes, {old} older ones, {hulls} hulls");
        assert!(bad.is_empty(), "{} unread, e.g. {:#?}", bad.len(), &bad[..bad.len().min(8)]);
        assert!(files > 2000 && hulls > 500, "{files} files, {hulls} hulls");
    }

    fn volume(ts: &[[[f32; 3]; 3]]) -> f64 {
        ts.iter().map(|[a, b, c]| { let (a, b, c) = (a.map(f64::from), b.map(f64::from), c.map(f64::from)); a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]) }).sum::<f64>() / 6.0
    }

    /// Hulls turn their triangles the way the triangle-mesh collisions do (so import and export treat both alike):
    /// an item with both encloses volume of the same sign in each.
    #[test]
    fn hulls_turn_the_way_collision_meshes_do() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let all = |k: &str| -> Vec<[[f32; 3]; 3]> { crate::g3_res::parse_xnvmsh(&g.read(k).unwrap()).unwrap().streams.iter().flat_map(|(s, _)| super::stream_triangles(s).unwrap()).collect() };
        let (mut same, mut other) = (0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with("_cv.xnvmsh")) {
            let Some(col) = g.find("_compiledphysic", &k.rsplit('/').next().unwrap().replace("_cv.xnvmsh", "_col.xnvmsh")) else { continue };
            let (h, c) = (volume(&all(&k)), volume(&all(&col)));
            if c.abs() < h.abs() * 0.05 { continue; }
            if (h > 0.0) == (c > 0.0) { same += 1 } else { other += 1 }
        }
        eprintln!("same {same}, other {other}");
        assert!(same > 20 && other * 10 <= same, "same {same}, other {other}");
    }
}
