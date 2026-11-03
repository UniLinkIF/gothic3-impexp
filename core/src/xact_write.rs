//! A new skinned mesh on a Gothic 3 actor's skeleton: the actor (`.xact`) is the template; its nodes and everything
//! else stay, its mesh, skin and materials become ours.
//!
//! ```text
//! 0   "GENOMFLE" · u16 1 · u32 string-table offset · u16 54 · … · FILETIME at 24 (compiled) · u32 source bytes at 32
//!     (the size of what it was compiled from: the FXA itself in every actor compiled since August 2006)
//!     · … "gena" · u16 · u32 FXA bytes (at 74) · "FXA " u8 1 u8 1 (at 78) · sections · mesh extra data · table
//! section  u32 id · u32 size · u32 version · body
//!   3 v3  mesh: u32 node · u32 vertices · u32 corners · u32 indices · u32 parts · u32 uv channels (1) · u32 (kept)
//!         part: u32 (low byte = material, the rest kept) · u32 indices · u32 corners
//!               · corner: u32 vertex · f32 position[3] · f32 normal[3] · f32 uv[2] · u32 index[indices]
//!   4 v1  skin: u32 node · per vertex: u8 n · n × (u16 node, u16 0x170C, f32 weight)
//!   6 v5  material: f32[16] colours and factors · u16 0 · u16 70 · u32 length · name · f32 0
//!   7 v4  texture map: u8 type (1 diffuse, 3 normal, 9 specular) · u8 0xF7 · u16 material · f32[6] · u32 length · name
//! extra   material map (u16 0, FXA material, string: `<name>.xshmat`) · u8 · meshes × (u8 · u32 corners · colours
//!         · tangents) — the whole actor is read and written by [`crate::xact_doc`]
//! ```
//! Laid out as every actor of the game (498 of 498 blocks with materials): `EMFX Default` first, the materials of the
//! mesh's parts and maps numbered from 1, and the map after the FXA pointing each at `<name before '|'>.xshmat` in the
//! string table. An actor that carries its levels of detail gets the new mesh in each.
//! Triangles: Gothic 3's actors keep the opposite order of its static meshes, so corners 1 and 2 swap.
//! Every actor of the game has one mesh (374 of 374; the four others are bare skeletons), and several Blender objects
//! come in as that one mesh with a material each.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// The mesh from Blender, in game space: positions per vertex, corners (vertex, normal, uv), a material per
/// triangle, (bone name, weight) per vertex (Gothic 3 keeps up to 13; the format allows 255).
pub struct SkinInput {
    pub positions: Vec<[f32; 3]>,
    pub corner_vertex: Vec<u32>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tri_material: Vec<u32>,
    pub weights: Vec<Vec<(String, f32)>>,
}

/// A material: a name and, for a new one, its image names (diffuse, normal, specular); `keep` = the template's own
/// sections of that name.
pub struct NewMaterial { pub name: String, pub diffuse: Option<String>, pub normal: Option<String>, pub specular: Option<String> }

#[cfg(test)]
fn u32_at(d: &[u8], p: usize) -> u32 { u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) }

