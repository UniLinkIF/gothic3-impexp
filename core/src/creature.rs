//! A new creature from Blender: its own skeleton, mesh and clips, written from nothing but Blender's data.
//!
//! The engine ties a clip to its actor by the clip name's first word (Engine.dll `eCWrapper_emfx2Motion::
//! CleanUpMotions`: `G3_<word>_Skeleton.FXA`, else `G3_<word>_Body_01.FXA`), so a creature called `Mole` is the actor
//! `G3_Mole_Body_01.xact` (or `G3_Mole_Skeleton.xact` without a mesh) and its clips are `Mole_<stance>_…_%_…xmot`,
//! named from the words the game composes clip names of. Laid out as the game's own actors: an identity root node
//! named after the creature carries the mesh (the wolf's `Wolf`, beside `Wolf_ROOT`), the default material first, the
//! skin per vertex, a colour and a tangent per corner, limits (section 8) and physics shapes (9) where Blender gives
//! them. Placing the creature in the world takes a template the game's editors make (`.tple`), not made here.
//!
//! The skeleton is the glb Blender writes of the armature (rest pose, glTF axes): the nodes' local transforms as the
//! motion export reads keys, so the actor's bind pose and its clips agree.

use crate::xact_doc::{Block, CornerData, Doc, Extra, Mesh, Node, Part, Section, Skin};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// Bones the game's code names (Game.dll, Script.dll): the head and spine it looks for after the creature's word, and
/// the slots it hangs items on.
pub const REQUIRED: [&str; 2] = ["Head_Head_1", "Spine_Spine_1"];
pub const SLOTS: [&str; 15] = ["Slot_Head", "Slot_Helmet", "Slot_Hair", "Slot_Beard", "Slot_Body", "Slot_Bow", "Slot_CrossBow", "Slot_AxeBack", "Slot_ShieldBack", "Slot_StaffBack",
    "Slot_MeleeBackLeft", "Slot_MeleeBackRight", "Slot_LeftHand_Shield", "Slot_LeftHand_Weapon", "Slot_RightHand_Weapon"];

#[derive(serde::Deserialize)]
pub struct CreatureSpec {
    pub word: String,
    /// The armature (and its actions) as Blender's glTF exporter writes it.
    pub glb: String,
    /// The mesh as the actor export sends it (`skin.bin`), or none for a bare skeleton.
    #[serde(default)] pub skin: Option<crate::xact_write::ActorSpec>,
    /// Per bone: rotation limits (18 floats: translation min/max, rotation min/max (radians), scale min/max) and a
    /// physics shape (kind, 8 floats), when Blender has them.
    #[serde(default)] pub limits: HashMap<String, Vec<f32>>,
    #[serde(default)] pub shapes: HashMap<String, (u32, Vec<f32>)>,
    /// Bone names the game's code does not expect are refused unless this is set.
    #[serde(default)] pub lenient: bool,
    /// The actions that are this creature's (the glb may hold others); none given = all.
    #[serde(default)] pub actions: Option<Vec<String>>,
}

#[derive(serde::Serialize, Default)]
pub struct CreatureReport { pub actor: String, pub bones: usize, pub vertices: usize, pub triangles: usize, pub clips: Vec<String>, pub skipped: Vec<String>, pub notes: Vec<String>, pub files: Vec<String> }

type Q = [f32; 4];
fn qmul(a: Q, b: Q) -> Q { [a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1], a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0], a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3], a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]] }
fn qrot(q: Q, v: [f32; 3]) -> [f32; 3] { let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), [-q[0], -q[1], -q[2], q[3]]); [p[0], p[1], p[2]] }
fn game_p(t: [f32; 3]) -> [f32; 3] { [t[0] * 100.0, t[1] * 100.0, -t[2] * 100.0] }

/// The bone names checked against what the game's code expects of a creature called `word`.
pub fn check_bones(word: &str, bones: &[String]) -> Vec<String> {
    let mut bad = vec![];
    for b in bones {
        if !b.is_ascii() || b.contains(char::is_whitespace) { bad.push(format!("{b}: letters, digits and _ only")); continue; }
        if b.starts_with("Slot_") { if !SLOTS.contains(&b.as_str()) { bad.push(format!("{b}: not a slot the game hangs items on ({})", SLOTS.join(", "))); } continue; }
        if b != word && !b.starts_with(&format!("{word}_")) { bad.push(format!("{b}: a creature's bones start with {word}_ (the game finds its head and spine so)")); continue; }
        let l = b.to_lowercase();
        for (side, word_) in [("left", "Left"), ("right", "Right")] {
            if l.contains(side) && !b.contains(&format!("_{word_}_")) { bad.push(format!("{b}: a side is written _{word_}_ (as {word}_{word_}_Arm_Arm_1)")); }
        }
    }
    for r in REQUIRED { let n = format!("{word}_{r}"); if !bones.contains(&n) { bad.push(format!("no bone {n}: the game looks for it")); } }
    bad
}

