//! A Gothic 3 clip (`.xmot`) read into its fields and written back from them, and a clip written from nothing but
//! its tracks.
//!
//! ```text
//! GENOMFLE · u16 1 · u32 table · u16 5 · u32 · u32 priority · FILETIME · u32 source bytes · FILETIME
//! · u16 n · n × (u16 key frame, u16 string)                                (frame effects)
//! · u32 LMA bytes · "LMA " · u8 1 · u8 1 · u8 0 · chunks
//!   chunk  u32 id · u32 size · u32 version · body
//!     1 v3 submotion   f32 pose pos[3] · rot[4] · scale[3] · bind pos[3] · rot[4] · scale[3] · str node
//!     2 v1 keys        u32 n · u8 'L' · u8 'P' | 'R' | 'S' · u16 (left in memory) · n × (f32 time, f32 value[3 or 4])
//! · DEADBEEF · u8 1 · u32 n · n × (u16 length, bytes)                      (the effects' names)
//! ```
//! The game keeps a key only where the motion bends: of its 725 830 key arrays 493 044 are empty (the pose holds),
//! 85 542 have fewer keys than the clip has frames. [`reduce`] drops the keys linear interpolation gives back.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Doc { pub version: u16, pub word: u32, pub priority: u32, pub filetime: u64, pub source: u32, pub filetime2: u64, pub effects: Vec<(u16, u16)>, pub lma: [u8; 3], pub chunks: Vec<Chunk>, pub table: Vec<Vec<u8>> }

#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    Sub { version: u32, values: [u32; 20], node: Vec<u8> },
    Keys { version: u32, interp: u8, kind: u8, pad: u16, width: usize, keys: Vec<Vec<u32>> },
}

struct R<'a> { d: &'a [u8], p: usize }
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> { let s = self.d.get(self.p..self.p + n).with_context(|| format!("clip runs short at 0x{:x}", self.p))?; self.p += n; Ok(s) }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> Result<u64> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
}