/// `template` with `input` as its mesh, laid out as every actor of the game: in each block (the actor and the levels of
/// detail it carries) the default material first and ours numbered from 1 (a material the template has keeps its own
/// sections), the mesh and skin ours, the map after the FXA naming each material's `<name>.xshmat` in the string table;
/// nodes, limits and shapes stay. Returns the file and notes.
pub fn replace_mesh(template: &[u8], input: &SkinInput, materials: &[NewMaterial]) -> Result<(Vec<u8>, Vec<String>)> {
    use crate::xact_doc::{CornerData, Mesh, Part, Section, Skin};
    let mut doc = crate::xact_doc::read(template).context("the base actor")?;
    let nc = input.corner_vertex.len();
    if nc % 3 != 0 || input.normals.len() != nc || input.uvs.len() != nc || input.tri_material.len() != nc / 3 { bail!("corner arrays disagree"); }
    let mut notes = vec![];
    // Corners per material: one per (vertex, normal, uv); Gothic 3 order (corners 1 and 2 swapped).
    let nmat = materials.len().max(1);
    let (mut part_corners, mut part_tris): (Vec<Vec<(u32, [f32; 3], [f32; 3], [f32; 2])>>, Vec<Vec<[u32; 3]>>) = (vec![vec![]; nmat], vec![vec![]; nmat]);
    let mut seen: Vec<HashMap<(u32, [u32; 5]), u32>> = vec![HashMap::new(); nmat];
    for t in 0..nc / 3 {
        let mi = (input.tri_material[t] as usize).min(nmat - 1);
        let ids = [0, 2, 1].map(|k| {
            let c = t * 3 + k;
            let (v, n, uv) = (input.corner_vertex[c], input.normals[c], input.uvs[c]);
            *seen[mi].entry((v, [n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), uv[0].to_bits(), uv[1].to_bits()])).or_insert_with(|| { part_corners[mi].push((v, input.positions[v as usize], n, uv)); part_corners[mi].len() as u32 - 1 })
        });
        part_tris[mi].push(ids);
    }
    let (mut all_pos, mut all_nrm, mut all_uv, mut all_tri) = (vec![], vec![], vec![], vec![]);
    for mi in 0..nmat {
        let base = all_pos.len() as u32;
        for (_, p, n, uv) in &part_corners[mi] { all_pos.push(*p); all_nrm.push(*n); all_uv.push(*uv); }
        for t in &part_tris[mi] { all_tri.push(t.map(|i| i + base)); }
    }
    let tangents: Vec<[u32; 3]> = crate::xcmsh_write::tangents(&all_pos, &all_nrm, &all_uv, &all_tri).iter().map(|t| { let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-8); [t[0] / l, t[1] / l, t[2] / l].map(f32::to_bits) }).collect();
    let fxa_name = |s: &str| s.split('|').next().unwrap_or(s).to_string();
    // The FXA number of each material: the default one is 0, the others count from 1 (and have a shader material).
    let is_default = |m: &NewMaterial| m.name == "EMFX Default";
    let named: Vec<&NewMaterial> = materials.iter().filter(|m| !is_default(m)).collect();
    let fxa_of: Vec<u32> = { let mut k = 0; materials.iter().map(|m| if is_default(m) { 0 } else { k += 1; k }).collect() };
    let table: Vec<Vec<u8>> = named.iter().map(|m| format!("{}.xshmat", fxa_name(&m.name)).into_bytes()).collect();
    let nblocks = doc.blocks.len();
    for (bi, block) in doc.blocks.iter_mut().enumerate() {
        let nodes: Vec<String> = block.sections.iter().filter_map(|s| if let Section::Node { node, .. } = s { Some(String::from_utf8_lossy(&node.name).to_lowercase()) } else { None }).collect();
        let Some((mesh_node, word, cookie)) = block.sections.iter().find_map(|s| if let Section::Mesh { mesh, .. } = s { Some((mesh.node, mesh.word, mesh.parts.first().map(|p| p.head & 0xFFFF_FF00).unwrap_or(0))) } else { None }) else {
            if bi == 0 { bail!("the base actor has no mesh (a bare skeleton): only actors with a mesh can be replaced"); }
            continue;
        };
        if block.sections.iter().filter(|s| matches!(s, Section::Mesh { .. })).count() != 1 { bail!("the base actor has several meshes; only one-mesh actors (human bodies, heads, creatures) can be replaced"); }
        let weight_cookie = block.sections.iter().find_map(|s| if let Section::Skin { skin, .. } = s { skin.weights.iter().flatten().next().map(|w| w.1) } else { None }).unwrap_or(0x170C);
        // The template's own sections of each material it keeps, by name.
        let mut own: HashMap<String, Vec<Section>> = HashMap::new();
        let mut current: Option<String> = None;
        let mut default: Option<Section> = None;
        for s in &block.sections {
            match s {
                Section::Material { name, .. } => { let n = String::from_utf8_lossy(name).to_string(); if n == "EMFX Default" && default.is_none() { default = Some(s.clone()); current = None; } else { own.insert(n.to_lowercase(), vec![s.clone()]); current = Some(n.to_lowercase()); } }
                Section::Map { .. } => if let Some(c) = &current { own.get_mut(c).unwrap().push(s.clone()); },
                _ => {}
            }
        }
        let colors = [0.0f32, 0.0, 0.0, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 25.0, 0.0, 1.0, 1.5].map(f32::to_bits);
        let mut mat_sections = vec![default.unwrap_or(Section::Material { version: 5, colors, a: 0, b: 70, name: b"EMFX Default".to_vec(), last: 0 })];
        for (i, m) in named.iter().enumerate() {
            let idx = i as u16 + 1;
            match own.get(&m.name.to_lowercase()) {
                Some(secs) => for s in secs { let mut s = s.clone(); if let Section::Map { material, .. } = &mut s { *material = idx; } mat_sections.push(s); },
                None => {
                    mat_sections.push(Section::Material { version: 5, colors, a: 0, b: 70, name: m.name.as_bytes().to_vec(), last: 0 });
                    for (kind, t) in [(1u8, &m.diffuse), (3, &m.normal), (9, &m.specular)] { if let Some(t) = t { mat_sections.push(Section::Map { version: 4, kind, b: 0xF7, material: idx, values: [1.0f32, 0.0, 0.0, 1.0, 1.0, 0.0].map(f32::to_bits), texture: t.as_bytes().to_vec() }); } }
                }
            }
        }
        let parts: Vec<Part> = (0..nmat).filter(|&mi| !part_tris[mi].is_empty()).map(|mi| Part {
            head: cookie | fxa_of.get(mi).copied().unwrap_or(1),
            corners: part_corners[mi].iter().map(|(v, p, n, uv)| (*v, p.iter().chain(n).chain(uv).map(|x| x.to_bits()).collect())).collect(),
            indices: part_tris[mi].iter().flatten().copied().collect(),
        }).collect();
        let mut missing = std::collections::BTreeSet::new();
        let weights: Vec<Vec<(u16, u16, u32)>> = input.weights.iter().map(|w| {
            let mut v: Vec<(u16, f32)> = w.iter().filter(|x| x.1 > 0.0).filter_map(|(b, x)| match nodes.iter().position(|n| n == &b.to_lowercase()) { Some(i) => Some((i as u16, *x)), None => { missing.insert(b.clone()); None } }).collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            v.truncate(255);
            if v.is_empty() { v.push((mesh_node as u16, 1.0)); }
            let s: f32 = v.iter().map(|x| x.1).sum();
            v.into_iter().map(|(n, x)| (n, weight_cookie, (x / s).to_bits())).collect()
        }).collect();
        if !missing.is_empty() { bail!("vertex groups that are not bones of the base actor: {}", missing.into_iter().collect::<Vec<_>>().join(", ")); }
        // The sections again: our materials where the template's were, our mesh and skin in place of its own.
        let mut out = vec![];
        let mut mats_done = false;
        for s in std::mem::take(&mut block.sections) {
            match s {
                Section::Material { .. } | Section::Map { .. } => if !mats_done { out.extend(mat_sections.clone()); mats_done = true; },
                Section::Mesh { version, .. } => out.push(Section::Mesh { version, mesh: Mesh { node: mesh_node, vertices: input.positions.len() as u32, uv_sets: 1, word, parts: vec![] } }),
                Section::Skin { version, skin } if skin.node == mesh_node => out.push(Section::Skin { version, skin: Skin { node: mesh_node, weights: vec![] } }),
                s => out.push(s),
            }
        }
        if !mats_done { bail!("the base actor has no material sections"); }
        for s in out.iter_mut() {
            match s {
                Section::Mesh { mesh, .. } => mesh.parts = parts.clone(),
                Section::Skin { skin, .. } if skin.node == mesh_node => skin.weights = weights.clone(),
                _ => {}
            }
        }
        block.sections = out;
        let flag = block.extra.meshes.first().map(|m| m.flag).unwrap_or(1);
        block.extra.materials = (0..named.len()).map(|i| [0, i as u16 + 1, i as u16]).collect();
        block.extra.meshes = vec![CornerData { flag, colors: vec![0xFF00_0000; all_pos.len()], tangents: tangents.clone() }];
    }
    if nblocks > 1 { notes.push(format!("the {} levels of detail it carries take the same mesh", nblocks - 1)); }
    doc.table = table;
    doc.filetime = crate::volume::filetime_now();
    let first = crate::xact_doc::write(&doc);
    // The source of an actor compiled now is its FXA (as in every actor compiled since August 2006).
    let at = 64 + doc.records.len() * 30 + 4 + 6;
    doc.source = u32::from_le_bytes(first[at..at + 4].try_into().unwrap());
    Ok((crate::xact_doc::write(&doc), notes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The hero's head and body, fed their own mesh, decode to the same triangles (position and uv per corner,
    /// whichever way round) and weights.
    #[test]
    fn actors_take_their_own_mesh_back() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        for name in ["G3_Head_Hero_Hero_01", "G3_Hero_Body_Player"] {
            let k = g.find("_compiledanimation", &format!("{name}.xact")).unwrap();
            let d = g.read(&k).unwrap();
            let a = crate::g3_actor::decode_xact(&d).unwrap();
            let m = &a.meshes[0];
            let nv = m.weights.len();
            let mut positions = vec![[0f32; 3]; nv];
            for (c, &v) in m.vertex.iter().enumerate() { positions[v as usize] = m.positions[c]; }
            // Blender's order = the game's turned.
            let (mut cv, mut cn, mut cu) = (vec![], vec![], vec![]);
            for t in &m.triangles { for &c in &[t[0], t[2], t[1]] { cv.push(m.vertex[c as usize]); cn.push(m.normals[c as usize]); cu.push(m.uvs[c as usize]); } }
            let weights = m.weights.iter().map(|w| w.iter().map(|(n, x)| (a.nodes[*n as usize].name.clone(), *x)).collect()).collect();
            let input = SkinInput { positions, corner_vertex: cv, normals: cn, uvs: cu, tri_material: m.tri_material.clone(), weights };
            let mats: Vec<NewMaterial> = a.materials.iter().map(|x| NewMaterial { name: x.name.clone(), diffuse: x.diffuse.clone(), normal: x.normal.clone(), specular: x.specular.clone() }).collect();
            let (w, _) = replace_mesh(&d, &input, &mats).unwrap();
            let b = crate::g3_actor::decode_xact(&w).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            let key = |a: &crate::g3_actor::G3Actor| { let m = &a.meshes[0]; let mut v: Vec<[i64; 15]> = m.triangles.iter().map(|t| { let c = |i: u32| { let p = m.positions[i as usize]; let u = m.uvs[i as usize]; [(p[0] * 10.0) as i64, (p[1] * 10.0) as i64, (p[2] * 10.0) as i64, (u[0] * 1e4) as i64, (u[1] * 1e4) as i64] }; let mut cs = [c(t[0]), c(t[1]), c(t[2])]; let r = cs.iter().enumerate().min_by_key(|x| *x.1).unwrap().0; cs.rotate_left(r); let mut o = [0i64; 15]; for i in 0..3 { o[i * 5..i * 5 + 5].copy_from_slice(&cs[i]); } o }).collect(); v.sort(); v };
            assert_eq!(key(&a), key(&b), "{name}: triangles");
            assert_eq!(b.nodes.len(), a.nodes.len());
            let ws = |a: &crate::g3_actor::G3Actor| { let mut v: Vec<Vec<(u16, i64)>> = a.meshes[0].weights.iter().map(|w| { let mut x: Vec<(u16, i64)> = w.iter().map(|(n, x)| (*n, (x * 1000.0).round() as i64)).collect(); x.sort(); x }).collect(); v.sort(); v };
            assert_eq!(ws(&a), ws(&b), "{name}: weights");
            assert_eq!(b.materials.len(), a.materials.len());
            assert_eq!(crate::staticmesh::needs_turn(&b.meshes[0].positions, &b.meshes[0].normals, b.meshes[0].triangles.iter().copied()), true, "{name}: Gothic 3 order");
        }
    }

    /// The u32 at 32 is the size of the source the actor was compiled from (the slot `.xmot` and `.ximg` use for theirs).
    /// Every actor compiled from August 2006 on (FILETIME at 24) has the FXA length there; the 66 that do not were all
    /// compiled earlier, from bigger sources (all larger than their FXA). Ours is compiled now from the FXA it holds.
    #[test]
    fn the_source_size_is_the_fxa_length_as_in_every_later_actor() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        const AUG_2006: u64 = 127_988_640_000_000_000; // 2006-08-01 as FILETIME
        let (mut later, mut older_other) = (0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xact")) {
            let d = g.read(&k).unwrap();
            if d.get(78..82) != Some(b"FXA ") { continue; }
            let (time, source, fxa) = (u64::from_le_bytes(d[24..32].try_into().unwrap()), u32_at(&d, 32), u32_at(&d, 74));
            assert!(source >= fxa, "{k}");
            if time >= AUG_2006 { assert_eq!(source, fxa, "{k}"); later += 1; } else if source != fxa { older_other += 1; }
        }
        assert!(later > 100 && older_other == 66, "{later} later, {older_other} older with another source size");
        let k = g.find("_compiledanimation", "G3_Head_Hero_Hero_01.xact").unwrap();
        let d = g.read(&k).unwrap();
        let a = crate::g3_actor::decode_xact(&d).unwrap();
        let s = own_skin(&a, "G3_Head_Hero_Hero_01", "G3_Head_Hero_Hero_01", &std::env::temp_dir().join(format!("g3ie_test_source_{}", std::process::id())));
        let input = read_input(&s).unwrap();
        let mats: Vec<NewMaterial> = a.materials.iter().map(|x| NewMaterial { name: x.name.clone(), diffuse: x.diffuse.clone(), normal: x.normal.clone(), specular: x.specular.clone() }).collect();
        let (w, _) = replace_mesh(&d, &input, &mats).unwrap();
        assert_eq!(u32_at(&w, 32), u32_at(&w, 74));
        assert!(u64::from_le_bytes(w[24..32].try_into().unwrap()) > AUG_2006);
    }

    /// What every actor of the game keeps (498 of 498 blocks with materials): the default material first, materials of
    /// maps and mesh parts counted from 1, the map after the FXA naming `<material>.xshmat` in the string table, a colour
    /// and tangent per corner, the skin on the mesh's node. Returns what breaks it.
    pub(crate) fn laid_out_as_the_games(doc: &crate::xact_doc::Doc) -> Vec<String> {
        use crate::xact_doc::Section;
        let mut bad = vec![];
        for (bi, b) in doc.blocks.iter().enumerate() {
            let mats: Vec<String> = b.sections.iter().filter_map(|s| if let Section::Material { name, .. } = s { Some(String::from_utf8_lossy(name).into_owned()) } else { None }).collect();
            let Some(mesh) = b.sections.iter().find_map(|s| if let Section::Mesh { mesh, .. } = s { Some(mesh) } else { None }) else { continue };
            if mats.first().map(String::as_str) != Some("EMFX Default") { bad.push(format!("block {bi}: the default material is not first")); }
            for s in &b.sections { if let Section::Map { material, .. } = s { if *material == 0 || *material as usize >= mats.len() { bad.push(format!("block {bi}: map of material {material}")); } } }
            if mesh.parts.iter().any(|p| (p.head & 0xff) as usize >= mats.len()) { bad.push(format!("block {bi}: a part names no material")); }
            for r in &b.extra.materials {
                let want = mats.get(r[1] as usize).map(|n| format!("{}.xshmat", n.split('|').next().unwrap()).to_lowercase());
                let got = doc.table.get(r[2] as usize).map(|s| String::from_utf8_lossy(s).to_lowercase());
                if r[0] != 0 || want.is_none() || want != got { bad.push(format!("block {bi}: map {r:?} -> {got:?}, material {want:?}")); }
            }
            if b.extra.materials.len() >= mats.len() { bad.push(format!("block {bi}: {} map records for {} materials", b.extra.materials.len(), mats.len())); }
            let corners: usize = mesh.parts.iter().map(|p| p.corners.len()).sum();
            if b.extra.meshes.len() != 1 || b.extra.meshes[0].colors.len() != corners || b.extra.meshes[0].tangents.len() != corners { bad.push(format!("block {bi}: corner data for {:?} of {corners} corners", b.extra.meshes.iter().map(|m| m.colors.len()).collect::<Vec<_>>())); }
            if !b.sections.iter().any(|s| matches!(s, Section::Skin { skin, .. } if skin.node == mesh.node && skin.weights.len() == mesh.vertices as usize)) { bad.push(format!("block {bi}: no skin for the mesh")); }
        }
        bad
    }

    /// A stock actor replaced by its own mesh (a creature, a body, a head, and a body that carries its two levels of
    /// detail) comes out laid out as the game's: the same blocks, each as above, its sibling `_lod1`/`_lod2` files too;
    /// the template itself passes the same check.
    #[test]
    fn a_replaced_actor_is_laid_out_as_the_games() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_layout_{}", std::process::id()));
        for base in ["G3_Wolf_Body_01", "G3_Hero_Body_Player", "G3_Head_Hero_Hero_01", "G3_Fat_Body_AssTrader"] {
            let k = g.find("_compiledanimation", &format!("{base}.xact")).unwrap();
            let d = g.read(&k).unwrap();
            let t = crate::xact_doc::read(&d).unwrap();
            assert!(laid_out_as_the_games(&t).is_empty(), "{base} (the game's): {:?}", laid_out_as_the_games(&t));
            let a = crate::g3_actor::decode_xact(&d).unwrap();
            let (r, files) = build(&g, &own_skin(&a, base, base, &dir.join(base))).unwrap();
            assert!(r.replaced, "{base}");
            let siblings = (1..=3).filter(|i| g.size_of(&k.replace(".xact", &format!("_lod{i}.xact"))).is_some()).count();
            let actors: Vec<&(String, String, Vec<u8>)> = files.iter().filter(|f| f.1.to_lowercase().ends_with(".xact")).collect();
            assert_eq!(actors.len(), 1 + siblings, "{base}: {:?}", r.notes);
            for f in &actors {
                let doc = crate::xact_doc::read(&f.2).unwrap_or_else(|e| panic!("{}: {e:#}", f.1));
                let template = crate::xact_doc::read(&g.read(&g.find("_compiledanimation", f.1.rsplit('/').next().unwrap()).unwrap()).unwrap()).unwrap();
                assert_eq!(doc.blocks.len(), template.blocks.len(), "{}: levels of detail", f.1);
                assert!(laid_out_as_the_games(&doc).is_empty(), "{}: {:?}", f.1, laid_out_as_the_games(&doc));
                let b = crate::g3_actor::decode_xact(&f.2).unwrap();
                assert_eq!(b.meshes[0].triangles.len(), a.meshes[0].triangles.len(), "{}", f.1);
            }
            if base == "G3_Fat_Body_AssTrader" { assert!(siblings == 2 && crate::xact_doc::read(&actors[0].2).unwrap().blocks.len() == 3); }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A new actor's name becomes a file name the way a model's does: typed with Cyrillic, spaces or slashes it never
    /// turns into a path out of the mod's folder. The base actor's own name (any case, Blender's `.001`) replaces it.
    #[test]
    fn a_new_actor_is_named_like_a_model() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let base = "G3_Head_Hero_Hero_01";
        let a = crate::g3_actor::decode_xact(&g.read(&g.find("_compiledanimation", &format!("{base}.xact")).unwrap()).unwrap()).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_actor_name_{}", std::process::id()));
        let spec = |name: &str| own_skin(&a, base, name, &dir);
        let (r, files) = build(&g, &spec(r"Моя голова/..\..\x")).unwrap();
        let path = &files.iter().find(|f| f.0 == "_compiledAnimation").unwrap().1;
        assert!(!r.replaced && path.starts_with("gothic3_impexp/") && path.ends_with(".xact"), "{path}");
        assert!(path[15..].chars().all(|c| c.is_ascii_alphanumeric() || "_.".contains(c)) && !path.contains(".."), "{path}");
        assert_eq!(r.actor, path[15..path.len() - 5]);
        let (r, files) = build(&g, &spec("g3_head_hero_hero_01.001")).unwrap();
        assert!(r.replaced, "{}", r.actor);
        assert_eq!(files.iter().find(|f| f.0 == "_compiledAnimation").unwrap().1.to_lowercase(), g.path_of(&g.find("_compiledanimation", &format!("{base}.xact")).unwrap()).unwrap().to_lowercase());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The spec the add-on would send for actor `a` (base `base`) with its own mesh, skin and materials, as `name`;
    /// the skin file goes into `dir`.
    pub(crate) fn own_skin(a: &crate::g3_actor::G3Actor, base: &str, name: &str, dir: &std::path::Path) -> ActorSpec {
        let m = &a.meshes[0];
        let nv = m.weights.len();
        let mut positions = vec![[0f32; 3]; nv];
        for (c, &v) in m.vertex.iter().enumerate() { positions[v as usize] = m.positions[c]; }
        // The add-on's skin file: positions · corner vertex · normals · uvs · material per triangle · 8 weights per vertex.
        let mut b = vec![];
        for p in &positions { for x in p { b.extend(x.to_le_bytes()); } }
        let corners: Vec<u32> = m.triangles.iter().flat_map(|t| [t[0], t[2], t[1]]).collect();
        for &c in &corners { b.extend(m.vertex[c as usize].to_le_bytes()); }
        for &c in &corners { for x in m.normals[c as usize] { b.extend(x.to_le_bytes()); } }
        for &c in &corners { for x in m.uvs[c as usize] { b.extend(x.to_le_bytes()); } }
        for &t in &m.tri_material { b.extend(t.to_le_bytes()); }
        for w in &m.weights { for k in 0..8 { let (n, x) = w.get(k).copied().unwrap_or((0, 0.0)); b.extend((n as u32).to_le_bytes()); b.extend(x.to_le_bytes()); } }
        std::fs::create_dir_all(dir).unwrap();
        let geo = dir.join("skin.bin");
        std::fs::write(&geo, b).unwrap();
        ActorSpec { base: base.into(), name: name.into(), geometry: geo.to_string_lossy().into_owned(), vertices: nv, corners: corners.len(), bones: a.nodes.iter().map(|n| n.name.clone()).collect(),
            materials: a.materials.iter().map(|x| crate::export::MatSpec { name: x.name.clone(), ..Default::default() }).collect() }
    }
}