/// A clip name for an action: `<stance>_<left>_<right>_<pose>_<action>_<phase>_<N|O|I>_<direction>_<VV>` with or
/// without a first word and `_%_…`; the creature's word goes first, the clip's own part defaults to `%_00_P0_0`.
pub fn clip_name(word: &str, action: &str) -> Result<String> {
    let (head, tail) = match action.split_once("_%_") { Some((h, t)) => (h, format!("%_{t}")), None => (action, "%_00_P0_0".to_string()) };
    let p: Vec<&str> = head.split('_').collect();
    let p = match p.len() { 10 => &p[1..], 9 => &p[..], n => bail!("{action}: {n} words, a clip name has the 8 words of the game and a variant (Stand_None_None_P0_Ambient_Loop_N_Fwd_01)") };
    let variant: u32 = p[8].parse().with_context(|| format!("{action}: the variant {:?} is not a number", p[8]))?;
    let words: [String; 8] = std::array::from_fn(|i| p[i].to_string());
    crate::clip_words::compose(word, &words, variant, &tail).with_context(|| action.to_string())
}

struct Joint { name: String, parent: Option<String>, pos: [f32; 3], rot: Q }

/// The armature's joints from the glb: local transforms in game axes; a root bone takes in whatever is above it.
fn joints(gl: &gltf::Gltf) -> Result<(Vec<Joint>, Vec<String>)> {
    let mut parent: HashMap<usize, usize> = HashMap::new();
    for n in gl.nodes() { for c in n.children() { parent.insert(c.index(), n.index()); } }
    // The skin's joints; a bare skeleton (no mesh, so no skin) is every node below the armature that is no mesh.
    let ids: Vec<usize> = match gl.skins().next() {
        Some(skin) => skin.joints().map(|j| j.index()).collect(),
        None => gl.nodes().filter(|n| n.mesh().is_none() && parent.contains_key(&n.index())).map(|n| n.index()).collect(),
    };
    if ids.is_empty() { bail!("the glb has no armature"); }
    let mut notes = vec![];
    let nodes: Vec<gltf::Node> = gl.nodes().collect();
    let mut out = vec![];
    for &i in &ids {
        let n = &nodes[i];
        let (mut t, mut r, s) = n.transform().decomposed();
        if s.iter().any(|x| (x - 1.0).abs() > 1e-3) { notes.push(format!("{}: scaled {s:?} at rest; the game's bones are not", n.name().unwrap_or("?"))); }
        let mut p = parent.get(&i).copied();
        // Up to the first joint above: what lies between (the armature object) goes into this bone.
        while let Some(a) = p { if ids.contains(&a) { break; } let (pt, pr, _) = nodes[a].transform().decomposed(); let rt = qrot(pr, t); t = [pt[0] + rt[0], pt[1] + rt[1], pt[2] + rt[2]]; r = qmul(pr, r); p = parent.get(&a).copied(); }
        out.push(Joint { name: n.name().unwrap_or("").to_string(), parent: p.map(|a| nodes[a].name().unwrap_or("").to_string()), pos: game_p(t), rot: crate::glb::conv_q(r) });
    }
    Ok((out, notes))
}

