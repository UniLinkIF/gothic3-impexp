//! Lip-sync of Gothic 3's voice lines on the faces of its animated heads.
//!
//! Gothic 3 has no face shapes (no actor carries a morph section): its 28 `G3_Head_*_Animated_*` heads have face bones
//! (`*_Lip_*`, `*_Jaw_*`, `*_EyeBrown_*`, 22 keyed in all), and next to each a clip `<head>_LipSync.xmot` of 15 poses,
//! one every 0.04 s. A voice line (`Speech_<Language>.pak`: `<Language>_ogg/<line>_<Language>.ogg`) has its timeline
//! beside it in `<Language>_LipSyncData/<line>_<Language>.xlip`:
//!
//! ```text
//! GENOMFLE · u16 1 · u32 table · u16 1 · u32 n · n × (f32 start s, u32 phone 1..13) · table (DEADBEEF 01 u32 0)
//! ```
//! (every line read: count, rising times, phones 1–13). A phone p shows pose p of the head's clip: pose 1 is the rest
//! pose (the same as pose 0) and phone 1 the most frequent one, the pauses; that reading is ours, not the game's code.

use crate::g3_motion::{G3Motion, G3Track};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Seconds between the poses of a `_LipSync` clip.
pub const POSE_STEP: f32 = 0.04;

/// The speech volumes of `lang` (`Speech_English.pak`, then its patches), game folder `root`.
fn volumes(root: &Path, lang: &str) -> Result<Vec<PathBuf>> {
    let dir = root.join("Data");
    let stem = format!("speech_{}", lang.to_lowercase());
    let mut v: Vec<(i32, PathBuf)> = std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))?.flatten().filter_map(|e| {
        let p = e.path();
        let (s, x) = (p.file_stem()?.to_string_lossy().to_lowercase(), p.extension()?.to_string_lossy().to_lowercase());
        if s != stem { return None; }
        if x == "pak" { Some((-1, p)) } else { x.strip_prefix('p').and_then(|n| n.parse().ok()).map(|n| (n, p)) }
    }).collect();
    v.sort();
    if !v.iter().any(|x| x.0 == -1) { bail!("Gothic 3 has no {lang} speech here (Data\\Speech_{lang}.pak)"); }
    Ok(v.into_iter().map(|x| x.1).collect())
}

/// Every file of the speech volumes of `lang`, by lowercase path: (volume, entry); patches over the archive.
fn entries(root: &Path, lang: &str) -> Result<std::collections::BTreeMap<String, (PathBuf, crate::g3::G3Entry)>> {
    let mut m = std::collections::BTreeMap::new();
    for v in volumes(root, lang)? {
        for e in crate::g3::read_volume(&v)? {
            let k = e.path.to_lowercase().replace('\\', "/");
            if e.is_deleted() { m.remove(&k); } else { m.insert(k, (v.clone(), e)); }
        }
    }
    Ok(m)
}

/// The voice lines of `lang` that have a lip-sync timeline (names without the language).
pub fn lines(root: &Path, lang: &str) -> Result<Vec<String>> {
    let end = format!("_{}.xlip", lang.to_lowercase());
    Ok(entries(root, lang)?.values().filter(|(_, e)| e.path.to_lowercase().ends_with(&end))
        .map(|(_, e)| { let s = crate::stem_of(&e.path.replace('\\', "/")); s[..s.len() - lang.len() - 1].to_string() }).collect())
}

/// A `.xlip` timeline: (start s, phone).
pub fn parse_xlip(d: &[u8]) -> Result<Vec<(f32, u32)>> {
    if !d.starts_with(b"GENOMFLE") || d.len() < 20 { bail!("not a GENOMFLE lip-sync file"); }
    let u = |p: usize| u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
    let (table, n) = (u(10) as usize, u(16) as usize);
    if u16::from_le_bytes([d[14], d[15]]) != 1 || 20 + n * 8 != table || table > d.len() { bail!("lip-sync timeline of {n} phones does not fit ({} bytes)", d.len()); }
    Ok((0..n).map(|i| (f32::from_bits(u(20 + i * 8)), u(24 + i * 8))).collect())
}

