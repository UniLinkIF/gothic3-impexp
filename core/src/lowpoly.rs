//! The distant land: what the game draws of the ground far away, instead of its landscape cells.
//!
//! `_compiledMesh/G3_World_Lowpoly_Landscape_01/G3_World_Landscape_Lowpoly_<nnn>.xcmsh` (209 tiles of the ground, and
//! the sea's `…_Water_Lowpoly_<nnn>`), each a 300 m square of the world grid (box on multiples of 30000 cm), vertices
//! in world space, a material per element whose texture is a baked picture of the land (uvs into an atlas, not along
//! the ground), a second uv set (stream 73), colours; no collision, no lightmap.
//!
//! When cells change, every ground tile over the changed part follows by the difference the edit made: at each point,
//! the edited cell's height minus the shipped cell's height there is added to the tile. The tile keeps its own
//! triangles (so the atlas stays right); those over the changed part are split until no edge is longer than
//! [`EDGE`], each split shared with the neighbour across it (no cracks), and a new vertex takes the uv, colours and
//! second uv set of the triangle it lies in. Outside the change nothing moves.

use crate::g3::G3Ctx;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// Longest edge (cm, on the ground) of a tile's triangle over a changed part.
pub const EDGE: f32 = 800.0;
const ROOT: &str = "_compiledmesh/g3_world_lowpoly_landscape_01/";

/// The ground tiles (not the sea's) with their boxes on the ground (x, z min / max).
pub fn tiles(g: &G3Ctx) -> Result<Vec<(String, [f32; 4])>> {
    let mut v = vec![];
    for k in g.keys().into_iter().filter(|k| k.starts_with(ROOT) && k.contains("_landscape_lowpoly_") && k.ends_with(".xcmsh")) {
        let (lo, hi) = crate::g3_res::xcmsh_box(&g.read(&k)?).with_context(|| k.clone())?;
        v.push((k, [lo[0], lo[2], hi[0], hi[2]]));
    }
    Ok(v)
}

/// Heights of a ground (triangles, cm): the highest point above (x, z).
pub struct Heights { tris: Vec<[[f32; 3]; 3]>, grid: HashMap<(i32, i32), Vec<u32>> }

const CELL: f32 = 500.0;

impl Heights {
    pub fn new(tris: Vec<[[f32; 3]; 3]>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            let (x0, x1) = (t.iter().map(|p| p[0]).fold(f32::MAX, f32::min), t.iter().map(|p| p[0]).fold(f32::MIN, f32::max));
            let (z0, z1) = (t.iter().map(|p| p[2]).fold(f32::MAX, f32::min), t.iter().map(|p| p[2]).fold(f32::MIN, f32::max));
            for gx in (x0 / CELL).floor() as i32..=(x1 / CELL).floor() as i32 {
                for gz in (z0 / CELL).floor() as i32..=(z1 / CELL).floor() as i32 { grid.entry((gx, gz)).or_default().push(i as u32); }
            }
        }
        Heights { tris, grid }
    }
    pub fn at(&self, x: f32, z: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for &i in self.grid.get(&((x / CELL).floor() as i32, (z / CELL).floor() as i32))? {
            let t = &self.tris[i as usize];
            if let Some(w) = bary(t[0], t[1], t[2], x, z) {
                let y = w[0] * t[0][1] + w[1] * t[1][1] + w[2] * t[2][1];
                if best.map_or(true, |b| y > b) { best = Some(y); }
            }
        }
        best
    }
}

/// Barycentric weights of (x, z) in triangle a b c seen from above, when it lies in it.
fn bary(a: [f32; 3], b: [f32; 3], c: [f32; 3], x: f32, z: f32) -> Option<[f32; 3]> {
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-6 { return None; }
    let w0 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
    let w1 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
    let w2 = 1.0 - w0 - w1;
    (w0 >= -1e-5 && w1 >= -1e-5 && w2 >= -1e-5).then_some([w0, w1, w2])
}

