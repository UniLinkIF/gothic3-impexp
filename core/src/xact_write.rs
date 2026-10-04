//! A new skinned mesh on a Gothic 3 actor's skeleton: the actor (`.xact`) is the template; its nodes and everything
//! else stay, its mesh, skin and materials become ours.
//!
//! ```text
//! 0   "GENOMFLE" · u16 1 · u32 string-table offset · … · FILETIME at 24 · u32 FXA bytes at 32
//!     · … "gena" · u16 · u32 FXA bytes (at 74) · "FXA " u8 1 u8 1 (at 78) · sections · mesh extra data · table
//! section  u32 id · u32 size · u32 version · body
//!   3 v3  mesh: u32 node · u32 vertices · u32 corners · u32 indices · u32 parts · u32 uv channels (1) · u32 (kept)
//!         part: u32 (low byte = material, the rest kept) · u32 indices · u32 corners
//!               · corner: u32 vertex · f32 position[3] · f32 normal[3] · f32 uv[2] · u32 index[indices]
//!   4 v1  skin: u32 node · per vertex: u8 n · n × (u16 node, u16 0x170C, f32 weight)
//!   6 v5  material: f32[16] colours and factors · u16 0 · u16 70 · u32 length · name · f32 0
//!   7 v4  texture map: u8 type (1 diffuse, 3 normal, 9 specular) · u8 0xF7 · u16 material · f32[6] · u32 length · name
//! extra   a head of 16 or 22 bytes (kept from the template) · u32 corners · u32 colour[corners]
//!         · f32 tangent[3][corners]
//! ```
//! Triangles: Gothic 3's actors keep the opposite order of its static meshes, so corners 1 and 2 swap.
//! Only actors with one mesh can be a template (the human bodies, heads and most creatures).

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

fn u32_at(d: &[u8], p: usize) -> u32 { u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) }

fn section(o: &mut Vec<u8>, id: u32, ver: u32, body: &[u8]) {
    o.extend(id.to_le_bytes());
    o.extend((body.len() as u32).to_le_bytes());
    o.extend(ver.to_le_bytes());
    o.extend(body);
}

fn material_sections(o: &mut Vec<u8>, m: &NewMaterial, index: u16) {
    let mut b = vec![];
    for x in [0.0f32, 0.0, 0.0, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 25.0, 0.0, 1.0, 1.5] { b.extend(x.to_le_bytes()); }
    b.extend(0u16.to_le_bytes());
    b.extend(70u16.to_le_bytes());
    b.extend((m.name.len() as u32).to_le_bytes());
    b.extend(m.name.as_bytes());
    b.extend(0f32.to_le_bytes());
    section(o, 6, 5, &b);
    for (ty, tex) in [(1u8, &m.diffuse), (3, &m.normal), (9, &m.specular)] {
        let Some(t) = tex else { continue };
        let mut b = vec![ty, 0xF7];
        b.extend(index.to_le_bytes());
        for x in [0.3f32, 0.0, 0.0, 1.0, 1.0, 0.0] { b.extend(x.to_le_bytes()); }
        b.extend((t.len() as u32).to_le_bytes());
        b.extend(t.as_bytes());
        section(o, 7, 4, &b);
    }
}