#[derive(serde::Deserialize)]
pub struct ActorSpec { pub base: String, pub name: String, pub geometry: String, pub vertices: usize, pub corners: usize, pub bones: Vec<String>, pub materials: Vec<crate::export::MatSpec> }

#[derive(serde::Serialize)]
pub struct ActorReport { pub actor: String, pub replaced: bool, pub vertices: usize, pub triangles: usize, pub materials: Vec<String>, pub notes: Vec<String> }

/// Geometry from the add-on: positions f32×3 per vertex · corner vertex u32 · normals f32×3 and uvs f32×2 per corner
/// · material u32 per triangle · 8 × (u32 bone, f32 weight) per vertex.
pub(crate) fn read_input(spec: &ActorSpec) -> Result<SkinInput> {
    let d = std::fs::read(&spec.geometry).with_context(|| format!("read {}", spec.geometry))?;
    let (v, n) = (spec.vertices, spec.corners);
    let want = v * 12 + n * 4 + n * 12 + n * 8 + n / 3 * 4 + v * 64;
    if d.len() != want { bail!("skinned geometry is {} bytes, expected {want}", d.len()); }
    let f = |a: usize| f32::from_le_bytes(d[a..a + 4].try_into().unwrap());
    let u = |a: usize| u32::from_le_bytes(d[a..a + 4].try_into().unwrap());
    let positions = (0..v).map(|i| [f(i * 12), f(i * 12 + 4), f(i * 12 + 8)]).collect();
    let o = v * 12;
    let corner_vertex: Vec<u32> = (0..n).map(|i| u(o + i * 4)).collect();
    let o = o + n * 4;
    let normals = (0..n).map(|i| [f(o + i * 12), f(o + i * 12 + 4), f(o + i * 12 + 8)]).collect();
    let o = o + n * 12;
    let uvs = (0..n).map(|i| [f(o + i * 8), f(o + i * 8 + 4)]).collect();
    let o = o + n * 8;
    let tri_material = (0..n / 3).map(|i| u(o + i * 4)).collect();
    let o = o + n / 3 * 4;
    if corner_vertex.iter().any(|&c| c as usize >= v) { bail!("corner refers past the vertices"); }
    let weights = (0..v).map(|i| (0..8).filter_map(|k| { let b = u(o + i * 64 + k * 8) as usize; let w = f(o + i * 64 + k * 8 + 4); (w > 0.0).then(|| spec.bones.get(b).map(|n| (n.clone(), w))).flatten() }).collect()).collect();
    Ok(SkinInput { positions, corner_vertex, normals, uvs, tri_material, weights })
}

