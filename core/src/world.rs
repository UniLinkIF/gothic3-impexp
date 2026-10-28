//! The world as context: what the game places in one world layer (`projects_compiled/g3_world_01/.../*.node`,
//! `GENOMFLE`), and around it the static cells (`sysdyn_{…}/g3_world_01_x<X>y0z<Z>_cstat.node`, the town houses and
//! rocks) and the landscape. Read only — placing objects is a world editor's job; this lets a new model be made to
//! fit where it will stand.
//!
//! ```text
//! entity: u16 0x53 (version), u16 name (table index), 16-byte GUID, ... flags ...
//!         +0x2b  LOCAL matrix  (16 f32, row vectors: world = local · M, translation in [12..14], w = 1)
//!         +0x6b  WORLD matrix  (same layout)
//!         ... property sets follow; the mesh set writes
//!         [u16 "ResourceFileName"][u16 "bCMeshResourceString"][u16 30][u32 size][u16 ?][u16 <mesh>.xcmsh]
//! ```
//! An entity is recognised by the version and two matrices whose last column is (0, 0, 0, 1); its mesh is the first
//! mesh property before the next entity. Static cells place `.xlmsh` LoD sets (the full mesh first, then `_LOD1`…):
//! the full `.xcmsh` stands in for them. Distance copies (`*_lowpoly`) are left out.
use crate::g3::G3Ctx;
use crate::g3_res::genomfle_strings;
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement { pub mesh: String, pub world: [f32; 16] }

const VERSION: u16 = 0x53;
const LOCAL: usize = 0x2b;
const WORLD: usize = 0x6b;
const ROOT: &str = "projects_compiled/g3_world_01/";
/// Static cells are 100 m squares named by their centre (`x95000` holds 90000..100000); a cell is read when its
/// centre is within the radius plus this.
const CELL: f32 = 10000.0;

fn f(d: &[u8], at: usize) -> f32 { f32::from_le_bytes(d[at..at + 4].try_into().unwrap()) }
fn u16_(d: &[u8], at: usize) -> u16 { u16::from_le_bytes([d[at], d[at + 1]]) }
fn matrix(d: &[u8], at: usize) -> [f32; 16] { let mut m = [0f32; 16]; for (k, x) in m.iter_mut().enumerate() { *x = f(d, at + k * 4); } m }
fn affine(d: &[u8], m: usize) -> bool {
    (f(d, m + 60) - 1.0).abs() < 1e-4 && f(d, m + 12) == 0.0 && f(d, m + 28) == 0.0 && f(d, m + 44) == 0.0
        && (0..16).all(|k| f(d, m + k * 4).is_finite()) && (12..15).all(|k| f(d, m + k * 4).abs() < 1.0e7)
}

/// Every mesh placement of one `.node` world layer.
pub fn placements(d: &[u8]) -> Result<Vec<Placement>> {
    if !d.starts_with(b"GENOMFLE") { bail!("not a compiled (GENOMFLE) world layer: a stand-in of the editors, nothing the game places"); }
    let t = genomfle_strings(d)?;
    let end = (u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize).min(d.len());
    let idx = |s: &str| t.iter().position(|x| x == s).map(|i| i as u16);
    // The mesh property: `eCMesh_PS.ResourceFileName` (dynamic objects), `eCVisualMeshStatic_PS.ResourceFilePath`
    // (static cells, mostly `.xlmsh` LoD sets), `MeshFilePath` (vegetation).
    let names: Vec<u16> = ["ResourceFileName", "ResourceFilePath", "MeshFilePath"].iter().filter_map(|s| idx(s)).collect();
    if names.is_empty() || end < WORLD + 64 + 14 { return Ok(vec![]); }
    let mut ents = vec![];
    let mut s = 14;
    while s + WORLD + 64 <= end {
        if u16_(d, s) == VERSION && affine(d, s + WORLD) && affine(d, s + LOCAL) { ents.push(s); s += WORLD + 64; } else { s += 1; }
    }
    let mut meshes = vec![];
    for p in 14..end.saturating_sub(12) {
        if names.contains(&u16_(d, p)) && u16_(d, p + 4) == 30 {
            if let Some(m) = t.get(u16_(d, p + 10) as usize) {
                let l = m.to_lowercase();
                if l.ends_with(".xcmsh") || l.ends_with(".xlmsh") { meshes.push((p, m.clone())); }
            }
        }
    }
    let mut out = vec![];
    let mut mi = 0;
    for (k, &s) in ents.iter().enumerate() {
        let next = ents.get(k + 1).copied().unwrap_or(end);
        while mi < meshes.len() && meshes[mi].0 < s { mi += 1; }
        if mi < meshes.len() && meshes[mi].0 < next { out.push(Placement { mesh: meshes[mi].1.clone(), world: matrix(d, s + WORLD) }); }
    }
    Ok(out)
}

