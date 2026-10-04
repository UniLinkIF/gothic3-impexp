//! Writing a Gothic 3 motion (`.xmot`): a clip of the game rewritten with new keys.
//!
//! The game's clip is the template: its GENOMFLE head (resource properties and frame effects) and its tracks
//! stay; for every bone that has a track there, the position and rotation keys become the new
//! ones and the pose (the first frame) follows them. Scale tracks and bones without new keys stay as they were.
//!
//! ```text
//! 0   "GENOMFLE" · u16 1 · u32 string-table offset · … · u32 LMA bytes (at the LMA start − 4)
//!     "LMA " u8 1 · u8 1 · u8 0 · chunks · string table
//! ```

use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// New keys of one bone, in game space: (time s, local position cm), (time s, local rotation x y z w).
#[derive(Default, Debug, Clone)]
pub struct Keys { pub pos: Vec<(f32, [f32; 3])>, pub rot: Vec<(f32, [f32; 4])> }

fn u32_at(d: &[u8], p: usize) -> u32 { u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) }

const LINEAR: u8 = b'L';

fn chunk(o: &mut Vec<u8>, id: u32, version: u32, body: &[u8]) {
    o.extend(id.to_le_bytes());
    o.extend((body.len() as u32).to_le_bytes());
    o.extend(version.to_le_bytes());
    o.extend(body);
}

fn track(kind: u8, keys: impl Iterator<Item = (f32, Vec<f32>)>) -> Vec<u8> {
    let keys: Vec<(f32, Vec<f32>)> = keys.collect();
    let mut b = vec![];
    b.extend((keys.len() as u32).to_le_bytes());
    b.extend([LINEAR, kind, 0, 0]);
    for (t, v) in keys { b.extend(t.to_le_bytes()); for x in v { b.extend(x.to_le_bytes()); } }
    b
}