pub fn build(g: &crate::g3::G3Ctx, spec: &ActorSpec) -> Result<(ActorReport, Vec<(String, String, Vec<u8>)>)> {
    let base_key = g.find("_compiledanimation", &format!("{}.xact", spec.base.trim_end_matches(".xact"))).with_context(|| format!("{}: no such actor in Gothic 3", spec.base))?;
    let template = g.read(&base_key)?;
    let base = crate::g3_actor::decode_xact(&template)?;
    let input = read_input(spec)?;
    let now = crate::volume::filetime_now();
    let mut files = vec![];
    let mut notes = vec![];
    let mats = materials(g, &spec.materials, &base.materials, now, &mut files, &mut notes)?;
    let (bytes, more) = replace_mesh(&template, &input, &mats)?;
    notes.extend(more);
    // A name the game's files can carry, as a model's: never a path, Cyrillic transliterated.
    let name = crate::export::clean(&spec.name);
    if name.is_empty() { bail!("actor name is empty"); }
    let base_path = g.path_of(&base_key).unwrap_or(&base_key).to_string();
    let replaced = name.eq_ignore_ascii_case(&crate::stem_of(&base_path));
    let path = if replaced { base_path } else { format!("gothic3_impexp/{name}.xact") };
    files.push(("_compiledAnimation".to_string(), path, bytes));
    // Replacing, the actor's levels of detail beside it (`_lod1`, `_lod2`: the same as the ones it carries) follow.
    if replaced {
        for i in 1..=3 {
            let k = base_key.replace(".xact", &format!("_lod{i}.xact"));
            if g.size_of(&k).is_none() { continue; }
            let (b, _) = replace_mesh(&g.read(&k)?, &input, &mats).with_context(|| k.clone())?;
            files.push(("_compiledAnimation".to_string(), g.path_of(&k).unwrap_or(&k).to_string(), b));
            notes.push(format!("_lod{i} too"));
        }
    }
    Ok((ActorReport { actor: name, replaced, vertices: input.positions.len(), triangles: input.corner_vertex.len() / 3, materials: mats.iter().map(|m| m.name.clone()).collect(), notes }, files))
}

