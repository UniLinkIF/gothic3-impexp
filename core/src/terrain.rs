//! The landscape as an editable object: its cells (`_compiledMesh/G3_<Land>_Landscape_01/LOD/…_Cell_<n>.xcmsh`,
//! vertices in world space; 303 in Myrtana, 119 in Nordmar, 360 in Varant) out to Blender with everything they carry,
//! and the edited ones back.
//!
//! A cell carries, per vertex: position, normal, uv, a diffuse colour (its alpha blends the ground layers), a specular
//! colour (its alpha is the baked brightness), in Varant three more uv sets (streams 15, 21, 18); per element a game
//! material; beside it its collision (`<cell>.xnvmsh`, a part per element) and one lightmap. All of it goes out, and
//! an edited cell is written again by the model export (`export::build`): collision from its own triangles, the
//! lightmap rebuilt for its vertices. A cell that comes back as it went out is not written at all, so the game's
//! file stays. The distant view of the land (`G3_World_Lowpoly_Landscape_01`) is a mesh of its own and stays as it is.
//!
//! ```text
//! <cell>.g3terrain  u32 vertices · u32 triangles · u32 further uv sets n · u32 stream[n]
//!                   · f32 position[3·v] · f32 normal[3·v] · f32 uv[2·v] · (f32 uv[2·v]) × n · u32 diffuse[v]
//!                   · u32 specular[v] · u32 triangle[3·t] (the game's order) · u32 material[t] (into the materials)
//! ```

