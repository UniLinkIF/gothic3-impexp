//! Lip-sync of Gothic 3's voice lines on the faces of its animated heads.
//!
//! Gothic 3 has no face shapes (no actor carries a morph section): its 28 `G3_Head_*_Animated_*` heads have face bones
//! (`*_Lip_*`, `*_Jaw_*`, `*_EyeBrown_*`, 22 keyed in all), and next to each a clip `<head>_LipSync.xmot` of 15 poses,
//! one every 0.04 s. A voice line (`Speech_<Language>.pak`: `<Language>_ogg/<line>_<Language>.ogg`) has its timeline
//! beside it in `<Language>_LipSyncData/<line>_<Language>.xlip`:
//!
//! ```text
//! GENOMFLE · u16 1 · u32 table · u16 1 · u32 n · n × (f32 start s, u32 phone 1..13) · table (DEADBEEF 01 u32 0)
//! (Speech_German.p00, a patch: the same without GENOMFLE and table, `u16 1 · u32 n · n × (f32, u32)`)
//! ```
//! (every line read: count, rising times, phones 1–13). A phone p shows key p of each of the clip's key arrays: the
//! game's own code ([`motion`]). Pose 1 is the rest pose (as pose 0): phone 1, the most frequent one, the pauses.

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
pub fn parse_xlip(d: &[u8]) -> Result<Vec<(f32, u32)>> { Ok(parse_xlip_form(d)?.0) }

/// The timeline and whether the file is the GENOMFLE form (the archive's) or the plain one (a patch volume's three German
/// lines: `u16 1 · u32 n · n × (f32, u32)`, nothing else).
pub fn parse_xlip_form(d: &[u8]) -> Result<(Vec<(f32, u32)>, bool)> {
    let u = |p: usize| u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
    let entries = |at: usize, n: usize| (0..n).map(|i| (f32::from_bits(u(at + i * 8)), u(at + 4 + i * 8))).collect();
    if !d.starts_with(b"GENOMFLE") {
        if d.len() < 6 || u16::from_le_bytes([d[0], d[1]]) != 1 || 6 + u(2) as usize * 8 != d.len() { bail!("not a lip-sync file ({} bytes)", d.len()); }
        return Ok((entries(6, u(2) as usize), false));
    }
    if d.len() < 20 { bail!("lip-sync file of {} bytes", d.len()); }
    let (table, n) = (u(10) as usize, u(16) as usize);
    if u16::from_le_bytes([d[14], d[15]]) != 1 || 20 + n * 8 != table || table > d.len() { bail!("lip-sync timeline of {n} phones does not fit ({} bytes)", d.len()); }
    Ok((entries(20, n), true))
}

/// The plain form of a timeline.
pub fn write_xlip_plain(phones: &[(f32, u32)]) -> Vec<u8> {
    let mut o = 1u16.to_le_bytes().to_vec();
    o.extend((phones.len() as u32).to_le_bytes());
    for (t, p) in phones { o.extend(t.to_le_bytes()); o.extend(p.to_le_bytes()); }
    o
}

/// A `.xlip` timeline as the game writes them (every one of the game's is written back to the byte).
pub fn write_xlip(phones: &[(f32, u32)]) -> Vec<u8> {
    let mut o = b"GENOMFLE".to_vec();
    o.extend(1u16.to_le_bytes());
    o.extend((20 + phones.len() as u32 * 8).to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend((phones.len() as u32).to_le_bytes());
    for (t, p) in phones { o.extend(t.to_le_bytes()); o.extend(p.to_le_bytes()); }
    o.extend(0xDEADBEEFu32.to_le_bytes());
    o.push(1);
    o.extend(0u32.to_le_bytes());
    o
}

/// What the add-on sends back: a line's name and language and its phones (start s, pose).
#[derive(serde::Deserialize)]
pub struct LipSpec { pub line: String, pub language: String, pub phones: Vec<(f32, u32)> }

/// The `.xlip` of a line for a mod: in `Speech_<Language>`, where the game looks for the line's lip-sync. Phones are
/// the poses 0–14 of the heads' clips (the game's lines use 1–13), their starts rising.
pub fn export(spec: &LipSpec) -> Result<(serde_json::Value, (String, String, Vec<u8>))> {
    let lang = spec.language.as_str();
    if lang.is_empty() || !lang.chars().all(|c| c.is_ascii_alphabetic()) { bail!("language {lang:?}"); }
    let base = spec.line.trim_end_matches(".xlip");
    let base = base.strip_suffix(&format!("_{lang}")).unwrap_or(base);
    if base.is_empty() || !base.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)) { bail!("line name {base:?}: letters, digits, _ - only"); }
    if spec.phones.is_empty() { bail!("no phones"); }
    if let Some(p) = spec.phones.iter().find(|p| p.1 > 14) { bail!("phone {} at {} s: the heads' clips have poses 0-14", p.1, p.0); }
    if spec.phones.windows(2).any(|w| w[1].0 < w[0].0) || spec.phones[0].0 < 0.0 { bail!("phone starts must rise from 0"); }
    let path = format!("{lang}_LipSyncData/{base}_{lang}.xlip");
    let report = serde_json::json!({ "line": base, "language": lang, "phones": spec.phones.len(), "duration": spec.phones.last().map(|p| p.0).unwrap_or(0.0), "path": format!("Speech_{lang}/{path}") });
    Ok((report, (format!("Speech_{lang}"), path, write_xlip(&spec.phones))))
}