/// The value of a key track at time `t` (linear between keys; rotations normalised).
fn sample<const N: usize>(keys: &[(f32, [f32; N])], t: f32) -> Option<[f32; N]> {
    let first = keys.first()?;
    if t <= first.0 { return Some(first.1); }
    for w in keys.windows(2) {
        let ((t0, a), (t1, b)) = (w[0], w[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            // Rotations the short way round.
            let s = if N == 4 && a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>() < 0.0 { -1.0 } else { 1.0 };
            let mut v = [0f32; N];
            for k in 0..N { v[k] = a[k] * (1.0 - f) + b[k] * s * f; }
            if N == 4 { let l = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-8); for x in v.iter_mut() { *x /= l; } }
            return Some(v);
        }
    }
    Some(keys.last()?.1)
}

/// The head's poses put on a timeline: each keyed track of the `_LipSync` clip gets, at every phone's start, the pose
/// that phone shows, and holds the last one for `tail` s. Tracks without keys keep their pose.
pub fn motion(clip: &G3Motion, phones: &[(f32, u32)], tail: f32) -> G3Motion {
    let poses = ((clip.duration / POSE_STEP).round() as u32).max(1);
    let at = |p: u32| p.min(poses) as f32 * POSE_STEP;
    let mut out = G3Motion { tracks: vec![], duration: phones.last().map(|x| x.0 + tail).unwrap_or(0.0), effects: vec![] };
    for t in &clip.tracks {
        let mut n = G3Track { bone: t.bone.clone(), pose_pos: t.pose_pos, pose_rot: t.pose_rot, ..Default::default() };
        for &(time, p) in phones {
            if let Some(v) = sample(&t.pos, at(p)) { n.pos.push((time, v)); }
            if let Some(v) = sample(&t.rot, at(p)) { n.rot.push((time, v)); }
        }
        if let (Some(&(_, v)), Some(&(time, _))) = (n.pos.last(), phones.last()) { n.pos.push((time + tail, v)); }
        if let (Some(&(_, v)), Some(&(time, _))) = (n.rot.last(), phones.last()) { n.rot.push((time + tail, v)); }
        out.tracks.push(n);
    }
    out
}

#[derive(serde::Serialize)]
pub struct LipOut { pub line: String, pub head: String, pub phones: usize, pub duration: f32, pub glb: String, pub sound: Option<String>, pub face_bones: Vec<String> }