/// `template` (a game clip) with `keys` (lowercase bone name -> keys) put in. Returns the file and the number of
/// bones that got new keys.
pub fn rewrite(template: &[u8], keys: &HashMap<String, Keys>, effects: Option<&[(f32, String)]>) -> Result<(Vec<u8>, usize)> {
    if !template.starts_with(b"GENOMFLE") { bail!("the clip is not a GENOMFLE resource"); }
    let lma = template.windows(4).take(512).position(|w| w == b"LMA ").filter(|&p| p >= 4).context("no LMA motion in the clip")?;
    let lma_len = u32_at(template, lma - 4) as usize;
    let end = lma + lma_len;
    let table = u32_at(template, 10) as usize;
    if end > template.len() || table > template.len() { bail!("clip sizes do not fit"); }
    // Chunks: (offset, id, body size, version).
    let mut cs = vec![];
    let mut p = lma + 7;
    while p + 12 <= end {
        let (id, size, ver) = (u32_at(template, p), u32_at(template, p + 4) as usize, u32_at(template, p + 8));
        if p + 12 + size > end { bail!("chunk {id} at 0x{p:x} runs past the motion"); }
        cs.push((p, id, size, ver));
        p += 12 + size;
    }
    let key_size = |p: usize, size: usize| -> usize { let n = u32_at(template, p + 12) as usize; if n == 0 { 0 } else { (size - 8) / n } };

    let mut body = template[lma..lma + 7].to_vec();
    let mut changed = 0;
    let mut i = 0;
    while i < cs.len() {
        let (p, id, size, ver) = cs[i];
        let mut j = i + 1;
        while j < cs.len() && cs[j].1 != 1 { j += 1; }
        let whole = |a: usize, b: usize| -> &[u8] { let s = cs[a].0; let e = if b < cs.len() { cs[b].0 } else { end }; &template[s..e] };
        if id != 1 {
            body.extend(whole(i, j));
            i = j;
            continue;
        }
        let b = p + 12;
        let n = u32_at(template, b + 80) as usize;
        let bone = String::from_utf8_lossy(&template[b + 84..b + 84 + n]).to_lowercase();
        let Some(k) = keys.get(&bone).filter(|k| !k.pos.is_empty() || !k.rot.is_empty()) else {
            body.extend(whole(i, j));
            i = j;
            continue;
        };
        changed += 1;
        // The submotion with its pose = the new first frame.
        let mut sub = template[b..b + size].to_vec();
        if let Some((_, v)) = k.pos.first() { for c in 0..3 { sub[c * 4..c * 4 + 4].copy_from_slice(&v[c].to_le_bytes()); } }
        if let Some((_, q)) = k.rot.first() { for c in 0..4 { sub[12 + c * 4..16 + c * 4].copy_from_slice(&q[c].to_le_bytes()); } }
        chunk(&mut body, 1, ver, &sub);
        // Position and rotation tracks (in EMotionFX's order), then the template's scale tracks as they were.
        let kind = |tp: usize| template[tp + 17];
        let old = |c: u8| -> Vec<u8> { cs[i + 1..j].iter().filter(|&&(tp, _, ts, _)| key_size(tp, ts) > 0 && kind(tp) == c).flat_map(|&(tp, _, ts, _)| template[tp..tp + 12 + ts].to_vec()).collect() };
        if !k.pos.is_empty() { chunk(&mut body, 2, 1, &track(b'P', k.pos.iter().map(|(t, v)| (*t, v.to_vec())))); } else { body.extend(old(b'P')); }
        if !k.rot.is_empty() { chunk(&mut body, 2, 1, &track(b'R', k.rot.iter().map(|(t, q)| (*t, q.to_vec())))); } else { body.extend(old(b'R')); }
        body.extend(old(b'S'));
        for &(tp, _, ts, _) in &cs[i + 1..j] { if key_size(tp, ts) == 0 { body.extend(&template[tp..tp + 12 + ts]); } }
        i = j;
    }
    let Some(fx) = effects else {
        let mut out = template[..lma - 4].to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(&body);
        let new_table = out.len() as u32;
        out.extend(&template[table.max(end)..]);
        out[10..14].copy_from_slice(&new_table.to_le_bytes());
        return Ok((out, changed));
    };
    // New frame effects: the head up to the effect count, the effects, the motion, the string table with their names.
    if template.len() < 46 || u16::from_le_bytes([template[14], template[15]]) < 5 { bail!("the clip's head is older than version 5"); }
    let step = crate::g3_motion::decode_xmot(template).map(|m| crate::g3_motion::key_step(&m)).unwrap_or(1.0 / 30.0);
    let mut strings = crate::g3_res::genomfle_strings(template).unwrap_or_default();
    let mut out = template[..44].to_vec();
    out.extend((fx.len() as u16).to_le_bytes());
    for (t, name) in fx {
        let i = match strings.iter().position(|s| s == name) { Some(i) => i, None => { strings.push(name.clone()); strings.len() - 1 } };
        out.extend(((t / step).round().clamp(0.0, 65535.0) as u16).to_le_bytes());
        out.extend((i as u16).to_le_bytes());
    }
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(&body);
    let new_table = out.len() as u32;
    out.extend(0xDEADBEEFu32.to_le_bytes());
    out.push(1);
    out.extend((strings.len() as u32).to_le_bytes());
    for s in &strings { out.extend((s.len() as u16).to_le_bytes()); out.extend(s.as_bytes()); }
    out[10..14].copy_from_slice(&new_table.to_le_bytes());
    Ok((out, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip rewritten with its own keys decodes to the same tracks; rewritten with shifted keys, the shift shows.
    #[test]
    fn clips_take_new_keys() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let k = g.find("_compiledanimation", "Wolf_Stand_None_Fist_P0_Move_Run_N_Fwd_00_%_00_P0_400.xmot").unwrap();
        let d = g.read(&k).unwrap();
        let m = crate::g3_motion::decode_xmot(&d).unwrap();
        let same: HashMap<String, Keys> = m.tracks.iter().map(|t| (t.bone.to_lowercase(), Keys { pos: t.pos.clone(), rot: t.rot.clone() })).collect();
        let (w, n) = rewrite(&d, &same, None).unwrap();
        // Same layout to the byte (values differ only where the reader normalised a quaternion).
        assert_eq!(w.len(), d.len());
        assert!(n > 10);
        let back = crate::g3_motion::decode_xmot(&w).unwrap();
        assert_eq!(back.tracks.len(), m.tracks.len());
        for (a, b) in m.tracks.iter().zip(&back.tracks) {
            assert_eq!(a.bone, b.bone);
            assert_eq!((a.pos.len(), a.rot.len(), a.scale.len()), (b.pos.len(), b.rot.len(), b.scale.len()), "{}", a.bone);
        }
        let mut up = same.clone();
        let t0 = &m.tracks.iter().find(|t| !t.pos.is_empty()).unwrap().bone;
        for k in up.get_mut(&t0.to_lowercase()).unwrap().pos.iter_mut() { k.1[1] += 10.0; }
        let (w2, _) = rewrite(&d, &up, None).unwrap();
        let back2 = crate::g3_motion::decode_xmot(&w2).unwrap();
        let a = &m.tracks.iter().find(|t| &t.bone == t0).unwrap().pos;
        let b = &back2.tracks.iter().find(|t| &t.bone == t0).unwrap().pos;
        assert!(a.iter().zip(b).all(|(x, y)| (y.1[1] - x.1[1] - 10.0).abs() < 1e-4));
        // The GENOMFLE head and table stay, sizes follow.
        assert_eq!(&w[..10], &d[..10]);
    }
}

/// The first animation of a glb exported by Blender, as game-space keys per bone (lowercase name), times from 0.
pub fn keys_from_glb(glb: &[u8]) -> Result<(String, HashMap<String, Keys>, f32)> {
    let g = gltf::Gltf::from_slice(glb).context("read glb")?;
    let blob = g.blob.clone().context("glb has no binary chunk")?;
    let a = g.animations().next().context("the glb has no animation")?;
    let name = a.name().unwrap_or("Motion").to_string();
    let mut t0 = f32::MAX;
    let mut t1 = 0f32;
    for ch in a.channels() { if let Some(t) = ch.reader(|_| Some(&blob)).read_inputs() { for x in t { t0 = t0.min(x); t1 = t1.max(x); } } }
    if t0 == f32::MAX { bail!("animation {name} has no keys"); }
    let mut out: HashMap<String, Keys> = HashMap::new();
    for ch in a.channels() {
        let Some(bone) = ch.target().node().name().map(|n| n.to_lowercase()) else { continue };
        let r = ch.reader(|_| Some(&blob));
        let times: Vec<f32> = r.read_inputs().context("channel without times")?.map(|t| t - t0).collect();
        let k = out.entry(bone).or_default();
        use gltf::animation::util::ReadOutputs;
        match r.read_outputs().context("channel without values")? {
            ReadOutputs::Translations(v) => k.pos = v.zip(&times).map(|(p, &t)| (t, [p[0] * 100.0, p[1] * 100.0, -p[2] * 100.0])).collect(),
            ReadOutputs::Rotations(v) => {
                let mut prev: Option<[f32; 4]> = None;
                k.rot = v.into_f32().zip(&times).map(|(q, &t)| {
                    let mut g = crate::glb::conv_q(q);
                    if let Some(p) = prev { if g.iter().zip(&p).map(|(a, b)| a * b).sum::<f32>() < 0.0 { g = g.map(|x| -x); } }
                    prev = Some(g);
                    (t, g)
                }).collect();
            }
            _ => {}
        }
    }
    Ok((name, out, t1 - t0))
}

/// What the add-on asks for: the game clip that is the template (and is replaced unless `as_name` is given), the glb, and
/// optionally new frame effects (time s, name).
#[derive(serde::Deserialize)]
pub struct MotionSpec { pub clip: String, pub glb: String, #[serde(default)] pub as_name: Option<String>, #[serde(default)] pub effects: Option<Vec<(f32, String)>> }

#[derive(serde::Serialize)]
pub struct Report { pub clip: String, pub template: String, pub new: bool, pub bones: usize, pub keys: usize, pub duration: f32, pub effects: usize, pub path: String }

/// The clip for the mod: the template clip with the glb's keys, under its own name or `as_name` (a new clip beside the
/// game's, named the way the game names them so the animation system can pick it).
pub fn build(g: &crate::g3::G3Ctx, spec: &MotionSpec) -> Result<(Report, (String, String, Vec<u8>))> {
    let clip = &spec.clip;
    let key = g.find("_compiledanimation", &format!("{}.xmot", clip.trim_end_matches(".xmot"))).with_context(|| format!("{clip}: no such clip in Gothic 3"))?;
    let (_, keys, duration) = keys_from_glb(&std::fs::read(&spec.glb).with_context(|| format!("read {}", spec.glb))?)?;
    let template = g.read(&key)?;
    let (bytes, bones) = rewrite(&template, &keys, spec.effects.as_deref())?;
    let effects = match &spec.effects { Some(f) => f.len(), None => crate::g3_motion::frame_effects(&template).map(|f| f.len()).unwrap_or(0) };
    if bones == 0 { bail!("none of the clip's bones is in the exported animation — is it the right armature?"); }
    let template_path = g.path_of(&key).unwrap_or(&key).to_string();
    let new = spec.as_name.as_deref().filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(&crate::stem_of(&template_path)));
    if let Some(n) = new { if !n.chars().all(|c| c.is_ascii_alphanumeric() || "_%-".contains(c)) { bail!("clip name {n:?}: letters, digits, _ % - only"); } }
    let path = match new { Some(n) => format!("{n}.xmot"), None => template_path.clone() };
    let n = keys.values().map(|k| k.pos.len() + k.rot.len()).sum();
    Ok((Report { clip: crate::stem_of(&path), template: crate::stem_of(&template_path), new: new.is_some(), bones, keys: n, duration, effects, path: format!("_compiledAnimation/{path}") }, ("_compiledAnimation".to_string(), path, bytes)))
}
