//! Gothic 3 motions (`_compiledAnimation/*.xmot`): one clip of keyframed node transforms.
//!
//! A `GENOMFLE` resource wrapping an old EMotionFX motion (`LMA `, v1.1), the sibling of the `FXA ` actor in
//! [`crate::g3_actor`]. Layout, measured on every `.xmot` of the install (see the tests):
//!
//! ```text
//! u32 LMA bytes (counted from "LMA ") · "LMA " · u8 1 · u8 1 · u8 0 · chunks to the end (the GENOMFLE tail follows):
//!   u32 id · u32 size (body bytes after the version) · u32 version · body
//!   1 v3  submotion: f32 pose position[3] · quat[4] (x y z w) · f32 scale[3] · the bind pose (position, rotation, scale)
//!                    · u32 len · name                                    -> starts a track for node `name`
//!   2 v1  key track of the submotion before it: u32 n · u8 interpolation ('L' linear) · u8 kind ('P' position,
//!                    'R' rotation, 'S' scale) · u16 padding (left as it was in memory) · n × key
//!                    key = f32 time (s) · value: quat (x y z w) for rotation, vec3 otherwise
//! ```
//!
//! Real bytes (wolf ambient loop, node `Wolf_Tail_Tail_4`):
//! ```text
//! 0036: 4c4d4120 01 01 00                               "LMA " 1.1
//! 145b: 01000000 64000000 03000000 80a68841 ...         submotion, 100 bytes, pos.x 17.08 …
//! 14cb: 02000000 ec070000 01000000 65000000 4c523f02    key track, 2028 bytes, 101 keys, 'L' 'R'
//! 14df: 00000000 00000000 5c012e3b 00000000 00fe7f3f    t 0 · rot (0, 0.00266, 0, 0.99997)
//! ```
//!
//! Conventions (checked by posing the wolf actor, `wolf_walk_poses_the_wolf_on_its_feet`):
//! * Key values are **absolute local transforms**, not deltas. The submotion's pose is the clip's first frame and
//!   equals the bind pose for an idle clip.
//! * The parent of a track is the **nearest ancestor that has a track**: the actor's helper bones (`*_ROOT`,
//!   `*_END`, never in a motion) are folded in, so the `X_1` track = `parent X_END ∘ X_ROOT ∘ X_1` of the actor.
//! * Quaternions are used as stored, the same as the actor's node rotations: `world.rot = parent.rot * local.rot`
//!   (Hamilton product), `world.pos = parent.pos + rotate(parent.rot, local.pos)`, `rotate(q, v) = q v q⁻¹`.
//!   No conjugation (posing with the conjugate leaves the paws 100+ units in the air).
//! * Times are seconds from 0 (keys at 1/30 or 1/25 s); `duration` = the last key time of any track.
//! * Quaternion keys are ~1e-4 off unit length (quantised to 16 bits once: 0.99997 = 32766/32767); 1668 keys in 77
//!   clips (demon spine, |q| ≈ 0.95) are further off and two are all zero (fat fingers). [`decode_xmot`] normalises
//!   every rotation key and replaces a zero one with the previous key. 131 keys repeat the previous key time.
//! * Tracks without keys exist (the pose only). Names starting with `!` are tool helpers (`!FH_…IKGoalHelper`,
//!   `!DMW_…BaseMotion_Spline`) with no actor node; the caller filters them.
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Default)]
pub struct G3Track {
    pub bone: String,
    /// (time s, local position)
    pub pos: Vec<(f32, [f32; 3])>,
    /// (time s, local rotation x y z w)
    pub rot: Vec<(f32, [f32; 4])>,
    pub scale: Vec<(f32, [f32; 3])>,
    /// The local transform when the track has no keys of that kind (= the first frame).
    pub pose_pos: [f32; 3],
    pub pose_rot: [f32; 4],
}

#[derive(Debug, Clone, Default)]
pub struct G3Motion { pub tracks: Vec<G3Track>, pub duration: f32 }

fn u32_at(d: &[u8], p: usize) -> Result<u32> { Ok(u32::from_le_bytes(d.get(p..p + 4).with_context(|| format!("motion runs short at 0x{p:x}"))?.try_into().unwrap())) }
fn f32_at(d: &[u8], p: usize) -> Result<f32> { Ok(f32::from_bits(u32_at(d, p)?)) }

/// The chunks of the LMA motion: (offset, id, body size, version).
fn chunks(d: &[u8]) -> Result<Vec<(usize, u32, usize, u32)>> {
    let at = d.windows(4).take(512).position(|w| w == b"LMA ").filter(|&p| p >= 4).context("no LMA motion")?;
    let end = (u32_at(d, at - 4)? as usize + at).min(d.len());
    let mut v = vec![];
    let mut p = at + 7;
    while p + 12 <= end {
        let (id, size, ver) = (u32_at(d, p)?, u32_at(d, p + 4)? as usize, u32_at(d, p + 8)?);
        if p + 12 + size > end { bail!("chunk {id} at 0x{p:x} runs past the motion"); }
        v.push((p, id, size, ver));
        p += 12 + size;
    }
    if p != end { bail!("{} trailing bytes in the motion", end - p); }
    Ok(v)
}