/// One changed cell: its ground as shipped and as edited, and the box (x, z) of what changed.
pub struct Change { pub stock: Heights, pub edited: Heights, pub area: [f32; 4] }

impl Change {
    /// The height the edit added at (x, z); nothing where either ground has no point.
    fn delta(&self, x: f32, z: f32) -> f32 {
        if x < self.area[0] || x > self.area[2] || z < self.area[1] || z > self.area[3] { return 0.0; }
        match (self.edited.at(x, z), self.stock.at(x, z)) { (Some(a), Some(b)) => a - b, _ => 0.0 }
    }
}

/// The box (x, z) of the corners an edit moved, added or removed (positions to 0.1 cm), widened by `margin`.
pub fn changed_area(stock: &[[f32; 3]], edited: &[[f32; 3]], margin: f32) -> Option<[f32; 4]> {
    let key = |p: &[f32; 3]| p.map(|x| (x * 10.0).round() as i64);
    let (s, e): (std::collections::HashSet<[i64; 3]>, std::collections::HashSet<[i64; 3]>) = (stock.iter().map(key).collect(), edited.iter().map(key).collect());
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in edited.iter().filter(|p| !s.contains(&key(p))).chain(stock.iter().filter(|p| !e.contains(&key(p)))) {
        b = [b[0].min(p[0]), b[1].min(p[2]), b[2].max(p[0]), b[3].max(p[2])];
    }
    (b[0] <= b[2]).then(|| [b[0] - margin, b[1] - margin, b[2] + margin, b[3] + margin])
}

fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool { a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] }

#[derive(Clone)]
struct V { p: [f32; 3], uv: [f32; 2], extra: Vec<[f32; 2]>, dif: u32, spe: u32 }

fn lerp_argb(a: u32, b: u32) -> u32 { (0..4).map(|k| { let (x, y) = ((a >> (k * 8)) & 255, (b >> (k * 8)) & 255); ((x + y + 1) / 2) << (k * 8) }).fold(0, |s, x| s | x) }

fn mid(a: &V, b: &V) -> V {
    V { p: [0, 1, 2].map(|k| (a.p[k] + b.p[k]) / 2.0), uv: [0, 1].map(|k| (a.uv[k] + b.uv[k]) / 2.0), extra: a.extra.iter().zip(&b.extra).map(|(x, y)| [(x[0] + y[0]) / 2.0, (x[1] + y[1]) / 2.0]).collect(), dif: lerp_argb(a.dif, b.dif), spe: lerp_argb(a.spe, b.spe) }
}