fn stem(key: &str) -> &str { key.rsplit('/').next().unwrap().trim_end_matches(".node") }

/// The world layers a modder can pick (by file stem): every `.node` but the static cells and the distance copies.
pub fn layers(g: &G3Ctx) -> Vec<String> {
    g.keys().into_iter().filter(|k| k.starts_with(ROOT) && k.ends_with(".node") && !k.contains("/sysdyn_") && !k.contains("lowpoly"))
        .filter_map(|k| g.path_of(&k).map(|p| stem(p).to_string())).collect()
}

/// A static cell's centre from its name (`…_x-105000y0z75000_cstat`).
fn cell_centre(key: &str) -> Option<[f32; 2]> {
    let s = stem(key);
    let at = s.rfind("_x")? + 2;
    let rest = &s[at..];
    let (x, rest) = rest.split_once('y')?;
    let (_, rest) = rest.split_once('z')?;
    let z = rest.split('_').next()?;
    Some([x.parse().ok()?, z.parse().ok()?])
}

/// The `.xcmsh` stem a `.xlmsh` LoD set shows at full detail (its first mesh string).
fn lod_full_mesh(g: &G3Ctx, xlmsh: &str) -> Option<String> {
    let b = g.read(&g.find("_compiledmesh", xlmsh)?).ok()?;
    crate::g3_res::inline_strings(&b).into_iter().find(|s| s.to_lowercase().ends_with(".xcmsh")).map(|s| s[..s.len() - 6].to_string())
}

#[derive(serde::Serialize, Debug)]
pub struct Item { pub mesh: String, pub source: String, pub world: [f32; 16] }

#[derive(serde::Serialize, Debug)]
pub struct Area {
    pub layer: String,
    /// The layer's middle (x, z, cm): the mean of its placements.
    pub center: [f32; 2],
    pub radius: f32,
    pub items: Vec<Item>,
    /// Landscape cells under the area (their vertices are in world space).
    pub landscape: Vec<String>,
    pub skipped: BTreeMap<String, usize>,
}

/// Layer `name` and, within `radius` cm of its middle, the static cells' placements and the landscape cells.
pub fn area(g: &G3Ctx, name: &str, radius: f32) -> Result<Area> {
    let keys = g.keys();
    let want = name.trim_end_matches(".node").to_lowercase();
    let key = keys.iter().find(|k| k.starts_with(ROOT) && k.ends_with(".node") && stem(k) == want).with_context(|| format!("{name}: no such world layer in Gothic 3"))?;
    let own = placements(&g.read(key)?).with_context(|| format!("read {key}"))?;
    let layer = stem(g.path_of(key).unwrap_or(key)).to_string();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut lods: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut resolve = |p: &Placement, skipped: &mut BTreeMap<String, usize>| -> Option<String> {
        let l = p.mesh.to_lowercase();
        if l.contains("_lowpoly") { *skipped.entry("distance copies (_lowpoly)".into()).or_default() += 1; return None; }
        let s = if l.ends_with(".xlmsh") {
            match lods.entry(l).or_insert_with(|| lod_full_mesh(g, &p.mesh)).clone() { Some(s) => s, None => { *skipped.entry(format!("LoD set without a mesh: {}", p.mesh)).or_default() += 1; return None } }
        } else { p.mesh[..p.mesh.len() - 6].to_string() };
        if crate::staticmesh::mesh_key(g, &s).is_none() { *skipped.entry(format!("not in the game: {s}")).or_default() += 1; return None; }
        Some(s)
    };
    let mut items = vec![];
    for p in &own { if let Some(m) = resolve(p, &mut skipped) { items.push(Item { mesh: m, source: layer.clone(), world: p.world }); } }
    let n = own.len().max(1) as f32;
    let center = if own.is_empty() { [0.0; 2] } else { [own.iter().map(|p| p.world[12]).sum::<f32>() / n, own.iter().map(|p| p.world[14]).sum::<f32>() / n] };
    let mut landscape = vec![];
    if radius > 0.0 {
        let reach = radius + CELL;
        for k in keys.iter().filter(|k| k.starts_with(ROOT) && k.contains("/sysdyn_") && k.ends_with("_cstat.node")) {
            let Some(c) = cell_centre(k) else { continue };
            let (dx, dz) = (c[0] - center[0], c[1] - center[1]);
            if dx * dx + dz * dz > reach * reach { continue; }
            let Ok(ps) = g.read(k).and_then(|b| placements(&b)) else { *skipped.entry("unreadable static cell".into()).or_default() += 1; continue };
            let src = stem(g.path_of(k).unwrap_or(k)).to_string();
            for p in ps {
                let (dx, dz) = (p.world[12] - center[0], p.world[14] - center[1]);
                if dx * dx + dz * dz > radius * radius { continue; }
                if let Some(m) = resolve(&p, &mut skipped) { items.push(Item { mesh: m, source: src.clone(), world: p.world }); }
            }
        }
        // Landscape cells (`*_landscape_cell_N`, vertices in world space) whose box comes within the radius.
        for k in keys.iter().filter(|k| k.starts_with("_compiledmesh/") && k.contains("landscape_cell") && !k.contains("lowpoly") && k.ends_with(".xcmsh")) {
            let Ok(m) = g.read(k).and_then(|b| crate::g3_res::decode_xcmsh(&b)) else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for p in &m.geo.positions { lo = [lo[0].min(p[0]), lo[1].min(p[2])]; hi = [hi[0].max(p[0]), hi[1].max(p[2])]; }
            let (cx, cz) = (center[0].clamp(lo[0], hi[0]), center[1].clamp(lo[1], hi[1]));
            if (cx - center[0]).powi(2) + (cz - center[1]).powi(2) <= radius * radius {
                landscape.push(stem(g.path_of(k).unwrap_or(k)).trim_end_matches(".xcmsh").to_string());
            }
        }
    }
    Ok(Area { layer, center, radius, items, landscape, skipped })
}