/// Key arrays of a `_LipSync` track the game takes: those with at least this many keys (Engine.dll
/// `eCWrapper_emfx2Motion::CreateLipSyncMotion`, 0x300ee830: `cmp [track+0x38], 0xe` for position, `+0x18` rotation,
/// `+0x58` scale).
pub const MIN_KEYS: usize = 14;

/// The motion the game makes of a line, as Engine.dll does: for every key array of the head's clip with at least
/// [`MIN_KEYS`] keys, a key at each phone's start whose value is the array's key number `phone` (`keys[phone]`, its
/// time not looked at). Shorter arrays get no keys (their track keeps its pose). `tail` s are added to the length only.
pub fn motion(clip: &G3Motion, phones: &[(f32, u32)], tail: f32) -> G3Motion {
    let mut out = G3Motion { tracks: vec![], duration: phones.last().map(|x| x.0 + tail).unwrap_or(0.0), effects: vec![] };
    fn put<const N: usize>(keys: &[(f32, [f32; N])], phones: &[(f32, u32)]) -> Vec<(f32, [f32; N])> {
        if keys.len() < MIN_KEYS { return vec![]; }
        phones.iter().filter_map(|&(t, p)| keys.get(p as usize).map(|k| (t, k.1))).collect()
    }
    for t in &clip.tracks {
        out.tracks.push(G3Track { bone: t.bone.clone(), pose_pos: t.pose_pos, pose_rot: t.pose_rot, pos: put(&t.pos, phones), rot: put(&t.rot, phones), scale: put(&t.scale, phones) });
    }
    out
}

#[derive(serde::Serialize)]
pub struct LipOut { pub line: String, pub head: String, pub phones: usize, pub duration: f32, pub glb: String, pub sound: Option<String>, pub face_bones: Vec<String>, pub timeline: Vec<(f32, u32)> }

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
    Ok(LipOut { line: base.to_string(), head: head_name, phones: phones.len(), duration: m.duration, glb: glb.to_string_lossy().into_owned(), sound, face_bones, timeline: phones })
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

    /// Every `.xlip` of the game, in every speech volume, written back by our writer is the same to the byte.
    #[test]
    fn every_timeline_of_the_game_writes_back_to_the_byte() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let data = Path::new(&root).join("Data");
        let mut n = 0;
        for v in std::fs::read_dir(&data).unwrap().flatten().map(|e| e.path()).filter(|p| p.file_name().unwrap().to_string_lossy().to_lowercase().starts_with("speech_")) {
            for e in crate::g3::read_volume(&v).unwrap().iter().filter(|e| !e.is_deleted() && e.path.to_lowercase().ends_with(".xlip")) {
                let d = crate::g3::read_entry(&v, e).unwrap();
                let (t, genomfle) = parse_xlip_form(&d).unwrap_or_else(|x| panic!("{} in {}: {x:#}", e.path, v.display()));
                assert_eq!(if genomfle { write_xlip(&t) } else { write_xlip_plain(&t) }, d, "{}", e.path);
                n += 1;
            }
        }
        assert_eq!(n, 24241);
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
            assert_eq!(ours.rot.len(), xlip.len());
            for (k, &(t, p)) in ours.rot.iter().zip(&xlip) { assert_eq!(k.0, t); assert_eq!(k.1, src.rot[p as usize].1, "{} phone {p}", src.bone); }
        }
        // Arrays shorter than the game takes (a few positions, the jaw's first bone) do not move.
        for t in clip.tracks.iter() {
            let ours = m.tracks.iter().find(|o| o.bone == t.bone).unwrap();
            assert_eq!((ours.pos.is_empty(), ours.rot.is_empty()), (t.pos.len() < MIN_KEYS, t.rot.len() < MIN_KEYS), "{}", t.bone);
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