pub fn decode_xmot(d: &[u8]) -> Result<G3Motion> {
    let cs = chunks(d)?;
    let key_size = |p: usize, size: usize| -> Result<usize> {
        if size < 8 { bail!("key track of {size} bytes at 0x{p:x}"); }
        let n = u32_at(d, p + 12)? as usize;
        Ok(if n == 0 { 0 } else { (size - 8) / n })
    };
    let mut m = G3Motion::default();
    for &(p, id, size, _) in &cs {
        let b = p + 12;
        match id {
            1 => {
                if size < 84 { bail!("{size}-byte submotion at 0x{p:x}"); }
                let pose_pos = [f32_at(d, b)?, f32_at(d, b + 4)?, f32_at(d, b + 8)?];
                let pose_rot = [f32_at(d, b + 12)?, f32_at(d, b + 16)?, f32_at(d, b + 20)?, f32_at(d, b + 24)?];
                let n = u32_at(d, b + 80)? as usize;
                if 84 + n > size { bail!("name of {n} bytes in a {size}-byte submotion at 0x{p:x}"); }
                let bone = String::from_utf8_lossy(&d[b + 84..b + 84 + n]).into_owned();
                m.tracks.push(G3Track { bone, pose_pos, pose_rot, ..Default::default() });
            }
            2 => {
                let n = u32_at(d, b)? as usize;
                let kind = *d.get(b + 5).context("key track runs short")?;
                let k = key_size(p, size)?;
                if n * k + 8 != size { bail!("{n} keys don't fill a {size}-byte track at 0x{p:x}"); }
                let t = m.tracks.last_mut().with_context(|| format!("key track at 0x{p:x} before any submotion"))?;
                let key = |i: usize, j: usize| f32_at(d, b + 8 + i * k + j * 4);
                match k {
                    0 => {}
                    20 => {
                        for i in 0..n {
                            let q = [key(i, 1)?, key(i, 2)?, key(i, 3)?, key(i, 4)?];
                            let l = q.iter().map(|x| x * x).sum::<f32>().sqrt();
                            // A few clips store slightly non-unit (demon spine, |q| 0.95) or all-zero (fat fingers) keys.
                            let q = if l > 1e-3 { q.map(|x| x / l) } else { t.rot.last().map(|k| k.1).unwrap_or([0.0, 0.0, 0.0, 1.0]) };
                            t.rot.push((key(i, 0)?, q));
                        }
                    }
                    16 => {
                        let scale = match kind { b'P' => false, b'S' => true, x => bail!("vec3 track at 0x{p:x} of kind {:?}", x as char) };
                        let v = if scale { &mut t.scale } else { &mut t.pos };
                        for i in 0..n { v.push((key(i, 0)?, [key(i, 1)?, key(i, 2)?, key(i, 3)?])); }
                    }
                    _ => bail!("{k}-byte keys at 0x{p:x}"),
                }
            }
            _ => bail!("chunk {id} at 0x{p:x}"),
        }
    }
    m.duration = m.tracks.iter().flat_map(|t| t.pos.iter().map(|k| k.0).chain(t.rot.iter().map(|k| k.0)).chain(t.scale.iter().map(|k| k.0))).fold(0f32, f32::max);
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g3_actor::{decode_xact, G3Actor};

    const WOLF: &str = "_compiledanimation/g3_wolf_body_01.xact";
    const AMBIENT: &str = "_compiledanimation/wolf_stand_none_none_p0_ambient_loop_n_fwd_00_%_05_p0_0.xmot";
    const WALK: &str = "_compiledanimation/wolf_stand_none_none_p0_move_walk_n_fwd_00_%_00_p0_150.xmot";

    fn g3() -> Option<crate::g3::G3Ctx> { std::env::var_os("G3_GAME").map(|r| crate::g3::G3Ctx::open(std::path::Path::new(&r)).unwrap()) }

    type Q = [f32; 4];
    type V = [f32; 3];
    fn qmul(a: Q, b: Q) -> Q {
        [a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1], a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0], a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3], a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2]]
    }
    fn conj(q: Q) -> Q { [-q[0], -q[1], -q[2], q[3]] }
    fn qrot(q: Q, v: V) -> V { let r = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q)); [r[0], r[1], r[2]] }
    fn add(a: V, b: V) -> V { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
    fn dist(a: V, b: V) -> f32 { ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt() }
    fn len4(q: Q) -> f32 { (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt() }
    fn qnorm(q: Q) -> Q { let l = len4(q); q.map(|x| x / l) }
    /// 0 when the rotations are equal (q and -q are the same rotation).
    fn qdiff(a: Q, b: Q) -> f32 { 1.0 - (0..4).map(|i| a[i] * b[i]).sum::<f32>().abs() }

    /// World transforms from (parent, local pos, local rot); `flip` conjugates every stored rotation first.
    fn world(nodes: &[(Option<usize>, V, Q)], flip: bool) -> Vec<(V, Q)> {
        fn go(i: usize, nodes: &[(Option<usize>, V, Q)], flip: bool, w: &mut Vec<Option<(V, Q)>>) -> (V, Q) {
            if let Some(x) = w[i] { return x; }
            let (p, t, q) = nodes[i];
            let q = if flip { conj(qnorm(q)) } else { qnorm(q) };
            let r = match p { None => (t, q), Some(p) => { let (pt, pq) = go(p, nodes, flip, w); (add(pt, qrot(pq, t)), qmul(pq, q)) } };
            w[i] = Some(r);
            r
        }
        let mut w = vec![None; nodes.len()];
        (0..nodes.len()).map(|i| go(i, nodes, flip, &mut w)).collect()
    }
    fn sample<const N: usize>(keys: &[(f32, [f32; N])], t: f32, def: [f32; N]) -> [f32; N] {
        if keys.is_empty() { return def; }
        let i = keys.partition_point(|k| k.0 <= t);
        if i == 0 { return keys[0].1; }
        if i == keys.len() { return keys[i - 1].1; }
        let (a, b) = (keys[i - 1], keys[i]);
        let f = (t - a.0) / (b.0 - a.0).max(1e-9);
        let s = if N == 4 && (0..N).map(|j| a.1[j] * b.1[j]).sum::<f32>() < 0.0 { -1.0 } else { 1.0 };
        std::array::from_fn(|j| a.1[j] * (1.0 - f) + b.1[j] * s * f)
    }
    /// Pose an actor at time `t`: a tracked node takes its track as the local transform under its nearest tracked
    /// ancestor; an untracked node (the `_ROOT`/`_END` helpers) keeps its bind local transform under its parent.
    fn pose(a: &G3Actor, m: &G3Motion, t: f32, flip: bool) -> Vec<(V, Q)> {
        let tr: Vec<Option<&G3Track>> = a.nodes.iter().map(|n| m.tracks.iter().find(|k| k.bone == n.name)).collect();
        let nodes: Vec<(Option<usize>, V, Q)> = a.nodes.iter().enumerate().map(|(i, n)| match tr[i] {
            Some(k) => {
                let mut p = n.parent;
                while let Some(x) = p { if tr[x].is_some() { break; } p = a.nodes[x].parent; }
                (p, sample(&k.pos, t, k.pose_pos), sample(&k.rot, t, k.pose_rot))
            }
            None => (n.parent, n.pos, n.rot),
        }).collect();
        world(&nodes, flip)
    }

    #[test]
    fn every_gothic3_motion_decodes_with_sane_keys() {
        let Some(g) = g3() else { return };
        let (mut ok, mut fail, mut tracks, mut keys, mut scale_tracks, mut pos_tracks) = (0usize, vec![], 0usize, 0usize, 0usize, 0usize);
        let (mut worst_unit, mut longest, mut repeated) = (0f32, 0f32, 0usize);
        let mut scale_range = (f32::MAX, f32::MIN);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xmot")) {
            let m = match decode_xmot(&g.read(&k).unwrap()) { Ok(m) => m, Err(e) => { fail.push(format!("{k}: {e}")); continue } };
            ok += 1;
            longest = longest.max(m.duration);
            for t in &m.tracks {
                tracks += 1;
                keys += t.pos.len() + t.rot.len() + t.scale.len();
                if !t.pos.is_empty() { pos_tracks += 1; }
                if !t.scale.is_empty() { scale_tracks += 1; }
                let times = t.pos.iter().map(|k| k.0).collect::<Vec<_>>();
                for ts in [times, t.rot.iter().map(|k| k.0).collect(), t.scale.iter().map(|k| k.0).collect()] {
                    assert!(ts.windows(2).all(|w| w[0] <= w[1]), "{k} {}: times go back {ts:?}", t.bone);
                    repeated += ts.windows(2).filter(|w| w[0] == w[1]).count();
                    assert!(ts.iter().all(|&x| (0.0..=m.duration).contains(&x)), "{k} {}", t.bone);
                }
                for r in &t.rot { worst_unit = worst_unit.max((len4(r.1) - 1.0).abs()); }
                for s in &t.scale { for v in s.1 { scale_range = (scale_range.0.min(v), scale_range.1.max(v)); } }
            }
        }
        eprintln!("motions {ok} ok, {} failed {:?}; {tracks} tracks ({pos_tracks} with positions, {scale_tracks} with scale), {keys} keys ({repeated} repeat the previous time); longest {longest:.2} s; worst |q|-1 {worst_unit:.1e}; scale keys {scale_range:?}", fail.len(), fail.iter().take(5).collect::<Vec<_>>());
        assert!(fail.is_empty() && ok > 5000);
        assert!(worst_unit < 1e-2);
    }

    #[test]
    fn wolf_ambient_loop_names_the_wolf_bones_and_closes() {
        let Some(g) = g3() else { return };
        let a = decode_xact(&g.read(WOLF).unwrap()).unwrap();
        let m = decode_xmot(&g.read(AMBIENT).unwrap()).unwrap();
        let named: Vec<&G3Track> = m.tracks.iter().filter(|t| !t.bone.starts_with('!')).collect();
        let matched = named.iter().filter(|t| a.nodes.iter().any(|n| n.name == t.bone)).count();
        let mut worst_loop = 0f32;
        for t in m.tracks.iter().filter(|t| t.rot.len() > 2) { worst_loop = worst_loop.max(qdiff(t.rot[0].1, t.rot.last().unwrap().1)); }
        eprintln!("ambient: {} tracks, {} helpers, {matched}/{} named tracks are wolf nodes, duration {:.3} s, worst first/last rotation 1-|dot| {worst_loop:.1e}", m.tracks.len(), m.tracks.len() - named.len(), named.len(), m.duration);
        assert_eq!(matched, named.len());
        assert!(named.len() > 30 && (m.duration - 10.0 / 3.0).abs() < 1e-3);
        assert!(worst_loop < 1e-3);
    }

    #[test]
    fn wolf_walk_poses_the_wolf_on_its_feet() {
        let Some(g) = g3() else { return };
        let a = decode_xact(&g.read(WOLF).unwrap()).unwrap();
        let idx = |s: &str| a.nodes.iter().position(|n| n.name == s).unwrap();
        // A track folds the helper bones in: Leg_1 = Hip_END * Leg_ROOT of the actor.
        let amb = decode_xmot(&g.read(AMBIENT).unwrap()).unwrap();
        let leg1 = amb.tracks.iter().find(|t| t.bone == "Wolf_Right_Leg_Leg_1").unwrap();
        assert!(qdiff(leg1.pose_rot, qmul(a.nodes[idx("Wolf_Right_Leg_Hip_END")].rot, a.nodes[idx("Wolf_Right_Leg_Leg_ROOT")].rot)) < 1e-5);
        let walk = decode_xmot(&g.read(WALK).unwrap()).unwrap();
        let paws = ["Wolf_Right_Foot_Toe_END", "Wolf_Left_Foot_Toe_END", "Wolf_Right_Hand_Finger_END", "Wolf_Left_Hand_Finger_END"];
        let bones: Vec<usize> = (0..a.nodes.len()).filter(|&i| walk.tracks.iter().any(|t| t.bone == a.nodes[i].name && t.pos.is_empty())).collect();
        for flip in [false, true] {
            let bind = world(&a.nodes.iter().map(|n| (n.parent, n.pos, n.rot)).collect::<Vec<_>>(), flip);
            // The idle clip's first frame is the bind pose.
            let p0 = pose(&a, &amb, 0.0, flip);
            let idle = amb.tracks.iter().filter_map(|t| a.nodes.iter().position(|n| n.name == t.bone)).map(|i| dist(bind[i].0, p0[i].0)).fold(0f32, f32::max);
            let (mut low, mut high, mut stretch) = (f32::MAX, f32::MIN, 0f32);
            for f in 0..=25 {
                let p = pose(&a, &walk, f as f32 / 25.0 * walk.duration, flip);
                for s in paws { let y = p[idx(s)].0[1]; low = low.min(y); high = high.max(y); }
                // Bones without position keys keep their length to the (folded) parent.
                for &i in &bones {
                    let mut q = a.nodes[i].parent;
                    while let Some(x) = q { if walk.tracks.iter().any(|t| t.bone == a.nodes[x].name) { break; } q = a.nodes[x].parent; }
                    if let Some(q) = q { stretch = stretch.max((dist(p[i].0, p[q].0) - dist(p0[i].0, p0[q].0)).abs()); }
                }
            }
            eprintln!("flip {flip}: idle t0 vs bind {idle:.3}; walk {:.2} s paw y {low:.1}..{high:.1}; bone length drift {stretch:.4}", walk.duration);
            if !flip { assert!(idle < 0.1 && low > -8.0 && high < 25.0 && stretch < 0.01, "{idle} {low} {high} {stretch}"); } else { assert!(idle > 10.0 || high > 50.0); }
        }
    }
}