/// `template` with `input` as its mesh. Returns the file and notes.
pub fn replace_mesh(template: &[u8], input: &SkinInput, materials: &[NewMaterial]) -> Result<(Vec<u8>, Vec<String>)> {
    if !template.starts_with(b"GENOMFLE") || template.get(78..82) != Some(b"FXA ") { bail!("the base actor is not a GENOMFLE FXA actor"); }
    let fxa_len = u32_at(template, 74) as usize;
    let fxa_end = 78 + fxa_len;
    let table = u32_at(template, 10) as usize;
    if fxa_end > table || table > template.len() { bail!("actor sizes do not fit"); }
    // Sections and the names of the nodes.
    let mut secs = vec![];
    let mut p = 84;
    let mut nodes = vec![];
    while p + 12 <= fxa_end {
        let (id, size, ver) = (u32_at(template, p), u32_at(template, p + 4) as usize, u32_at(template, p + 8));
        if p + 12 + size > fxa_end { bail!("section {id} runs past the actor"); }
        if id == 0 { let b = p + 12 + 12 + 16 + 40; let n = u32_at(template, b) as usize; nodes.push(String::from_utf8_lossy(&template[b + 4..b + 4 + n]).to_lowercase()); }
        secs.push((p, id, size, ver));
        p += 12 + size;
    }
    let meshes: Vec<_> = secs.iter().filter(|s| s.1 == 3).collect();
    if meshes.len() != 1 { bail!("the base actor has {} meshes; only one-mesh actors (human bodies, heads) can be replaced", meshes.len()); }
    let &(mp, _, _, mver) = meshes[0];
    let mesh_node = u32_at(template, mp + 12);
    let dw1 = u32_at(template, mp + 36);
    let dw2 = u32_at(template, mp + 40) & 0xFFFF_FF00;
    // Extra data = a head (16 or 22 bytes, kept) · u32 corners · a colour and a tangent per corner.
    let old_corners = u32_at(template, mp + 20) as usize;
    let tail = table - fxa_end;
    let Some(head_len) = tail.checked_sub(4 + old_corners * 16).filter(|&h| h <= 64 && u32_at(template, fxa_end + h) as usize == old_corners) else { bail!("the base actor's mesh extra data is not colours and tangents per corner") };
    let extra_head = template[fxa_end..fxa_end + head_len].to_vec();

    // Corners per material: one per (vertex, normal, uv); Gothic 3 order (corners 1 and 2 swapped).
    let nc = input.corner_vertex.len();
    if nc % 3 != 0 || input.normals.len() != nc || input.uvs.len() != nc || input.tri_material.len() != nc / 3 { bail!("corner arrays disagree"); }
    let nmat = materials.len().max(1);
    let (mut part_corners, mut part_tris): (Vec<Vec<(u32, [f32; 3], [f32; 3], [f32; 2])>>, Vec<Vec<[u32; 3]>>) = (vec![vec![]; nmat], vec![vec![]; nmat]);
    let mut seen: Vec<HashMap<(u32, [u32; 5]), u32>> = vec![HashMap::new(); nmat];
    for t in 0..nc / 3 {
        let mi = (input.tri_material[t] as usize).min(nmat - 1);
        let ids = [0, 2, 1].map(|k| {
            let c = t * 3 + k;
            let v = input.corner_vertex[c];
            let n = input.normals[c];
            let uv = input.uvs[c];
            let key = (v, [n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), uv[0].to_bits(), uv[1].to_bits()]);
            *seen[mi].entry(key).or_insert_with(|| { part_corners[mi].push((v, input.positions[v as usize], n, uv)); part_corners[mi].len() as u32 - 1 })
        });
        part_tris[mi].push(ids);
    }
    let corners: usize = part_corners.iter().map(Vec::len).sum();
    let tris: usize = part_tris.iter().map(Vec::len).sum();
    let mut mesh = vec![];
    for x in [mesh_node, input.positions.len() as u32, corners as u32, (tris * 3) as u32, nmat as u32, 1, dw1] { mesh.extend(x.to_le_bytes()); }
    let mut all_pos = vec![]; let mut all_nrm = vec![]; let mut all_uv = vec![]; let mut all_tri = vec![];
    for mi in 0..nmat {
        mesh.extend((dw2 | mi as u32).to_le_bytes());
        mesh.extend(((part_tris[mi].len() * 3) as u32).to_le_bytes());
        mesh.extend((part_corners[mi].len() as u32).to_le_bytes());
        let base = all_pos.len() as u32;
        for (v, p, n, uv) in &part_corners[mi] {
            mesh.extend(v.to_le_bytes());
            for x in p.iter().chain(n).chain(uv) { mesh.extend(x.to_le_bytes()); }
            all_pos.push(*p); all_nrm.push(*n); all_uv.push(*uv);
        }
        for t in &part_tris[mi] { for i in t { mesh.extend(i.to_le_bytes()); } all_tri.push(t.map(|i| i + base)); }
    }

    // Skin per vertex.
    let mut notes = vec![];
    let mut skin = mesh_node.to_le_bytes().to_vec();
    let mut missing = std::collections::BTreeSet::new();
    for w in &input.weights {
        let mut v: Vec<(u16, f32)> = w.iter().filter(|x| x.1 > 0.0).filter_map(|(b, x)| match nodes.iter().position(|n| n == &b.to_lowercase()) { Some(i) => Some((i as u16, *x)), None => { missing.insert(b.clone()); None } }).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        v.truncate(255);
        if v.is_empty() { v.push((mesh_node as u16, 1.0)); }
        let s: f32 = v.iter().map(|x| x.1).sum();
        skin.push(v.len() as u8);
        for (n, x) in v { skin.extend(n.to_le_bytes()); skin.extend(0x170Cu16.to_le_bytes()); skin.extend((x / s).to_le_bytes()); }
    }
    if !missing.is_empty() { bail!("vertex groups that are not bones of the base actor: {}", missing.into_iter().collect::<Vec<_>>().join(", ")); }

    // The FXA again: materials where the template's were, our mesh and skin in place of its own.
    let mut fxa = template[78..84].to_vec();
    let mut mats_done = false;
    for &(sp, id, size, ver) in &secs {
        match id {
            6 | 7 => {
                if !mats_done { for (i, m) in materials.iter().enumerate() { material_sections(&mut fxa, m, i as u16); } mats_done = true; }
            }
            3 => section(&mut fxa, 3, mver, &mesh),
            4 if u32_at(template, sp + 12) == mesh_node => section(&mut fxa, 4, ver, &skin),
            _ => fxa.extend(&template[sp..sp + 12 + size]),
        }
    }
    if !mats_done { notes.push("the base actor had no material sections; none written".into()); }
    let mut extra = extra_head;
    extra.extend((corners as u32).to_le_bytes());
    for _ in 0..corners { extra.extend(0xFF00_0000u32.to_le_bytes()); }
    let tan = crate::xcmsh_write::tangents(&all_pos, &all_nrm, &all_uv, &all_tri);
    for t in &tan { let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-8); for x in t { extra.extend((x / l).to_le_bytes()); } }

    let mut out = template[..78].to_vec();
    let fxa_size = fxa.len() as u32;
    out.extend(&fxa);
    out.extend(&extra);
    let new_table = out.len() as u32;
    out.extend(&template[table..]);
    out[10..14].copy_from_slice(&new_table.to_le_bytes());
    out[24..32].copy_from_slice(&crate::volume::filetime_now().to_le_bytes());
    out[32..36].copy_from_slice(&fxa_size.to_le_bytes());
    out[74..78].copy_from_slice(&fxa_size.to_le_bytes());
    Ok((out, notes))
}