/// Tile `bytes` following `changes`; None when no change reaches it. Returns the tile written again and how many
/// triangles it has now.
pub fn follow(bytes: &[u8], changes: &[&Change]) -> Result<Option<(Vec<u8>, usize)>> {
    let m = crate::g3_res::decode_xcmsh(bytes)?;
    let geo = &m.geo;
    let mut vs: Vec<V> = (0..geo.positions.len()).map(|i| V { p: geo.positions[i], uv: geo.uvs.get(i).copied().unwrap_or([0.0; 2]), extra: m.extra_uvs.iter().map(|e| e.1[i]).collect(), dif: m.diffuse[i], spe: m.specular[i] }).collect();
    let mut tris: Vec<([u32; 3], usize)> = vec![];
    for (e, s) in geo.submeshes.iter().enumerate() {
        for t in geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize].chunks_exact(3) { tris.push(([t[0], t[1], t[2]], e)); }
    }
    let area_of = |t: &[u32; 3], vs: &[V]| { let p = t.map(|i| vs[i as usize].p); [p.iter().map(|q| q[0]).fold(f32::MAX, f32::min), p.iter().map(|q| q[2]).fold(f32::MAX, f32::min), p.iter().map(|q| q[0]).fold(f32::MIN, f32::max), p.iter().map(|q| q[2]).fold(f32::MIN, f32::max)] };
    if !tris.iter().any(|(t, _)| changes.iter().any(|c| overlaps(area_of(t, &vs), c.area))) { return Ok(None); }
    // Split the long triangles over a change; a split edge splits the triangles on both sides. Edges are told by their
    // ends' positions (the tile repeats a vertex across a uv seam), the midpoint is made per side with that side's uvs.
    let len = |a: &V, b: &V| (a.p[0] - b.p[0]).hypot(a.p[2] - b.p[2]);
    let pk = |v: &V| v.p.map(f32::to_bits);
    for _ in 0..12 {
        let mut split: std::collections::HashSet<([u32; 3], [u32; 3])> = Default::default();
        for (t, _) in &tris {
            if !changes.iter().any(|c| overlaps(area_of(t, &vs), c.area)) { continue; }
            for k in 0..3 {
                let (a, b) = (&vs[t[k] as usize], &vs[t[(k + 1) % 3] as usize]);
                if len(a, b) > EDGE { let (x, y) = (pk(a), pk(b)); split.insert(if x < y { (x, y) } else { (y, x) }); }
            }
        }
        if split.is_empty() { break; }
        let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
        let mut at = |a: u32, b: u32, vs: &mut Vec<V>| -> Option<u32> {
            let (x, y) = (pk(&vs[a as usize]), pk(&vs[b as usize]));
            if !split.contains(&if x < y { (x, y) } else { (y, x) }) { return None; }
            Some(*mids.entry((a.min(b), a.max(b))).or_insert_with(|| { let m = mid(&vs[a as usize], &vs[b as usize]); vs.push(m); vs.len() as u32 - 1 }))
        };
        let mut next = Vec::with_capacity(tris.len() * 2);
        for (t, e) in tris {
            // Turn the triangle so its split edges come first (the order of its corners stays).
            let ms = [at(t[0], t[1], &mut vs), at(t[1], t[2], &mut vs), at(t[2], t[0], &mut vs)];
            let n = ms.iter().flatten().count();
            let r = match n { 1 => ms.iter().position(|m| m.is_some()).unwrap(), 2 => (ms.iter().position(|m| m.is_none()).unwrap() + 1) % 3, _ => 0 };
            let (a, b, c) = (t[r], t[(r + 1) % 3], t[(r + 2) % 3]);
            let (ab, bc, ca) = (at(a, b, &mut vs), at(b, c, &mut vs), at(c, a, &mut vs));
            match (ab, bc, ca) {
                (None, None, None) => next.push(([a, b, c], e)),
                (Some(m), None, None) => { next.push(([a, m, c], e)); next.push(([m, b, c], e)); }
                (Some(m1), Some(m2), None) => { next.push(([a, m1, m2], e)); next.push(([m1, b, m2], e)); next.push(([a, m2, c], e)); }
                (Some(m1), Some(m2), Some(m3)) => { next.push(([a, m1, m3], e)); next.push(([m1, b, m2], e)); next.push(([m3, m2, c], e)); next.push(([m1, m2, m3], e)); }
                _ => unreachable!(),
            }
        }
        tris = next;
    }
    // The edit's heights.
    for v in vs.iter_mut() { let d: f32 = changes.iter().map(|c| c.delta(v.p[0], v.p[2])).sum(); v.p[1] += d; }
    // Normals from the new shape, smooth over a position, turned the way the tile's own are.
    let key = |p: [f32; 3]| p.map(f32::to_bits);
    let mut acc: HashMap<[u32; 3], [f32; 3]> = HashMap::new();
    for (t, _) in &tris {
        let [a, b, c] = t.map(|i| vs[i as usize].p);
        let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        for p in [a, b, c] { let e = acc.entry(key(p)).or_default(); for k in 0..3 { e[k] += n[k]; } }
    }
    let raw: Vec<[f32; 3]> = vs.iter().map(|v| { let n = acc.get(&key(v.p)).copied().unwrap_or([0.0, 1.0, 0.0]); let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12); n.map(|x| x / l) }).collect();
    let agree: f32 = (0..geo.normals.len().min(raw.len())).map(|i| raw[i].iter().zip(&geo.normals[i]).map(|(a, b)| a * b).sum::<f32>()).sum();
    let normals: Vec<[f32; 3]> = if agree < 0.0 { raw.iter().map(|n| n.map(|x| -x)).collect() } else { raw };
    // Parts per element, each with its own vertices.
    let mut parts = vec![];
    for (e, s) in geo.submeshes.iter().enumerate() {
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut p = crate::xcmsh_write::Part { material: s.material.clone(), positions: vec![], normals: vec![], uvs: vec![], triangles: vec![], diffuse: vec![], specular: vec![], extra_uvs: m.extra_uvs.iter().map(|x| (x.0, vec![])).collect() };
        for (t, te) in &tris {
            if *te != e { continue; }
            let tri = t.map(|i| *remap.entry(i).or_insert_with(|| {
                let v = &vs[i as usize];
                p.positions.push(v.p); p.normals.push(normals[i as usize]); p.uvs.push(v.uv); p.diffuse.push(v.dif); p.specular.push(v.spe);
                for (x, val) in p.extra_uvs.iter_mut().zip(&v.extra) { x.1.push(*val); }
                p.positions.len() as u32 - 1
            }));
            p.triangles.push(tri);
        }
        parts.push(p);
    }
    Ok(Some((crate::xcmsh_write::write(&parts), tris.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ground tile written back by the writer reads with all it carries (positions, uvs, second set, colours,
    /// triangles, materials); the sea's tiles are left out of the ground.
    #[test]
    fn every_ground_tile_writes_back() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let ts = tiles(&g).unwrap();
        assert!(ts.len() > 150 && ts.iter().all(|t| !t.0.contains("water")), "{} tiles", ts.len());
        for (k, _) in &ts {
            let m = crate::g3_res::decode_xcmsh(&g.read(k).unwrap()).unwrap();
            let geo = &m.geo;
            let parts: Vec<crate::xcmsh_write::Part> = geo.submeshes.iter().enumerate().map(|(e, s)| {
                let (first, nv) = m.elements[e];
                let r = first as usize..(first + nv) as usize;
                crate::xcmsh_write::Part { material: s.material.clone(), positions: geo.positions[r.clone()].to_vec(), normals: geo.normals[r.clone()].to_vec(), uvs: geo.uvs[r.clone()].to_vec(),
                    triangles: geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize].chunks_exact(3).map(|t| [t[0] - first, t[1] - first, t[2] - first]).collect(),
                    diffuse: m.diffuse[r.clone()].to_vec(), specular: m.specular[r.clone()].to_vec(), extra_uvs: m.extra_uvs.iter().map(|(s, v)| (*s, v[r.clone()].to_vec())).collect() }
            }).collect();
            let b = crate::g3_res::decode_xcmsh(&crate::xcmsh_write::write(&parts)).unwrap();
            let bits = |v: &[[f32; 2]]| v.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
            assert!(b.geo.positions == geo.positions && b.geo.indices == geo.indices && bits(&b.geo.uvs) == bits(&geo.uvs) && b.diffuse == m.diffuse && b.specular == m.specular, "{k}");
            assert_eq!(b.extra_uvs.iter().map(|(s, v)| (*s, bits(v))).collect::<Vec<_>>(), m.extra_uvs.iter().map(|(s, v)| (*s, bits(v))).collect::<Vec<_>>(), "{k}");
            assert!(m.extra_uvs.iter().any(|e| e.0 == 73), "{k}: no second uv set");
        }
    }

    /// A hill on flat ground shows on the tile over it, and only there: the tile's own points far from it stay put.
    #[test]
    fn a_tile_follows_a_hill_and_nothing_else() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (k, a) = tiles(&g).unwrap().into_iter().max_by_key(|t| g.size_of(&t.0).unwrap_or(0)).unwrap();
        let d = g.read(&k).unwrap();
        let old = crate::g3_res::decode_xcmsh(&d).unwrap();
        let old_h = Heights::new(old.geo.indices.chunks_exact(3).map(|t| [0, 1, 2].map(|i| old.geo.positions[t[i] as usize])).collect());
        // A flat ground under the tile's middle and the same with a hill: 10 m high, flat 30 m around, down to 40 m.
        let (cx, cz) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        let grid = |bump: f32| { let mut t = vec![]; let s = 500.0; let p = |i: i32, j: i32| { let (x, z) = (cx + i as f32 * s, cz + j as f32 * s); let r = (x - cx).hypot(z - cz); [x, bump * ((4000.0 - r) / 1000.0).clamp(0.0, 1.0), z] };
            for i in -20..20 { for j in -20..20 { t.push([p(i, j), p(i + 1, j), p(i + 1, j + 1)]); t.push([p(i, j), p(i + 1, j + 1), p(i, j + 1)]); } } t };
        let change = Change { stock: Heights::new(grid(0.0)), edited: Heights::new(grid(1000.0)), area: [cx - 4500.0, cz - 4500.0, cx + 4500.0, cz + 4500.0] };
        let (bytes, n) = follow(&d, &[&change]).unwrap().unwrap();
        let new = crate::g3_res::decode_xcmsh(&bytes).unwrap();
        let new_h = Heights::new(new.geo.indices.chunks_exact(3).map(|t| [0, 1, 2].map(|i| new.geo.positions[t[i] as usize])).collect());
        eprintln!("{k}: {} -> {n} triangles", old.geo.indices.len() / 3);
        if let (Some(o), Some(h)) = (old_h.at(cx, cz), new_h.at(cx, cz)) { assert!((h - o - 1000.0).abs() < 5.0, "middle: {o} -> {h}"); }
        // The ground outside the change keeps its height, at every old vertex.
        let mut far = 0;
        for p in &old.geo.positions {
            if p[0] > change.area[0] - 1.0 && p[0] < change.area[2] + 1.0 && p[2] > change.area[1] - 1.0 && p[2] < change.area[3] + 1.0 { continue; }
            let (o, h) = (old_h.at(p[0], p[2]).unwrap(), new_h.at(p[0], p[2]).unwrap());
            assert!((h - o).abs() < 0.5, "{p:?}: {o} -> {h}");
            far += 1;
        }
        assert!(far > 10);
        // No crack: every rim edge of the new tile (welded by position) lies on a rim edge of the old one.
        let rim = |m: &crate::g3_res::G3Mesh| { let k = |i: u32| m.geo.positions[i as usize].map(f32::to_bits); let mut c: HashMap<([u32; 3], [u32; 3]), usize> = HashMap::new();
            for t in m.geo.indices.chunks_exact(3) { for e in 0..3 { let (a, b) = (k(t[e]), k(t[(e + 1) % 3])); *c.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1; } }
            c.into_iter().filter(|x| x.1 == 1).map(|x| (x.0 .0.map(f32::from_bits), x.0 .1.map(f32::from_bits))).collect::<Vec<_>>() };
        let old_rim = rim(&old);
        let on = |p: [f32; 3]| old_rim.iter().any(|(a, b)| { let (dx, dz) = (b[0] - a[0], b[2] - a[2]); let l2 = dx * dx + dz * dz; let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[2] - a[2]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 }; (a[0] + t * dx - p[0]).hypot(a[2] + t * dz - p[2]) < 1.0 });
        for (a, b) in rim(&new) { assert!(on([(a[0] + b[0]) / 2.0, 0.0, (a[2] + b[2]) / 2.0]), "crack at {a:?} {b:?}"); }
        assert_eq!(new.geo.submeshes.len(), old.geo.submeshes.len());
    }
}