/// Per animation of the glb: (name, keys per joint name): positions (cm) and rotations in game axes, times from 0.
fn animations(gl: &gltf::Gltf) -> Result<Vec<(String, HashMap<String, crate::motion_write::Keys>)>> {
    let blob = gl.blob.clone().unwrap_or_default();
    let mut out = vec![];
    for a in gl.animations() {
        let name = a.name().unwrap_or("Motion").to_string();
        let mut t0 = f32::MAX;
        for ch in a.channels() { if let Some(t) = ch.reader(|_| Some(&blob)).read_inputs() { for x in t { t0 = t0.min(x); } } }
        let mut keys: HashMap<String, crate::motion_write::Keys> = HashMap::new();
        for ch in a.channels() {
            let Some(bone) = ch.target().node().name().map(String::from) else { continue };
            let r = ch.reader(|_| Some(&blob));
            let times: Vec<f32> = r.read_inputs().context("channel without times")?.map(|t| t - t0).collect();
            let k = keys.entry(bone).or_default();
            use gltf::animation::util::ReadOutputs;
            match r.read_outputs().context("channel without values")? {
                ReadOutputs::Translations(v) => k.pos = v.zip(&times).map(|(p, &t)| (t, game_p(p))).collect(),
                ReadOutputs::Rotations(v) => {
                    let mut prev: Option<Q> = None;
                    k.rot = v.into_f32().zip(&times).map(|(q, &t)| { let mut g = crate::glb::conv_q(q); if let Some(p) = prev { if g.iter().zip(&p).map(|(a, b)| a * b).sum::<f32>() < 0.0 { g = g.map(|x| -x); } } prev = Some(g); (t, g) }).collect();
                }
                _ => {}
            }
        }
        out.push((name, keys));
    }
    Ok(out)
}

fn bits<const N: usize>(v: [f32; N]) -> [u32; N] { v.map(f32::to_bits) }