#[cfg(test)]
mod tests {
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
}

#[derive(serde::Deserialize)]
pub struct ActorSpec { pub base: String, pub name: String, pub geometry: String, pub vertices: usize, pub corners: usize, pub bones: Vec<String>, pub materials: Vec<crate::export::MatSpec> }

#[derive(serde::Serialize)]
pub struct ActorReport { pub actor: String, pub replaced: bool, pub vertices: usize, pub triangles: usize, pub materials: Vec<String>, pub notes: Vec<String> }

/// Geometry from the add-on: positions f32×3 per vertex · corner vertex u32 · normals f32×3 and uvs f32×2 per corner
/// · material u32 per triangle · 8 × (u32 bone, f32 weight) per vertex.
fn read_input(spec: &ActorSpec) -> Result<SkinInput> {
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
    let mut mats = vec![];
    for m in &spec.materials {
        let clean = |s: &str| -> String { let b = match s.rsplit_once('.') { Some((a, x)) if x.len() == 3 && x.bytes().all(|c| c.is_ascii_digit()) => a, _ => s }; b.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect() };
        let name = clean(&m.name);
        if let Some(own) = base.materials.iter().find(|x| clean(&x.name).eq_ignore_ascii_case(&name)) {
            notes.push(format!("{name}: kept"));
            mats.push(NewMaterial { name: own.name.clone(), diffuse: own.diffuse.clone(), normal: own.normal.clone(), specular: own.specular.clone() });
            continue;
        }
        let h = [&m.diffuse, &m.normal, &m.specular].into_iter().flatten().fold(name.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)), |h, p| std::fs::read(p).unwrap_or_default().iter().fold(h, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3)));
        let mname = format!("G3IE_{name}_{:06x}", h & 0xffffff);
        let mut img = |png: &Option<String>, suffix: &str, normal: bool, alpha: bool| -> Result<Option<String>> {
            let Some(p) = png else { return Ok(None) };
            let im = image::open(p).with_context(|| format!("material {name}: {p}"))?.to_rgba8();
            let (w, hh) = im.dimensions();
            let px = if normal { crate::g3_write::normal_to_game(im.as_raw()) } else { im.into_raw() };
            let t = format!("{mname}{suffix}");
            files.push(("_compiledImage".to_string(), format!("gothic3_impexp/{t}.ximg"), crate::g3_write::ximg(&px, w, hh, if alpha { crate::g3_write::Dxt::Dxt5 } else { crate::g3_write::Dxt::Dxt1 }, now)?));
            Ok(Some(t))
        };
        let diffuse = img(&m.diffuse, "_Diffuse_01", false, m.alpha_test.is_some())?;
        let normal = img(&m.normal, "_Normal_01", true, false)?;
        let specular = img(&m.specular, "_Specular_01", false, false)?;
        // A shader material of the same name too, in case the engine takes the actor's material from there.
        let t = if m.alpha_test.is_some() { &crate::g3_write::MASKED } else if specular.is_some() { &crate::g3_write::OPAQUE_SPECULAR } else { &crate::g3_write::OPAQUE };
        let tk = g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).context("material template")?;
        let tex: Vec<(&str, String)> = [("diffuse", &diffuse), ("normal", &normal), ("specular", &specular)].into_iter().filter_map(|(r, x)| x.clone().map(|x| (r, x))).collect();
        files.push(("_compiledMaterial".to_string(), format!("gothic3_impexp/{mname}.xshmat"), crate::g3_write::material(&g.read(&tk)?, t, &tex, m.alpha_test)?));
        notes.push(format!("{name}: new {mname}"));
        mats.push(NewMaterial { name: mname, diffuse, normal, specular });
    }
    let (bytes, more) = replace_mesh(&template, &input, &mats)?;
    notes.extend(more);
    let base_path = g.path_of(&base_key).unwrap_or(&base_key).to_string();
    let replaced = spec.name.eq_ignore_ascii_case(&crate::stem_of(&base_path));
    let path = if replaced { base_path } else { format!("gothic3_impexp/{}.xact", spec.name) };
    files.push(("_compiledAnimation".to_string(), path, bytes));
    Ok((ActorReport { actor: spec.name.clone(), replaced, vertices: input.positions.len(), triangles: input.corner_vertex.len() / 3, materials: mats.iter().map(|m| m.name.clone()).collect(), notes }, files))
}