use crate::export::{self, CornerData, MatSpec, Spec};
use crate::g3::G3Ctx;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// One cell: its name (file stem as the game writes it), its key, its box over the ground (x, z min / max, cm).
#[derive(serde::Serialize, Debug, Clone)]
pub struct Cell { pub name: String, #[serde(skip)] pub key: String, pub land: String, pub area: [f32; 4] }

fn is_cell(key: &str) -> bool { key.starts_with("_compiledmesh/") && key.contains("/lod/") && key.contains("_landscape_cell_") && key.ends_with(".xcmsh") }

/// Every landscape cell of the game, with its box.
pub fn cells(g: &G3Ctx) -> Result<Vec<Cell>> {
    let mut v = vec![];
    for k in g.keys().into_iter().filter(|k| is_cell(k)) {
        let (lo, hi) = crate::g3_res::xcmsh_box(&g.read(&k)?).with_context(|| k.clone())?;
        let name = crate::stem_of(g.path_of(&k).unwrap_or(&k));
        let land = k.split('/').nth(1).unwrap_or("").trim_start_matches("g3_").split('_').next().unwrap_or("").to_string();
        v.push(Cell { name, key: k, land, area: [lo[0], lo[2], hi[0], hi[2]] });
    }
    Ok(v)
}

/// The cells `query` names: cell names (any case, separated by spaces or commas; a name's end such as
/// `myrtana_landscape_cell_244` is enough when only one cell has it), or `@x,z,r` for every cell whose box comes
/// within r cm of the point (x, z) on the ground.
pub fn select(all: &[Cell], query: &str) -> Result<Vec<Cell>> {
    if let Some(at) = query.trim().strip_prefix('@') {
        let v: Vec<f32> = at.split(',').map(|x| x.trim().parse::<f32>()).collect::<std::result::Result<_, _>>().context("@x,z,radius in cm")?;
        let [x, z, r] = v[..] else { bail!("@x,z,radius in cm") };
        let out: Vec<Cell> = all.iter().filter(|c| { let (cx, cz) = (x.clamp(c.area[0], c.area[2]), z.clamp(c.area[1], c.area[3])); (cx - x).powi(2) + (cz - z).powi(2) <= r * r }).cloned().collect();
        if out.is_empty() { bail!("no landscape cell within {r} cm of ({x}, {z})"); }
        return Ok(out);
    }
    let mut out: Vec<Cell> = vec![];
    for w in query.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()) {
        let w = w.trim_end_matches(".xcmsh").to_lowercase();
        let hits: Vec<&Cell> = match all.iter().find(|c| c.name.to_lowercase() == w) { Some(c) => vec![c], None => all.iter().filter(|c| c.name.to_lowercase().ends_with(&w)).collect() };
        match hits[..] {
            [c] => if !out.iter().any(|o| o.key == c.key) { out.push(c.clone()) },
            [] => bail!("{w}: no such landscape cell"),
            _ => bail!("{w}: {} cells end so ({}), name the land too", hits.len(), hits.iter().map(|c| c.name.as_str()).take(4).collect::<Vec<_>>().join(", ")),
        }
    }
    if out.is_empty() { bail!("no landscape cell named"); }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct CellOut { pub name: String, pub entry: String, pub file: String, pub vertices: usize, pub triangles: usize, pub uv_streams: Vec<u32> }

fn put_f(o: &mut Vec<u8>, x: f32) { o.extend(x.to_le_bytes()); }
fn put_u(o: &mut Vec<u8>, x: u32) { o.extend(x.to_le_bytes()); }

/// The cells as `.g3terrain` files and their materials (PNGs) in `out_dir`.
pub fn export_cells(g: &G3Ctx, cells: &[Cell], out_dir: &Path) -> Result<serde_json::Value> {
    std::fs::create_dir_all(out_dir)?;
    let mut warnings = vec![];
    let mut materials: Vec<crate::staticmesh::Material> = vec![];
    let mut outs = vec![];
    for c in cells {
        let m = crate::g3_res::decode_xcmsh(&g.read(&c.key)?).with_context(|| c.key.clone())?;
        let geo = &m.geo;
        let nv = geo.positions.len();
        if geo.normals.len() != nv || geo.uvs.len() != nv { bail!("{}: a landscape cell without normals or uvs", c.name); }
        let mat_ids: Vec<u32> = geo.submeshes.iter().map(|s| {
            let base = s.material.trim_end_matches(".xshmat");
            match materials.iter().position(|x| x.name.eq_ignore_ascii_case(base)) {
                Some(i) => i as u32,
                None => { materials.push(crate::staticmesh::material(g, &s.material, out_dir, &mut warnings)); materials.len() as u32 - 1 }
            }
        }).collect();
        let mut b = vec![];
        put_u(&mut b, nv as u32);
        put_u(&mut b, (geo.indices.len() / 3) as u32);
        put_u(&mut b, m.extra_uvs.len() as u32);
        for (s, _) in &m.extra_uvs { put_u(&mut b, *s); }
        for p in geo.positions.iter().chain(&geo.normals) { for x in p { put_f(&mut b, *x); } }
        for t in geo.uvs.iter().chain(m.extra_uvs.iter().flat_map(|e| e.1.iter())) { put_f(&mut b, t[0]); put_f(&mut b, t[1]); }
        for x in m.diffuse.iter().chain(&m.specular) { put_u(&mut b, *x); }
        for i in &geo.indices { put_u(&mut b, *i); }
        for (e, s) in geo.submeshes.iter().enumerate() { for _ in 0..s.index_count / 3 { put_u(&mut b, mat_ids[e]); } }
        let file = out_dir.join(format!("{}.g3terrain", c.name));
        std::fs::write(&file, b)?;
        outs.push(CellOut { name: c.name.clone(), entry: c.key.clone(), file: file.to_string_lossy().into_owned(), vertices: nv, triangles: geo.indices.len() / 3, uv_streams: m.extra_uvs.iter().map(|e| e.0).collect() });
    }
    Ok(serde_json::json!({ "cells": outs, "materials": materials, "warnings": warnings }))
}

/// One cell back from Blender: a model spec (geometry, materials, corner colours and uv sets).
#[derive(serde::Deserialize)]
pub struct CellSpec { pub name: String, pub geometry: String, pub corners: usize, pub materials: Vec<MatSpec>, pub corner_data: Option<CornerData> }

#[derive(serde::Deserialize)]
pub struct TerrainSpec { pub cells: Vec<CellSpec>, #[serde(default)] pub collision: Option<String> }

#[derive(serde::Serialize, Default)]
pub struct TerrainReport { pub changed: Vec<String>, pub unchanged: Vec<String>, pub files: Vec<String>, pub lightmaps: usize, pub warnings: Vec<String> }

/// Corners of a cell as (material, position cm, uv, diffuse, specular, further uvs) per triangle corner.
type Corner = (String, [f32; 3], [f32; 2], u32, u32, Vec<[f32; 2]>);

fn game_corners(m: &crate::g3_res::G3Mesh) -> Vec<Corner> {
    let geo = &m.geo;
    let mut v = vec![];
    for s in &geo.submeshes {
        let mat = export::clean(s.material.trim_end_matches(".xshmat")).to_lowercase();
        for &i in &geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize] {
            let i = i as usize;
            v.push((mat.clone(), geo.positions[i], geo.uvs[i], m.diffuse[i], m.specular[i], m.extra_uvs.iter().map(|e| e.1[i]).collect()));
        }
    }
    v
}

fn spec_corners(c: &CellSpec) -> Result<Vec<Corner>> {
    let d = std::fs::read(&c.geometry).with_context(|| format!("read {}", c.geometry))?;
    let n = c.corners;
    if d.len() != n * 32 + n / 3 * 4 { bail!("{}: geometry is {} bytes for {n} corners", c.name, d.len()); }
    let f = |i: usize| f32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
    let (cols, uvs) = match &c.corner_data { Some(cd) => (cd.colors, cd.uv_streams.len()), None => (false, 0) };
    let e = match &c.corner_data { Some(cd) => std::fs::read(&cd.file).with_context(|| format!("read {}", cd.file))?, None => vec![] };
    if e.len() != if cols { n * 8 } else { 0 } + uvs * n * 8 { bail!("{}: corner data is {} bytes", c.name, e.len()); }
    let u = |i: usize| u32::from_le_bytes(e[i * 4..i * 4 + 4].try_into().unwrap());
    let base = if cols { n * 2 } else { 0 };
    Ok((0..n).map(|k| {
        let mi = u32::from_le_bytes(d[(n * 8 + k / 3) * 4..(n * 8 + k / 3) * 4 + 4].try_into().unwrap()) as usize;
        let mat = c.materials.get(mi).map(|m| export::clean(&m.name).to_lowercase()).unwrap_or_default();
        let (dif, spe) = if cols { (u(k), u(n + k)) } else { (0, 0) };
        let more = (0..uvs).map(|s| [f32::from_bits(u(base + s * n * 2 + k * 2)), f32::from_bits(u(base + s * n * 2 + k * 2 + 1))]).collect();
        (mat, [f(k * 3), f(k * 3 + 1), f(k * 3 + 2)], [f(n * 6 + k * 2), f(n * 6 + k * 2 + 1)], dif, spe, more)
    }).collect())
}

/// True when the corners from Blender are the game's cell as it ships: the same triangles in the same order (each may
/// start at another corner), positions within 0.05 cm (metres in Blender round them), the same uvs, colours, materials.
fn same_cell(game: &[Corner], ours: &[Corner], colors: bool) -> bool {
    if game.len() != ours.len() { return false; }
    let same = |a: &Corner, b: &Corner| a.0 == b.0 && (0..3).all(|k| (a.1[k] - b.1[k]).abs() < 0.05) && (0..2).all(|k| (a.2[k] - b.2[k]).abs() < 1e-4)
        && (!colors || (a.3 == b.3 && a.4 == b.4)) && (b.5.is_empty() || a.5 == b.5);
    game.chunks_exact(3).zip(ours.chunks_exact(3)).all(|(g, o)| (0..3).any(|r| (0..3).all(|k| same(&g[k], &o[(k + r) % 3]))))
}

/// The edited cells written again; cells that come back unchanged are left out. Errors when nothing changed.
pub fn build(g: &G3Ctx, spec: &TerrainSpec) -> Result<(TerrainReport, Vec<(String, String, Vec<u8>)>)> {
    let mut r = TerrainReport::default();
    let mut files = vec![];
    for c in &spec.cells {
        let key = crate::staticmesh::mesh_key(g, &c.name).filter(|k| is_cell(k)).with_context(|| format!("{}: not a landscape cell of the game", c.name))?;
        let stock = crate::g3_res::decode_xcmsh(&g.read(&key)?).with_context(|| key.clone())?;
        if same_cell(&game_corners(&stock), &spec_corners(c)?, c.corner_data.as_ref().is_some_and(|d| d.colors)) { r.unchanged.push(c.name.clone()); continue; }
        let s = Spec { name: c.name.clone(), geometry: c.geometry.clone(), corners: c.corners, materials: c.materials.iter().map(|m| MatSpec { name: m.name.clone(), diffuse: m.diffuse.clone(), normal: m.normal.clone(), specular: m.specular.clone(), alpha_test: m.alpha_test, shape: m.shape.clone() }).collect(),
            collision: Some(export::CollisionSpec { geometry: None, corners: None, material: Some("earth".into()), mode: Some(spec.collision.clone().unwrap_or_else(|| "auto".into())), materials: vec![], convex: vec![] }), corner_data: c.corner_data.clone() };
        let (rep, f) = export::build(g, &s).with_context(|| c.name.clone())?;
        if !rep.replaced { bail!("{}: the cell was not found to replace", c.name); }
        r.lightmaps += rep.lightmaps;
        r.warnings.extend(rep.warnings.into_iter().map(|w| format!("{}: {w}", c.name)));
        r.changed.push(c.name.clone());
        files.extend(f);
    }
    if r.changed.is_empty() { bail!("no cell has changed ({} as the game has them) — nothing to export", r.unchanged.len()); }
    r.files = files.iter().map(|(a, p, _)| format!("{a}/{p}")).collect();
    Ok((r, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn g3() -> Option<G3Ctx> { std::env::var_os("G3_GAME").map(|r| G3Ctx::open_original(Path::new(&r)).expect("G3_GAME is a Gothic 3 install")) }

    /// Every landscape cell of the game written again by our mesh writer reads back with everything it carries: the
    /// streams in the game's order, each element's material, positions, normals, every uv set, both colours and the
    /// triangles, all exactly.
    #[test]
    fn every_landscape_cell_writes_back_with_all_it_carries() {
        let Some(g) = g3() else { return };
        let all = cells(&g).unwrap();
        assert_eq!(all.len(), 782);
        let (mut four, mut n) = (0, 0);
        for c in &all {
            let d = g.read(&c.key).unwrap();
            let m = crate::g3_res::decode_xcmsh(&d).unwrap();
            let geo = &m.geo;
            let parts: Vec<crate::xcmsh_write::Part> = geo.submeshes.iter().enumerate().map(|(e, s)| {
                let (first, nv) = m.elements[e];
                let r = first as usize..(first + nv) as usize;
                crate::xcmsh_write::Part { material: s.material.clone(), positions: geo.positions[r.clone()].to_vec(), normals: geo.normals[r.clone()].to_vec(), uvs: geo.uvs[r.clone()].to_vec(),
                    triangles: geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize].chunks_exact(3).map(|t| [t[0] - first, t[1] - first, t[2] - first]).collect(),
                    diffuse: m.diffuse[r.clone()].to_vec(), specular: m.specular[r.clone()].to_vec(), extra_uvs: m.extra_uvs.iter().map(|(s, v)| (*s, v[r.clone()].to_vec())).collect() }
            }).collect();
            let b = crate::g3_res::decode_xcmsh(&crate::xcmsh_write::write(&parts)).unwrap_or_else(|e| panic!("{}: {e:#}", c.name));
            assert_eq!(b.streams, m.streams, "{}", c.name);
            assert_eq!(b.elements, m.elements, "{}", c.name);
            let mats = |m: &crate::g3_res::G3Mesh| m.geo.submeshes.iter().map(|s| (s.material.clone(), s.first_index, s.index_count)).collect::<Vec<_>>();
            assert_eq!(mats(&b), mats(&m), "{}", c.name);
            let bits3 = |v: &[[f32; 3]]| v.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
            let bits2 = |v: &[[f32; 2]]| v.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
            assert!(bits3(&b.geo.positions) == bits3(&geo.positions) && bits3(&b.geo.normals) == bits3(&geo.normals) && bits2(&b.geo.uvs) == bits2(&geo.uvs), "{}", c.name);
            assert!(b.geo.indices == geo.indices && b.diffuse == m.diffuse && b.specular == m.specular, "{}", c.name);
            assert_eq!(b.extra_uvs.iter().map(|(s, v)| (*s, bits2(v))).collect::<Vec<_>>(), m.extra_uvs.iter().map(|(s, v)| (*s, bits2(v))).collect::<Vec<_>>(), "{}", c.name);
            // The box the writer puts in is the cell's own.
            let (lo, hi) = crate::g3_res::xcmsh_box(&crate::xcmsh_write::write(&parts)).unwrap();
            let (glo, ghi) = crate::g3_res::xcmsh_box(&d).unwrap();
            assert!((0..3).all(|k| (lo[k] - glo[k]).abs() < 0.5 && (hi[k] - ghi[k]).abs() < 0.5), "{}: box {lo:?} {hi:?} vs {glo:?} {ghi:?}", c.name);
            if m.extra_uvs.len() == 3 { four += 1; }
            n += 1;
        }
        eprintln!("{n} cells written back, {four} with four uv sets");
        assert_eq!(four, 360);
    }

    #[test]
    fn cells_are_found_by_name_and_place() {
        let Some(g) = g3() else { return };
        let all = cells(&g).unwrap();
        let c = select(&all, "myrtana_landscape_cell_244").unwrap();
        assert_eq!(c.len(), 1);
        assert!(select(&all, "cell_244").is_err(), "two lands have a cell 244");
        // The point in the middle of cell 244's box finds it, a radius reaching past it finds its neighbours too.
        let a = c[0].area;
        let (x, z) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        let one = select(&all, &format!("@{x},{z},1")).unwrap();
        assert!(one.iter().any(|o| o.key == c[0].key), "{one:?}");
        let more = select(&all, &format!("@{x},{z},{}", (a[2] - a[0]).max(a[3] - a[1]))).unwrap();
        assert!(more.len() > one.len(), "{} vs {}", more.len(), one.len());
    }

    /// The add-on's files for cells as they came out: geometry (game space, corners in the game's order, V down) and
    /// corner data. `edit` may change a cell's corner positions.
    pub(crate) fn spec_from_files(out: &serde_json::Value, dir: &Path, edit: &dyn Fn(&str, &mut Vec<[f32; 3]>)) -> TerrainSpec {
        let mats: Vec<String> = out["materials"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
        let cells = out["cells"].as_array().unwrap().iter().map(|c| {
            let name = c["name"].as_str().unwrap().to_string();
            let d = std::fs::read(c["file"].as_str().unwrap()).unwrap();
            let u = |i: usize| u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
            let (nv, nt, ne) = (u(0) as usize, u(1) as usize, u(2) as usize);
            let at = 3 + ne;
            let f = |i: usize| f32::from_bits(u(i));
            let p = |i: usize| [f(at + i * 3), f(at + i * 3 + 1), f(at + i * 3 + 2)];
            let nrm = |i: usize| [f(at + nv * 3 + i * 3), f(at + nv * 3 + i * 3 + 1), f(at + nv * 3 + i * 3 + 2)];
            let uv_at = at + nv * 6;
            let col_at = uv_at + nv * 2 * (1 + ne);
            let tri_at = col_at + nv * 2;
            let idx: Vec<usize> = (0..nt * 3).map(|k| u(tri_at + k) as usize).collect();
            let mut pos: Vec<[f32; 3]> = idx.iter().map(|&i| p(i)).collect();
            edit(&name, &mut pos);
            let mut geo = vec![];
            for x in pos.iter().flatten() { geo.extend(x.to_le_bytes()); }
            for &i in &idx { for x in nrm(i) { geo.extend(x.to_le_bytes()); } }
            for &i in &idx { geo.extend(f(uv_at + i * 2).to_le_bytes()); geo.extend(f(uv_at + i * 2 + 1).to_le_bytes()); }
            let used: Vec<u32> = (0..nt).map(|t| u(tri_at + nt * 3 + t)).collect();
            for t in &used { geo.extend(t.to_le_bytes()); }
            let mut cd = vec![];
            for &i in &idx { cd.extend(u(col_at + i).to_le_bytes()); }
            for &i in &idx { cd.extend(u(col_at + nv + i).to_le_bytes()); }
            for s in 0..ne { for &i in &idx { cd.extend(u(uv_at + nv * 2 * (1 + s) + i * 2).to_le_bytes()); cd.extend(u(uv_at + nv * 2 * (1 + s) + i * 2 + 1).to_le_bytes()); } }
            let gp = dir.join(format!("{name}.geo"));
            let cp = dir.join(format!("{name}.corners"));
            std::fs::write(&gp, geo).unwrap();
            std::fs::write(&cp, cd).unwrap();
            CellSpec { name, geometry: gp.to_string_lossy().into_owned(), corners: nt * 3,
                materials: mats.iter().map(|m| MatSpec { name: m.clone(), diffuse: None, normal: None, specular: None, alpha_test: None, shape: None }).collect(),
                corner_data: Some(CornerData { file: cp.to_string_lossy().into_owned(), colors: true, uv_streams: (0..ne).map(|s| u(3 + s)).collect() }) }
        }).collect();
        TerrainSpec { cells, collision: None }
    }

    /// Two neighbouring cells out and back: unchanged, nothing is written; one raised, only it is written (mesh,
    /// collision beside it, its lightmap), with the game's colours and uv sets, and its ground 2 m higher.
    #[test]
    fn only_the_edited_cell_comes_back() {
        let Some(g) = g3() else { return };
        let dir = std::env::temp_dir().join(format!("g3ie_test_terrain_{}", std::process::id()));
        let all = cells(&g).unwrap();
        for query in ["myrtana_landscape_cell_244 myrtana_landscape_cell_245", "varant_landscape_cell_244"] {
            let sel = select(&all, query).unwrap();
            let out = export_cells(&g, &sel, &dir).unwrap();
            let same = spec_from_files(&out, &dir, &|_, _| {});
            let e = build(&g, &same).err().expect("nothing changed");
            assert!(format!("{e:#}").contains("nothing to export"), "{e:#}");
            let first = sel[0].name.clone();
            let raised = spec_from_files(&out, &dir, &|n, pos| if n == first { for p in pos.iter_mut() { p[1] += 200.0; } });
            let (r, files) = build(&g, &raised).unwrap();
            assert_eq!(r.changed, vec![first.clone()], "{query}");
            assert_eq!(r.unchanged.len(), sel.len() - 1);
            let mesh = files.iter().find(|f| f.1.to_lowercase().ends_with(&format!("{}.xcmsh", first.to_lowercase()))).expect("the cell's mesh");
            assert!(files.iter().any(|f| f.1.to_lowercase().ends_with(&format!("{}.xnvmsh", first.to_lowercase())) && f.0 == "_compiledMesh"), "{:?}", r.files);
            assert_eq!(r.lightmaps, 1, "{:?}", r.files);
            assert!(files.iter().all(|f| f.1.to_lowercase().contains(&first.to_lowercase())), "{:?}", r.files);
            let ours = crate::g3_res::decode_xcmsh(&mesh.2).unwrap();
            let game = crate::g3_res::decode_xcmsh(&g.read(&sel[0].key).unwrap()).unwrap();
            assert_eq!(ours.streams, game.streams, "{query}: streams");
            let up = |m: &crate::g3_res::G3Mesh| m.geo.indices.iter().map(|&i| m.geo.positions[i as usize][1] as f64).sum::<f64>() / m.geo.indices.len() as f64;
            assert!((up(&ours) - up(&game) - 200.0).abs() < 0.5, "{} vs {}", up(&ours), up(&game));
            // Colours and further uv sets as the game's, corner by corner (in the game's order).
            let cs = |m: &crate::g3_res::G3Mesh| m.geo.indices.iter().map(|&i| (m.diffuse[i as usize], m.specular[i as usize], m.extra_uvs.iter().map(|e| e.1[i as usize].map(f32::to_bits)).collect::<Vec<_>>())).collect::<Vec<_>>();
            assert!(cs(&ours) == cs(&game), "{query}: colours / uv sets");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