/// `area` plus every mesh it uses written as OBJ into `out_dir` (as the model import does): the add-on loads each
/// once and links it at each placement.
pub fn area_obj(g: &G3Ctx, name: &str, radius: f32, ground: bool, out_dir: &Path) -> Result<serde_json::Value> {
    let mut a = area(g, name, radius)?;
    if !ground { a.landscape.clear(); }
    let mut meshes = serde_json::Map::new();
    let mut warnings = vec![];
    let mut used: Vec<&str> = a.items.iter().map(|i| i.mesh.as_str()).chain(a.landscape.iter().map(String::as_str)).collect();
    used.sort_by_key(|s| s.to_lowercase());
    used.dedup_by_key(|s| s.to_lowercase());
    for m in used {
        match crate::staticmesh::export_obj(g, m, out_dir) {
            Ok(o) => { meshes.insert(m.to_lowercase(), serde_json::to_value(&o)?); }
            Err(e) => warnings.push(format!("{m}: {e:#}")),
        }
    }
    for (what, n) in &a.skipped { warnings.push(format!("left out {n}: {what}")); }
    let mut v = serde_json::to_value(&a)?;
    v["meshes"] = serde_json::Value::Object(meshes);
    v["warnings"] = serde_json::to_value(warnings)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn g3() -> Option<G3Ctx> { std::env::var_os("G3_GAME").map(|r| G3Ctx::open(Path::new(&r)).expect("G3_GAME is a Gothic 3 install")) }

    #[test]
    fn cell_names_give_their_centre() {
        assert_eq!(cell_centre("projects_compiled/g3_world_01/sysdyn_{x}/g3_world_01_x-105000y0z75000_cstat.node"), Some([-105000.0, 75000.0]));
        assert_eq!(cell_centre("g3_myrtana_01_ardea_npc_01_shyb.node"), None);
    }

    /// Ardea's dynamic objects (the anvil where the game has it), and around them the town's houses from the static
    /// cells, as LoD sets resolved to their full meshes, and the ground under it.
    #[test]
    fn ardea_with_its_houses_and_ground() {
        let Some(g) = g3() else { return };
        assert!(layers(&g).iter().any(|l| l == "G3_Myrtana_01_Ardea_Dynamic_Objects_01_shyb" || l.eq_ignore_ascii_case("g3_myrtana_01_ardea_dynamic_objects_01_shyb")));
        let own = area(&g, "g3_myrtana_01_ardea_dynamic_objects_01_shyb", 0.0).unwrap();
        assert!(own.items.len() > 90, "{}", own.items.len());
        let anvil = own.items.iter().find(|x| x.mesh.eq_ignore_ascii_case("G3_Object_Interact_Anvil_01")).expect("anvil");
        assert!((anvil.world[12] - 89775.4).abs() < 1.0 && (anvil.world[14] + 12669.7).abs() < 1.0, "{:?}", &anvil.world[12..15]);
        assert!(own.landscape.is_empty() && own.items.iter().all(|i| !i.source.to_lowercase().ends_with("_cstat")));
        let a = area(&g, "g3_myrtana_01_ardea_dynamic_objects_01_shyb", 5000.0).unwrap();
        let statics = a.items.iter().filter(|i| i.source.to_lowercase().ends_with("_cstat")).count();
        eprintln!("ardea: centre {:?}, {} own, {statics} static, landscape {:?}, skipped {:?}", a.center, own.items.len(), a.landscape, a.skipped);
        assert!(statics > 20 && !a.landscape.is_empty());
        for i in &a.items { assert!(i.world.iter().all(|x| x.is_finite()) && crate::staticmesh::mesh_key(&g, &i.mesh).is_some(), "{i:?}"); }
    }

    /// Long thin pieces placed in rows (palisades, wooden walls, fences): the mesh's long axis, turned by the matrix
    /// read with row vectors (local X -> row 0), points at the next piece of the row. Read transposed it does not.
    #[test]
    fn wall_pieces_line_up_along_their_long_axis_with_row_vectors() {
        let Some(g) = g3() else { return };
        let mut by_mesh: BTreeMap<String, Vec<[f32; 16]>> = Default::default();
        for k in g.keys().into_iter().filter(|k| k.starts_with(ROOT) && k.ends_with(".node")) {
            for p in placements(&g.read(&k).unwrap()).unwrap_or_default() { by_mesh.entry(p.mesh.to_lowercase()).or_default().push(p.world); }
        }
        let (mut row, mut col, mut n) = (0f32, 0f32, 0usize);
        for (mesh, ws) in &by_mesh {
            if !["palisad", "wood_wall", "fence"].iter().any(|k| mesh.contains(k)) || ws.len() < 4 || !mesh.ends_with(".xcmsh") { continue; }
            let Some(key) = crate::staticmesh::mesh_key(&g, mesh) else { continue };
            let Ok(m) = crate::g3_res::decode_xcmsh(&g.read(&key).unwrap()) else { continue };
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &m.geo.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
            let (ex, ez) = (hi[0] - lo[0], hi[2] - lo[2]);
            let long = if ex > 2.5 * ez { 0 } else if ez > 2.5 * ex { 2 } else { continue };
            let span = ex.max(ez);
            for (i, w) in ws.iter().enumerate() {
                let near = ws.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, o)| [o[12] - w[12], o[13] - w[13], o[14] - w[14]])
                    .filter(|d| { let l = d[0].hypot(d[2]); l > 0.5 * span && l < 1.3 * span })
                    .min_by(|a, b| a[0].hypot(a[2]).partial_cmp(&b[0].hypot(b[2])).unwrap());
                let Some(d) = near else { continue };
                let l = d[0].hypot(d[2]);
                let r = [w[long * 4], w[long * 4 + 2]];
                let c = [w[long], w[8 + long]];
                let cos = |a: [f32; 2]| ((a[0] * d[0] + a[1] * d[2]) / (a[0].hypot(a[1]) * l)).abs();
                row += cos(r); col += cos(c); n += 1;
            }
        }
        eprintln!("wall pairs {n}: mean |cos| row-vector {:.3}, transposed {:.3}", row / n as f32, col / n as f32);
        assert!(n > 50 && row / n as f32 > 0.8 && row > col);
    }

    #[test]
    fn every_layer_reads() {
        let Some(g) = g3() else { return };
        let (mut n, mut files, mut other) = (0, 0, vec![]);
        for k in g.keys().into_iter().filter(|k| k.starts_with(ROOT) && k.ends_with(".node")) {
            let b = g.read(&k).unwrap();
            // A few layers are kept in another (plain archive) format: level-mesh stand-ins (`*_spg`, their meshes are
            // dummies the game does not ship) and the developers' tests.
            if !b.starts_with(b"GENOMFLE") { other.push(k); continue; }
            let v = placements(&b).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            if !v.is_empty() { files += 1; }
            n += v.len();
        }
        eprintln!("world: {n} placements in {files} layers; not GENOMFLE: {other:?}");
        assert!(n > 10_000 && other.len() < 20);
    }
}