/// The actor and its clips.
pub fn build(g: &crate::g3::G3Ctx, spec: &CreatureSpec) -> Result<(CreatureReport, Vec<(String, String, Vec<u8>)>)> {
    let word = spec.word.trim();
    if word.is_empty() || !word.chars().next().unwrap().is_ascii_uppercase() || !word.chars().all(|c| c.is_ascii_alphanumeric()) { bail!("creature word {word:?}: Latin letters and digits, a capital first (Mole, Swampbeast)"); }
    for end in ["Skeleton", "Body_01"] { if g.find("_compiledanimation", &format!("G3_{word}_{end}.xact")).is_some() { bail!("the game has G3_{word}_{end} already: pick another word (to change that one, export its actor)"); } }
    let glb = std::fs::read(&spec.glb).with_context(|| format!("read {}", spec.glb))?;
    let gl = gltf::Gltf::from_slice(&glb).context("read glb")?;
    let (joints, mut notes) = joints(&gl)?;
    let names: Vec<String> = joints.iter().map(|j| j.name.clone()).collect();
    let mut seen = std::collections::HashSet::new();
    if let Some(d) = names.iter().find(|n| !seen.insert(n.to_lowercase())) { bail!("two bones are called {d}"); }
    let bad = check_bones(word, &names);
    if !bad.is_empty() { if spec.lenient { notes.extend(bad.iter().map(|b| format!("bone name: {b}"))); } else { bail!("bone names the game's code does not expect:\n  {}", bad.join("\n  ")); } }
    if !names.iter().any(|n| *n == format!("{word}_ROOT")) { notes.push(format!("no {word}_ROOT: the game's creatures have their skeleton under one")); }
    let now = crate::volume::filetime_now();
    let mut files = vec![];
    let mut r = CreatureReport { bones: joints.len(), ..Default::default() };

    // Nodes: the creature's own identity root for the mesh (unless a bone is called so), then the bones.
    let mut node_names: Vec<String> = vec![];
    let mut sections = vec![Section::Info { version: 1, a: 2, b: 0x10, c: 0, d: 1, exporter: b"Gothic 3 ImpExp".to_vec(), source: format!("{word}.blend").into_bytes(), date: b"Nov  3 2026".to_vec() }];
    let node = |name: &str, parent: &str, pos: [f32; 3], rot: Q| Section::Node { version: 3, node: Node { pos: bits(pos), rot: bits(rot), scale_rot: bits([0.0, 0.0, 0.0, 1.0]), scale: bits([1.0; 3]), more: [0; 3], name: name.as_bytes().to_vec(), parent: parent.as_bytes().to_vec() } };
    let has_mesh = spec.skin.is_some();
    if has_mesh && !names.iter().any(|n| n == word) { sections.push(node(word, "", [0.0; 3], [0.0, 0.0, 0.0, 1.0])); node_names.push(word.to_string()); }
    for j in &joints { sections.push(node(&j.name, j.parent.as_deref().unwrap_or(""), j.pos, j.rot)); node_names.push(j.name.clone()); }
    let index = |n: &str| node_names.iter().position(|x| x.eq_ignore_ascii_case(n));
    for (b, v) in &spec.limits {
        let i = index(b).with_context(|| format!("limits of {b}: no such bone"))?;
        if v.len() != 18 { bail!("limits of {b}: 18 numbers, not {}", v.len()); }
        let mut values = [0u32; 18];
        for (k, x) in v.iter().enumerate() { values[k] = x.to_bits(); }
        sections.push(Section::Limit { version: 1, node: i as u32, values, flags: [0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0] });
    }

    // Materials (the default one first, as every actor of the game), the mesh, its skin.
    let mut table: Vec<Vec<u8>> = vec![];
    let mut extra = Extra { materials: vec![], flag: 1, meshes: vec![] };
    if let Some(skin) = &spec.skin {
        let input = crate::xact_write::read_input(skin)?;
        let mats = crate::xact_write::materials(g, &skin.materials, &[], now, &mut files, &mut r.notes)?;
        let colors = bits([0.0, 0.0, 0.0, 0.75, 0.75, 0.75, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 25.0, 0.0, 1.0, 1.5]);
        sections.push(Section::Material { version: 5, colors, a: 0, b: 70, name: b"EMFX Default".to_vec(), last: 0 });
        for (i, m) in mats.iter().enumerate() {
            let fxa = i as u16 + 1;
            // A game material keeps its own images; a new one has the images just made.
            let (d, n, s) = match g.find("_compiledmaterial", &format!("{}.xshmat", m.name)).and_then(|k| crate::g3_res::parse_xshmat(&g.read(&k).ok()?).ok()) {
                Some(x) if m.diffuse.is_none() => { let (d, n, s) = crate::textures::maps(&x.textures); (d.map(|t| t.trim_end_matches(".tga").to_string()), n.map(|t| t.trim_end_matches(".tga").to_string()), s.map(|t| t.trim_end_matches(".tga").to_string())) }
                _ => (m.diffuse.clone(), m.normal.clone(), m.specular.clone()),
            };
            sections.push(Section::Material { version: 5, colors, a: 0, b: 70, name: m.name.as_bytes().to_vec(), last: 0 });
            for (kind, t) in [(1u8, d), (3, n), (9, s)] { if let Some(t) = t { sections.push(Section::Map { version: 4, kind, b: 0xF7, material: fxa, values: bits([1.0, 0.0, 0.0, 1.0, 1.0, 0.0]), texture: t.into_bytes() }); } }
            extra.materials.push([0, fxa, table.len() as u16]);
            table.push(format!("{}.xshmat", m.name).into_bytes());
        }
        // Corners per material, one per (vertex, normal, uv); Gothic 3's actors turn the other way (corners 1, 2 swap).
        let nmat = mats.len().max(1);
        let mut parts: Vec<(Vec<(u32, [f32; 3], [f32; 3], [f32; 2])>, Vec<[u32; 3]>)> = vec![(vec![], vec![]); nmat];
        let mut seen: Vec<HashMap<(u32, [u32; 5]), u32>> = vec![HashMap::new(); nmat];
        for t in 0..input.corner_vertex.len() / 3 {
            let mi = (input.tri_material[t] as usize).min(nmat - 1);
            let ids = [0, 2, 1].map(|k| {
                let c = t * 3 + k;
                let (v, n, uv) = (input.corner_vertex[c], input.normals[c], input.uvs[c]);
                *seen[mi].entry((v, [n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), uv[0].to_bits(), uv[1].to_bits()])).or_insert_with(|| { parts[mi].0.push((v, input.positions[v as usize], n, uv)); parts[mi].0.len() as u32 - 1 })
            });
            parts[mi].1.push(ids);
        }
        let mesh_node = index(word).unwrap_or(0) as u32;
        let (mut all_p, mut all_n, mut all_uv, mut all_t) = (vec![], vec![], vec![], vec![]);
        let mut mparts = vec![];
        for (mi, (cs, ts)) in parts.iter().enumerate() {
            if ts.is_empty() { continue; }
            let base = all_p.len() as u32;
            mparts.push(Part { head: mi as u32 + 1, corners: cs.iter().map(|(v, p, n, uv)| (*v, p.iter().chain(n).chain(uv).map(|x| x.to_bits()).collect())).collect(), indices: ts.iter().flatten().copied().collect() });
            for (_, p, n, uv) in cs { all_p.push(*p); all_n.push(*n); all_uv.push(*uv); }
            for t in ts { all_t.push(t.map(|i| i + base)); }
        }
        sections.push(Section::Mesh { version: 3, mesh: Mesh { node: mesh_node, vertices: input.positions.len() as u32, uv_sets: 1, word: 0, parts: mparts } });
        let mut missing = std::collections::BTreeSet::new();
        let weights = input.weights.iter().map(|w| {
            let mut v: Vec<(u16, f32)> = w.iter().filter(|x| x.1 > 0.0).filter_map(|(b, x)| match index(b) { Some(i) => Some((i as u16, *x)), None => { missing.insert(b.clone()); None } }).collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            if v.is_empty() { v.push((mesh_node as u16, 1.0)); }
            let s: f32 = v.iter().map(|x| x.1).sum();
            v.into_iter().map(|(n, x)| (n, 0u16, (x / s).to_bits())).collect()
        }).collect();
        if !missing.is_empty() { bail!("vertex groups that are not bones: {}", missing.into_iter().collect::<Vec<_>>().join(", ")); }
        sections.push(Section::Skin { version: 1, skin: Skin { node: mesh_node, weights } });
        let tan = crate::xcmsh_write::tangents(&all_p, &all_n, &all_uv, &all_t);
        extra.meshes.push(CornerData { flag: 1, colors: vec![0xFF00_0000; all_p.len()], tangents: tan.iter().map(|t| { let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-8); [t[0] / l, t[1] / l, t[2] / l].map(f32::to_bits) }).collect() });
        r.vertices = input.positions.len();
        r.triangles = all_t.len();
    }
    for (b, (kind, v)) in &spec.shapes {
        if index(b).is_none() { bail!("shape of {b}: no such bone"); }
        if v.len() != 8 || b.len() >= 36 { bail!("shape of {b}: 8 numbers and a name under 36 letters"); }
        let mut name = [0u8; 36];
        name[..b.len()].copy_from_slice(b.as_bytes());
        let mut values = [0u32; 8];
        for (k, x) in v.iter().enumerate() { values[k] = x.to_bits(); }
        sections.push(Section::Shape { version: 1, name, kind: *kind, values });
    }
    let mut doc = Doc { version: 0x36, word16: 0, priority: 0, filetime: now, source: 0, bbox: [0x7f7f_ffff, 0x7f7f_ffff, 0x7f7f_ffff, 0xff7f_ffff, 0xff7f_ffff, 0xff7f_ffff], records: vec![],
        blocks: vec![Block { gena: 4, fxa_version: [1, 1], sections, extra }], table };
    // The source is the FXA itself, as in every actor compiled since August 2006.
    let fxa = crate::xact_doc::write(&doc);
    doc.source = u32::from_le_bytes(fxa[74 + 0..78].try_into().unwrap());
    let actor = format!("G3_{word}_{}", if has_mesh { "Body_01" } else { "Skeleton" });
    files.push(("_compiledAnimation".to_string(), format!("{actor}.xact"), crate::xact_doc::write(&doc)));
    r.actor = actor;

    // Clips: every action named from the game's words, every bone a track (keys reduced as the game's).
    let pose: HashMap<String, ([f32; 3], Q)> = joints.iter().map(|j| (j.name.to_lowercase(), (j.pos, j.rot))).collect();
    for (action, keys) in animations(&gl)? {
        if spec.actions.as_ref().is_some_and(|a| !a.contains(&action)) { continue; }
        let name = match clip_name(word, &action) { Ok(n) => n, Err(e) => { r.skipped.push(format!("{e:#}")); continue } };
        let keys: HashMap<String, crate::motion_write::Keys> = keys.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
        let tracks: Vec<crate::xmot_doc::NewTrack> = joints.iter().map(|j| {
            let k = keys.get(&j.name.to_lowercase());
            let (bp, br) = pose[&j.name.to_lowercase()];
            crate::xmot_doc::NewTrack { node: j.name.clone(), bind_pos: bp, bind_rot: br, pos: k.map(|k| k.pos.clone()).unwrap_or_default(), rot: k.map(|k| k.rot.clone()).unwrap_or_default() }
        }).collect();
        if keys.is_empty() { r.skipped.push(format!("{action}: no keys on the creature's bones")); continue; }
        files.push(("_compiledAnimation".to_string(), format!("{name}.xmot"), crate::xmot_doc::from_tracks(&tracks, &[], now)));
        r.clips.push(name);
    }
    r.notes.extend(notes);
    r.files = files.iter().map(|(a, p, _)| format!("{a}/{p}")).collect();
    Ok((r, files))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bone_names_are_checked_as_the_game_expects_them() {
        let ok: Vec<String> = ["Mole_ROOT", "Mole_Spine_Spine_1", "Mole_Head_Head_1", "Mole_Left_Arm_Arm_1", "Slot_Head"].map(String::from).to_vec();
        assert!(check_bones("Mole", &ok).is_empty(), "{:?}", check_bones("Mole", &ok));
        let bad: Vec<String> = ["Mole_ROOT", "Mole_Spine_Spine_1", "Wolf_Tail_1", "Mole_LeftArm", "Slot_Hat"].map(String::from).to_vec();
        let b = check_bones("Mole", &bad);
        assert_eq!(b.len(), 4, "{b:?}");
        assert_eq!(clip_name("Mole", "Stand_None_None_P0_Ambient_Loop_N_Fwd_01").unwrap(), "Mole_Stand_None_None_P0_Ambient_Loop_N_Fwd_01_%_00_P0_0");
        assert_eq!(clip_name("Mole", "Wolf_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150").unwrap(), "Mole_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150");
        assert!(clip_name("Mole", "Dig").is_err() && clip_name("Mole", "Stand_None_None_P0_Breakdance_Loop_N_Fwd_01").is_err());
    }

    /// The wolf rebuilt as a new creature from its own glb (as the import writes it) and its skin: the actor reads back
    /// strictly with the wolf's nodes where the wolf has them, its mesh and its weights; its walk reads back as a clip
    /// the game would tie to it.
    #[test]
    fn the_wolf_comes_back_as_a_new_creature() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_creature_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = crate::g3_actor::decode_xact(&g.read("_compiledanimation/g3_wolf_body_01.xact").unwrap()).unwrap();
        let clip = "Wolf_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150";
        let m = crate::g3_motion::decode_xmot(&g.read(&g.find("_compiledanimation", &format!("{clip}.xmot")).unwrap()).unwrap()).unwrap();
        let (glb, _) = crate::glb::build("wolf", &a, &[(clip.to_string(), m.clone())], false, &mut |_: &str, _: bool| Ok(None)).unwrap();
        std::fs::write(dir.join("w.glb"), &glb).unwrap();
        // The wolf's bones keep their names (Wolf_…), so the new word's check is waived.
        let skin = crate::xact_write::tests::own_skin(&a, "G3_Wolf_Body_01", "Wolfling", &dir.join("skin"));
        let spec = CreatureSpec { word: "Wolfling".into(), glb: dir.join("w.glb").to_string_lossy().into_owned(), skin: Some(skin), limits: HashMap::new(), shapes: HashMap::new(), lenient: true, actions: None };
        let (r, files) = build(&g, &spec).unwrap();
        assert_eq!(r.actor, "G3_Wolfling_Body_01");
        let actor = &files.iter().find(|f| f.1 == "G3_Wolfling_Body_01.xact").unwrap().2;
        let doc = crate::xact_doc::read(actor).unwrap();
        assert_eq!(crate::xact_doc::write(&doc), *actor);
        let b = crate::g3_actor::decode_xact(actor).unwrap();
        // Every wolf bone with the wolf's bind pose (glb's f32 metres: within 0.01 cm and 1e-4).
        for n in a.nodes.iter().filter(|n| n.name != "Wolf") {
            let o = b.nodes.iter().find(|x| x.name == n.name).unwrap_or_else(|| panic!("{} lost", n.name));
            assert!((0..3).all(|k| (o.pos[k] - n.pos[k]).abs() < 0.01), "{}: {:?} vs {:?}", n.name, o.pos, n.pos);
            let d: f32 = (0..4).map(|k| o.rot[k] * n.rot[k]).sum();
            assert!(d.abs() > 1.0 - 1e-5, "{}: {:?} vs {:?}", n.name, o.rot, n.rot);
            assert_eq!(o.parent.map(|p| b.nodes[p].name.clone()), n.parent.map(|p| a.nodes[p].name.clone()), "{}", n.name);
        }
        assert_eq!(b.meshes[0].triangles.len(), a.meshes[0].triangles.len());
        assert_eq!(b.meshes[0].weights.len(), a.meshes[0].weights.len());
        // The walk, under the new word, moves every bone as the wolf's (within the reduction).
        assert!(r.clips.iter().any(|c| c == "Wolfling_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_150"), "{:?} {:?}", r.clips, r.skipped);
        let walk = crate::g3_motion::decode_xmot(&files.iter().find(|f| f.1.starts_with("Wolfling_Stand")).unwrap().2).unwrap();
        assert!(walk.tracks.len() >= m.tracks.len());
        assert!((walk.duration - m.duration).abs() < 0.05, "{} vs {}", walk.duration, m.duration);
        std::fs::remove_dir_all(&dir).ok();
    }
}