/// Voice line `line` (language `lang`) on head actor `head`: a glb of the head with the lip-sync as its animation, and
/// the voice as `.ogg`, both into `out_dir`. The glb animates every track of the clip (the rest as the pose the face
/// bones hang from); `face_bones` are the ones the line moves.
pub fn build(g: &crate::g3::G3Ctx, head: &str, line: &str, lang: &str, out_dir: &Path) -> Result<LipOut> {
    let head = head.trim_end_matches(".xact");
    let hk = g.find("_compiledanimation", &format!("{head}.xact")).with_context(|| format!("{head}: no such head in Gothic 3"))?;
    let head_name = crate::stem_of(g.path_of(&hk).unwrap_or(&hk));
    let ck = g.find("_compiledanimation", &format!("{head_name}_LipSync.xmot"))
        .with_context(|| format!("{head_name} has no face bones to move (only the G3_Head_…_Animated_… heads have them, each with its own _LipSync poses)"))?;
    let actor = crate::g3_actor::decode_xact(&g.read(&hk)?).with_context(|| hk.clone())?;
    let clip = crate::g3_motion::decode_xmot(&g.read(&ck)?).with_context(|| ck.clone())?;
    let files = entries(&g.root, lang)?;
    let line = line.trim_end_matches(".xlip");
    let base = line.strip_suffix(&format!("_{lang}")).unwrap_or(line);
    let get = |dir: &str, ext: &str| files.get(&format!("{}_{dir}/{}_{}.{ext}", lang, base, lang).to_lowercase());
    let (vol, e) = get("LipSyncData", "xlip").with_context(|| format!("{base}: no {lang} lip-sync for this line"))?;
    let phones = parse_xlip(&crate::g3::read_entry(vol, e)?).with_context(|| e.path.clone())?;
    std::fs::create_dir_all(out_dir)?;
    let sound = match get("ogg", "ogg") {
        Some((v, e)) => { let p = out_dir.join(format!("{base}_{lang}.ogg")); std::fs::write(&p, crate::g3::read_entry(v, e)?)?; Some(p.to_string_lossy().into_owned()) }
        None => None,
    };
    let m = motion(&clip, &phones, 0.3);
    let face_bones = m.tracks.iter().filter(|t| !t.rot.is_empty() || !t.pos.is_empty()).map(|t| t.bone.clone()).collect();
    let (bytes, _) = crate::glb::build(&head_name, &actor, &[(base.to_string(), m.clone())], false, &mut |_: &str, _: bool| Ok(None))?;
    let glb = out_dir.join(format!("{base}.glb"));
    std::fs::write(&glb, bytes)?;
    Ok(LipOut { line: base.to_string(), head: head_name, phones: phones.len(), duration: m.duration, glb: glb.to_string_lossy().into_owned(), sound, face_bones })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every English line's timeline reads (count, rising times, phones 1–13), and every lip-sync clip has its 15 poses
    /// 0.04 s apart.
    #[test]
    fn every_timeline_reads_and_every_head_has_its_poses() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let root = Path::new(&root);
        let files = entries(root, "English").unwrap();
        let mut n = 0;
        let mut phones = std::collections::BTreeSet::new();
        for (v, e) in files.values().filter(|(_, e)| e.path.to_lowercase().ends_with(".xlip")) {
            let t = parse_xlip(&crate::g3::read_entry(v, e).unwrap()).unwrap_or_else(|x| panic!("{}: {x:#}", e.path));
            assert!(t.windows(2).all(|w| w[0].0 <= w[1].0), "{}", e.path);
            phones.extend(t.iter().map(|x| x.1));
            n += 1;
        }
        assert!(n > 20000, "{n} timelines");
        assert_eq!(phones.into_iter().collect::<Vec<_>>(), (1..=13).collect::<Vec<u32>>());
        let g = crate::g3::G3Ctx::open_original(root).unwrap();
        let clips: Vec<String> = g.keys().into_iter().filter(|k| k.ends_with("_lipsync.xmot")).collect();
        assert_eq!(clips.len(), 28);
        for k in &clips {
            let m = crate::g3_motion::decode_xmot(&g.read(k).unwrap()).unwrap();
            assert!((m.duration - 14.0 * POSE_STEP).abs() < 1e-4, "{k}: {}", m.duration);
            assert_eq!(m.tracks.iter().map(|t| t.rot.len()).max(), Some(15), "{k}");
        }
    }

    /// A voice line on the hero's animated head: every keyed face bone gets a key per phone, each the pose of its phone.
    #[test]
    fn a_line_moves_the_face_pose_by_pose() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(Path::new(&root)).unwrap();
        let line = lines(&g.root, "English").unwrap().into_iter().find(|l| l.starts_with("INFO_Ass_Elite_Hard_Bandaro")).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_lip_{}", std::process::id()));
        let out = build(&g, "G3_Head_Hero_Hero_Animated_01", &line, "English", &dir).unwrap();
        assert!(out.phones > 5 && out.face_bones.len() >= 20 && out.sound.as_deref().is_some_and(|s| std::fs::metadata(s).unwrap().len() > 1000), "{} phones {:?}", out.phones, out.face_bones);
        let clip = crate::g3_motion::decode_xmot(&g.read(&g.find("_compiledanimation", "G3_Head_Hero_Hero_Animated_01_LipSync.xmot").unwrap()).unwrap()).unwrap();
        let xlip = { let f = entries(&g.root, "English").unwrap(); let (v, e) = f.get(&format!("english_lipsyncdata/{}_english.xlip", line.to_lowercase())).unwrap().clone(); parse_xlip(&crate::g3::read_entry(&v, &e).unwrap()).unwrap() };
        let m = motion(&clip, &xlip, 0.3);
        let full: Vec<&G3Track> = clip.tracks.iter().filter(|t| t.rot.len() == 15).collect();
        for src in &full {
            let ours = m.tracks.iter().find(|t| t.bone == src.bone).unwrap();
            assert_eq!(ours.rot.len(), xlip.len() + 1);
            for (k, &(t, p)) in ours.rot.iter().zip(&xlip) { assert_eq!(k.0, t); assert!(k.1.iter().zip(&src.rot[p as usize].1).all(|(a, b)| (a - b).abs() < 1e-5), "{} phone {p}", src.bone); }
        }
        // Pose 1, what the pauses (phone 1) show, is the rest pose as pose 0.
        let rest = full.iter().filter(|t| t.rot[0].1.iter().zip(&t.rot[1].1).all(|(a, b)| (a - b).abs() < 1e-4)).count();
        assert_eq!(rest, full.len(), "rest pose = pose 1 in {rest} of {} face bones", full.len());
        // The glb is the head with one animation.
        let gl = gltf::Gltf::open(&out.glb).unwrap();
        assert_eq!(gl.animations().count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