pub fn read(d: &[u8]) -> Result<Doc> {
    if !d.starts_with(b"GENOMFLE") || d.get(8..10) != Some(&[1, 0][..]) { bail!("not a GENOMFLE clip"); }
    let mut r = R { d, p: 10 };
    let table = r.u32()? as usize;
    let (version, word, priority, filetime, source, filetime2) = (r.u16()?, r.u32()?, r.u32()?, r.u64()?, r.u32()?, r.u64()?);
    let effects = (0..r.u16()?).map(|_| Ok((r.u16()?, r.u16()?))).collect::<Result<Vec<_>>>()?;
    let len = r.u32()? as usize;
    let end = r.p + len;
    if r.take(4)? != b"LMA " { bail!("no LMA motion at 0x{:x}", r.p - 4); }
    let lma = [r.u8()?, r.u8()?, r.u8()?];
    let mut chunks = vec![];
    while r.p < end {
        let (id, size, version) = (r.u32()?, r.u32()? as usize, r.u32()?);
        let at = r.p;
        chunks.push(match id {
            1 => { let mut values = [0u32; 20]; for v in values.iter_mut() { *v = r.u32()?; } let n = r.u32()? as usize; Chunk::Sub { version, values, node: r.take(n)?.to_vec() } }
            2 => {
                let n = r.u32()? as usize;
                let (interp, kind, pad) = (r.u8()?, r.u8()?, r.u16()?);
                let width = if n == 0 { 0 } else { (size - 8) / n / 4 };
                let keys = (0..n).map(|_| (0..width).map(|_| r.u32()).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;
                Chunk::Keys { version, interp, kind, pad, width, keys }
            }
            id => bail!("chunk {id} at 0x{:x}", at - 12),
        });
        if r.p != at + size { bail!("chunk {id} at 0x{:x}: {} of {size} bytes", at - 12, r.p - at); }
    }
    if r.p != end || r.p != table { bail!("the motion ends at 0x{:x}, the table is at 0x{table:x}", r.p); }
    if r.u32()? != 0xDEADBEEF || r.u8()? != 1 { bail!("no string table"); }
    let mut strings = vec![];
    for _ in 0..r.u32()? { let l = r.u16()? as usize; strings.push(r.take(l)?.to_vec()); }
    if r.p != d.len() { bail!("{} bytes after the string table", d.len() - r.p); }
    Ok(Doc { version, word, priority, filetime, source, filetime2, effects, lma, chunks, table: strings })
}

fn put(o: &mut Vec<u8>, x: u32) { o.extend(x.to_le_bytes()); }

pub fn write(doc: &Doc) -> Vec<u8> {
    let mut o = b"GENOMFLE".to_vec();
    o.extend(1u16.to_le_bytes());
    put(&mut o, 0);
    o.extend(doc.version.to_le_bytes());
    put(&mut o, doc.word);
    put(&mut o, doc.priority);
    o.extend(doc.filetime.to_le_bytes());
    put(&mut o, doc.source);
    o.extend(doc.filetime2.to_le_bytes());
    o.extend((doc.effects.len() as u16).to_le_bytes());
    for (f, s) in &doc.effects { o.extend(f.to_le_bytes()); o.extend(s.to_le_bytes()); }
    let mut m = b"LMA ".to_vec();
    m.extend(doc.lma);
    for c in &doc.chunks {
        let (id, version, body) = match c {
            Chunk::Sub { version, values, node } => { let mut b = vec![]; for v in values { put(&mut b, *v); } put(&mut b, node.len() as u32); b.extend(node); (1u32, *version, b) }
            Chunk::Keys { version, interp, kind, pad, keys, .. } => { let mut b = vec![]; put(&mut b, keys.len() as u32); b.push(*interp); b.push(*kind); b.extend(pad.to_le_bytes()); for k in keys { for v in k { put(&mut b, *v); } } (2, *version, b) }
        };
        put(&mut m, id); put(&mut m, body.len() as u32); put(&mut m, version); m.extend(body);
    }
    put(&mut o, m.len() as u32);
    o.extend(m);
    let table = o.len() as u32;
    o[10..14].copy_from_slice(&table.to_le_bytes());
    put(&mut o, 0xDEADBEEF);
    o.push(1);
    put(&mut o, doc.table.len() as u32);
    for s in &doc.table { o.extend((s.len() as u16).to_le_bytes()); o.extend(s); }
    o
}

/// The keys of a track that linear interpolation between the kept ones gives back within `tol` (per component);
/// the first and the last stay. A track whose every key is the first (within `tol`) keeps none: the pose holds.
pub fn reduce<const N: usize>(keys: &[(f32, [f32; N])], tol: f32) -> Vec<(f32, [f32; N])> {
    if keys.len() < 2 { return keys.to_vec(); }
    if keys.iter().all(|k| k.1.iter().zip(&keys[0].1).all(|(a, b)| (a - b).abs() <= tol)) { return vec![]; }
    let mut out = vec![keys[0]];
    let mut a = 0;
    while a < keys.len() - 1 {
        // The farthest key b such that every key between a and b lies on the line a-b.
        let mut b = a + 1;
        while b + 1 < keys.len() {
            let c = b + 1;
            let fits = (a + 1..c).all(|i| { let t = if keys[c].0 > keys[a].0 { (keys[i].0 - keys[a].0) / (keys[c].0 - keys[a].0) } else { 0.0 }; (0..N).all(|k| (keys[a].1[k] + (keys[c].1[k] - keys[a].1[k]) * t - keys[i].1[k]).abs() <= tol) });
            if !fits { break; }
            b = c;
        }
        out.push(keys[b]);
        a = b;
    }
    out
}

/// One node's motion for a new clip: pose (the first frame), the actor's bind pose, and keys per frame.
pub struct NewTrack { pub node: String, pub bind_pos: [f32; 3], pub bind_rot: [f32; 4], pub pos: Vec<(f32, [f32; 3])>, pub rot: Vec<(f32, [f32; 4])> }

/// Tolerances of the key reduction: centimetres, quaternion components.
pub const POS_TOL: f32 = 0.01;
pub const ROT_TOL: f32 = 1e-5;

/// A clip written from its tracks alone, as the game writes one: a submotion and the reduced position, rotation keys
/// per node, frame effects `(key frame, name)` with their names in the table.
pub fn from_tracks(tracks: &[NewTrack], effects: &[(u16, String)], filetime: u64) -> Vec<u8> {
    let b = |x: f32| x.to_bits();
    let mut chunks = vec![];
    for t in tracks {
        let pose_pos = t.pos.first().map(|k| k.1).unwrap_or(t.bind_pos);
        let pose_rot = t.rot.first().map(|k| k.1).unwrap_or(t.bind_rot);
        let mut values = [0u32; 20];
        let one = [1f32; 3];
        for (i, x) in pose_pos.iter().chain(&pose_rot).chain(&one).chain(&t.bind_pos).chain(&t.bind_rot).chain(&one).enumerate() { values[i] = b(*x); }
        chunks.push(Chunk::Sub { version: 3, values, node: t.node.as_bytes().to_vec() });
        let pos = reduce(&t.pos, POS_TOL);
        let rot = reduce(&t.rot, ROT_TOL);
        if !pos.is_empty() { chunks.push(Chunk::Keys { version: 1, interp: b'L', kind: b'P', pad: 0, width: 4, keys: pos.iter().map(|(tm, v)| [*tm, v[0], v[1], v[2]].map(b).to_vec()).collect() }); }
        if !rot.is_empty() { chunks.push(Chunk::Keys { version: 1, interp: b'L', kind: b'R', pad: 0, width: 5, keys: rot.iter().map(|(tm, v)| [*tm, v[0], v[1], v[2], v[3]].map(b).to_vec()).collect() }); }
    }
    let mut table: Vec<Vec<u8>> = vec![];
    let fx = effects.iter().map(|(f, n)| { let i = table.iter().position(|s| s == n.as_bytes()).unwrap_or_else(|| { table.push(n.as_bytes().to_vec()); table.len() - 1 }); (*f, i as u16) }).collect();
    let mut doc = Doc { version: 5, word: 0, priority: 0, filetime, source: 0, filetime2: filetime, effects: fx, lma: [1, 1, 0], chunks, table };
    let lma = write(&doc).len();
    doc.source = lma as u32;
    write(&doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every clip of the game, read into its fields and written from them, is the same file to the byte.
    #[test]
    fn every_clip_of_the_game_writes_back_to_the_byte() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let mut n = 0;
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xmot")) {
            let d = g.read(&k).unwrap();
            let doc = read(&d).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert!(write(&doc) == d, "{k}");
            n += 1;
        }
        assert_eq!(n, 5752);
    }

    /// Every clip of the game written again from its tracks alone (sampled at its own key step, reduced as the game
    /// does) reads back strictly and moves every node as the game's within the reduction's bound: the largest
    /// difference of a sampled position and rotation over all clips, and how many keys ours keeps against the game's.
    #[test]
    fn every_clip_written_from_its_tracks_moves_as_the_games() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (mut worst_p, mut worst_r, mut ours, mut theirs, mut n) = (0f32, 0f32, 0usize, 0usize, 0);
        let lerp = |k: &[(f32, Vec<f32>)], t: f32, pose: &[f32]| -> Vec<f32> {
            if k.is_empty() { return pose.to_vec(); }
            if t <= k[0].0 { return k[0].1.clone(); }
            for w in k.windows(2) { if t <= w[1].0 { let f = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 1.0 }; return w[0].1.iter().zip(&w[1].1).map(|(a, b)| a + (b - a) * f).collect(); } }
            k.last().unwrap().1.clone()
        };
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xmot")) {
            let m = crate::g3_motion::decode_xmot(&g.read(&k).unwrap()).unwrap();
            let step = crate::g3_motion::key_step(&m);
            let frames = (m.duration / step).round() as usize + 1;
            let dense = |t: &crate::g3_motion::G3Track| -> NewTrack {
                let (pk, rk): (Vec<(f32, Vec<f32>)>, Vec<(f32, Vec<f32>)>) = (t.pos.iter().map(|x| (x.0, x.1.to_vec())).collect(), t.rot.iter().map(|x| (x.0, x.1.to_vec())).collect());
                let at = |i: usize| i as f32 * step;
                NewTrack { node: t.bone.clone(), bind_pos: t.pose_pos, bind_rot: t.pose_rot,
                    pos: if t.pos.is_empty() { vec![] } else { (0..frames).map(|i| (at(i), <[f32; 3]>::try_from(lerp(&pk, at(i), &t.pose_pos)).unwrap())).collect() },
                    rot: if t.rot.is_empty() { vec![] } else { (0..frames).map(|i| { let q = lerp(&rk, at(i), &t.pose_rot); let l = q.iter().map(|x| x * x).sum::<f32>().sqrt(); (at(i), [q[0] / l, q[1] / l, q[2] / l, q[3] / l]) }).collect() } }
            };
            let tracks: Vec<NewTrack> = m.tracks.iter().map(dense).collect();
            let bytes = from_tracks(&tracks, &[], 0);
            let doc = read(&bytes).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert_eq!(write(&doc), bytes, "{k}");
            let back = crate::g3_motion::decode_xmot(&bytes).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert_eq!(back.tracks.len(), m.tracks.len(), "{k}");
            for (a, b) in m.tracks.iter().zip(&back.tracks) {
                let (pa, pb): (Vec<(f32, Vec<f32>)>, Vec<(f32, Vec<f32>)>) = (a.pos.iter().map(|x| (x.0, x.1.to_vec())).collect(), b.pos.iter().map(|x| (x.0, x.1.to_vec())).collect());
                let (ra, rb): (Vec<(f32, Vec<f32>)>, Vec<(f32, Vec<f32>)>) = (a.rot.iter().map(|x| (x.0, x.1.to_vec())).collect(), b.rot.iter().map(|x| (x.0, x.1.to_vec())).collect());
                for i in 0..frames {
                    let t = i as f32 * step;
                    let (x, y) = (lerp(&pa, t, &a.pose_pos), lerp(&pb, t, &b.pose_pos));
                    worst_p = worst_p.max(x.iter().zip(&y).map(|(p, q)| (p - q).abs()).fold(0.0, f32::max));
                    let (x, y) = (lerp(&ra, t, &a.pose_rot), lerp(&rb, t, &b.pose_rot));
                    let lx = x.iter().map(|v| v * v).sum::<f32>().sqrt(); let ly = y.iter().map(|v| v * v).sum::<f32>().sqrt();
                    let dot = x.iter().zip(&y).map(|(p, q)| p * q).sum::<f32>() / (lx * ly);
                    worst_r = worst_r.max(2.0 * dot.abs().min(1.0).acos());
                }
                ours += b.pos.len() + b.rot.len();
                theirs += a.pos.len() + a.rot.len();
            }
            n += 1;
        }
        eprintln!("{n} clips from their tracks: worst position {worst_p:.4} cm, rotation {:.4}°; keys ours {ours}, the game's {theirs}", worst_r.to_degrees());
        assert!(worst_p < 0.05 && worst_r.to_degrees() < 0.1, "{worst_p} cm, {}°", worst_r.to_degrees());
    }
}