/// The materials of an actor: `keep`'s own where a spec names one of them, else new ones (images and a shader material
/// from a template) into `files`.
pub(crate) fn materials(g: &crate::g3::G3Ctx, specs: &[crate::export::MatSpec], keep: &[crate::g3_actor::G3Material], now: u64, files: &mut Vec<(String, String, Vec<u8>)>, notes: &mut Vec<String>) -> Result<Vec<NewMaterial>> {
    let mut mats = vec![];
    for m in specs {
        let clean = crate::export::clean;
        let name = clean(&m.name);
        if let Some(own) = keep.iter().find(|x| clean(&x.name).eq_ignore_ascii_case(&name)) {
            notes.push(format!("{name}: kept"));
            mats.push(NewMaterial { name: own.name.clone(), diffuse: own.diffuse.clone(), normal: own.normal.clone(), specular: own.specular.clone() });
            continue;
        }
        let h = [&m.diffuse, &m.normal, &m.specular].into_iter().flatten().fold(name.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)), |h, p| std::fs::read(p).unwrap_or_default().iter().fold(h, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3)));
        let mname = format!("G3IE_{name}_{:06x}", h & 0xffffff);
        // The shader material of the same name, from a skinned template: the game keeps one (TransformationType 2) for
        // all but 1 of the 193 materials its actors name.
        let made = crate::export::make_material(g, m, &mname, true, now, files, notes)?;
        notes.push(format!("{name}: new {mname} ({})", crate::g3_write::look_name(made.template)));
        mats.push(NewMaterial { name: mname, diffuse: made.diffuse, normal: made.normal, specular: made.specular });
    }
    Ok(mats)
}
