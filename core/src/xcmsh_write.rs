//! Writing static meshes (`.xcmsh`, `eCResourceMeshComplex_PS`) as a plain Genome archive.
//!
//! ```text
//! u16 1 · u8 1 · u16 1 · u8 1 · "eCResourceMeshComplex_PS" · u8 1 · u16 0 · u16 83 · u16 83 · u32 size (rest)
//! · u16 30 · u32 2 · properties  BoundingBox bCBox (u16 30, u32 24, min, max) · ResourcePriority float 0
//! · u16 35 (mesh version) · u16 30 · u32 first element's size hint · f32 0 · u32 elements
//! element (one per material, version 5, as the game's own):
//!   u16 5 · u32 FVF (0x1d2: position, normal, diffuse, specular, uv) · f32 min[3] max[3] · u32 size hint
//!   · "<material>.xshmat" · u32 streams · per stream: u32 type · u16 1 · u8 1 · u32 count · data
//!     0 indices u32 · 12 uv f32×2 · 5 specular u32 · 4 diffuse u32 · 3 normals f32×3 · 1 positions f32×3
//!     · 64 tangents f32×3 (× handedness)
//!   · u8 1 · u32 0 · u8 1 · u32 0 · u8 1 · u32 0   (two index lists and a batch list, empty as in the landscape)
//!   · u32 nodes · per node: f32 radius · f32 centre[3] · u32 first · u32 count   (a sphere tree in preorder: a leaf
//!     has count 1, its triangles run from `first` to the next leaf's `first`; an inner node counts its subtree)
//!   · u8 1 · u32 triangles · u32 triangle per slot
//! ```
//! Coordinates are the game's (left-handed, Y up, centimetres); triangles front clockwise, as Gothic 3's own meshes.
//! A version 1 element without colours (what 0.9 wrote) shows in the game without its textures. Diffuse alpha is
//! the landscape's layer blend, specular alpha the baked brightness: both are copied from the mesh it replaces.

/// One material's part: vertices and triangles indexing them.
pub struct Part {
    pub material: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
    /// Vertex colours (ARGB); empty = white / opaque black.
    pub diffuse: Vec<u32>,
    pub specular: Vec<u32>,
}

/// Default colours of a vertex nothing is known about: white, fully lit.
pub const WHITE: u32 = 0xffff_ffff;
pub const LIT: u32 = 0xff00_0000;

/// The sphere tree of an element: nodes (radius, centre, first, count) in preorder and the triangle order.
pub fn sphere_tree(pos: &[[f32; 3]], tris: &[[u32; 3]]) -> (Vec<(f32, [f32; 3], u32, u32)>, Vec<u32>) {
    const LEAF: usize = 16;
    let centroid = |t: u32| -> [f32; 3] { let tr = tris[t as usize]; [0, 1, 2].map(|k| (pos[tr[0] as usize][k] + pos[tr[1] as usize][k] + pos[tr[2] as usize][k]) / 3.0) };
    fn build(ids: &mut [u32], first: u32, pos: &[[f32; 3]], tris: &[[u32; 3]], centroid: &dyn Fn(u32) -> [f32; 3], nodes: &mut Vec<(f32, [f32; 3], u32, u32)>) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &t in ids.iter() { for &v in &tris[t as usize] { for k in 0..3 { lo[k] = lo[k].min(pos[v as usize][k]); hi[k] = hi[k].max(pos[v as usize][k]); } } }
        let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let r = ids.iter().flat_map(|&t| tris[t as usize]).map(|v| { let p = pos[v as usize]; ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt() }).fold(0f32, f32::max);
        let at = nodes.len();
        nodes.push((r, c, first, 1));
        if ids.len() <= LEAF { return; }
        let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap();
        ids.sort_by(|&a, &b| centroid(a)[axis].total_cmp(&centroid(b)[axis]));
        let mid = ids.len() / 2;
        let (l, rr) = ids.split_at_mut(mid);
        build(l, first, pos, tris, centroid, nodes);
        build(rr, first + mid as u32, pos, tris, centroid, nodes);
        nodes[at].3 = (nodes.len() - at) as u32;
    }
    let mut ids: Vec<u32> = (0..tris.len() as u32).collect();
    let mut nodes = vec![];
    if !ids.is_empty() { build(&mut ids, 0, pos, tris, &centroid, &mut nodes); }
    (nodes, ids)
}

fn s(o: &mut Vec<u8>, x: &str) { o.extend((x.len() as u16).to_le_bytes()); o.extend(x.as_bytes()); }
fn u16_(o: &mut Vec<u8>, x: u16) { o.extend(x.to_le_bytes()); }
fn u32_(o: &mut Vec<u8>, x: u32) { o.extend(x.to_le_bytes()); }
fn f(o: &mut Vec<u8>, x: f32) { o.extend(x.to_le_bytes()); }
fn v3(o: &mut Vec<u8>, v: [f32; 3]) { for x in v { f(o, x); } }

fn bbox<'a>(ps: impl Iterator<Item = &'a [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in ps { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
    if lo[0] > hi[0] { ([0.0; 3], [0.0; 3]) } else { (lo, hi) }
}

/// Per-vertex tangents (u direction) times handedness, from positions, normals and uvs.
pub fn tangents(pos: &[[f32; 3]], nrm: &[[f32; 3]], uv: &[[f32; 2]], tris: &[[u32; 3]]) -> Vec<[f32; 3]> {
    let n = pos.len();
    let (mut t, mut b) = (vec![[0f32; 3]; n], vec![[0f32; 3]; n]);
    let sub = |a: [f32; 3], c: [f32; 3]| [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
    for tri in tris {
        let [i0, i1, i2] = tri.map(|i| i as usize);
        let (e1, e2) = (sub(pos[i1], pos[i0]), sub(pos[i2], pos[i0]));
        let (du1, dv1, du2, dv2) = (uv[i1][0] - uv[i0][0], uv[i1][1] - uv[i0][1], uv[i2][0] - uv[i0][0], uv[i2][1] - uv[i0][1]);
        let d = du1 * dv2 - du2 * dv1;
        if d.abs() < 1e-12 { continue; }
        let r = 1.0 / d;
        let tu = [0, 1, 2].map(|k| (e1[k] * dv2 - e2[k] * dv1) * r);
        let tv = [0, 1, 2].map(|k| (e2[k] * du1 - e1[k] * du2) * r);
        for i in [i0, i1, i2] { for k in 0..3 { t[i][k] += tu[k]; b[i][k] += tv[k]; } }
    }
    (0..n).map(|i| {
        let nn = nrm[i];
        let d = nn[0] * t[i][0] + nn[1] * t[i][1] + nn[2] * t[i][2];
        let mut x = [t[i][0] - nn[0] * d, t[i][1] - nn[1] * d, t[i][2] - nn[2] * d];
        let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
        if l < 1e-8 {
            // No uv gradient: any direction across the normal.
            let a = if nn[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let c = [nn[1] * a[2] - nn[2] * a[1], nn[2] * a[0] - nn[0] * a[2], nn[0] * a[1] - nn[1] * a[0]];
            let cl = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-8);
            return c.map(|v| v / cl);
        }
        x = x.map(|v| v / l);
        let c = [nn[1] * x[2] - nn[2] * x[1], nn[2] * x[0] - nn[0] * x[2], nn[0] * x[1] - nn[1] * x[0]];
        let hand = if c[0] * b[i][0] + c[1] * b[i][1] + c[2] * b[i][2] < 0.0 { -1.0 } else { 1.0 };
        x.map(|v| v * hand)
    }).collect()
}

/// The element's size hint (the game's own is near its vertex and index bytes; not a length).
fn hint(p: &Part) -> u32 { (124 + 7 * 20 + p.triangles.len() * 12 + p.positions.len() * (12 + 12 + 8 + 4 + 4 + 12)) as u32 }

pub fn write(parts: &[Part]) -> Vec<u8> {
    let mut o = vec![];
    u16_(&mut o, 1); o.push(1); u16_(&mut o, 1); o.push(1);
    s(&mut o, "eCResourceMeshComplex_PS");
    o.push(1); u16_(&mut o, 0);
    u16_(&mut o, 83); u16_(&mut o, 83);
    let size_at = o.len();
    u32_(&mut o, 0);
    u16_(&mut o, 30); u32_(&mut o, 2);
    let (lo, hi) = bbox(parts.iter().flat_map(|p| p.positions.iter()));
    s(&mut o, "BoundingBox"); s(&mut o, "bCBox"); u16_(&mut o, 30); u32_(&mut o, 24); v3(&mut o, lo); v3(&mut o, hi);
    s(&mut o, "ResourcePriority"); s(&mut o, "float"); u16_(&mut o, 30); u32_(&mut o, 4); f(&mut o, 0.0);
    let parts: Vec<&Part> = parts.iter().filter(|p| !p.triangles.is_empty()).collect();
    // Class version 35, its own chunk version 30 and a size (the game repeats the first element's hint there).
    u16_(&mut o, 35); u16_(&mut o, 30); u32_(&mut o, parts.first().map(|p| hint(p)).unwrap_or(0)); f(&mut o, 0.0);
    u32_(&mut o, parts.len() as u32);
    for p in parts {
        let nv = p.positions.len();
        let uv: Vec<[f32; 2]> = if p.uvs.len() == nv { p.uvs.clone() } else { vec![[0.0, 0.0]; nv] };
        let dif: Vec<u32> = if p.diffuse.len() == nv { p.diffuse.clone() } else { vec![WHITE; nv] };
        let spe: Vec<u32> = if p.specular.len() == nv { p.specular.clone() } else { vec![LIT; nv] };
        let tan = tangents(&p.positions, &p.normals, &uv, &p.triangles);
        let (nodes, order) = sphere_tree(&p.positions, &p.triangles);
        let streams = 7u32;
        u16_(&mut o, 5);
        u32_(&mut o, 0x2 | 0x10 | 0x40 | 0x80 | 0x100);
        let (lo, hi) = bbox(p.positions.iter());
        v3(&mut o, lo); v3(&mut o, hi);
        u32_(&mut o, hint(p));
        let m = p.material.trim_end_matches(".xshmat");
        s(&mut o, &format!("{m}.xshmat"));
        u32_(&mut o, streams);
        let head = |o: &mut Vec<u8>, ty: u32, n: usize| { u32_(o, ty); u16_(o, 1); o.push(1); u32_(o, n as u32); };
        head(&mut o, 0, p.triangles.len() * 3);
        for t in &p.triangles { for i in t { u32_(&mut o, *i); } }
        head(&mut o, 12, nv); for x in &uv { f(&mut o, x[0]); f(&mut o, x[1]); }
        head(&mut o, 5, nv); for x in &spe { u32_(&mut o, *x); }
        head(&mut o, 4, nv); for x in &dif { u32_(&mut o, *x); }
        head(&mut o, 3, nv); for x in &p.normals { v3(&mut o, *x); }
        head(&mut o, 1, nv); for x in &p.positions { v3(&mut o, *x); }
        head(&mut o, 64, nv); for x in &tan { v3(&mut o, *x); }
        for _ in 0..3 { o.push(1); u32_(&mut o, 0); }
        u32_(&mut o, nodes.len() as u32);
        for (r, c, first, count) in &nodes { f(&mut o, *r); v3(&mut o, *c); u32_(&mut o, *first); u32_(&mut o, *count); }
        o.push(1);
        u32_(&mut o, order.len() as u32);
        for t in &order { u32_(&mut o, *t); }
    }
    let rest = (o.len() - size_at - 4) as u32;
    o[size_at..size_at + 4].copy_from_slice(&rest.to_le_bytes());
    o
}

#[cfg(test)]
mod tests {
    /// A shipped mesh decoded, written again and decoded once more gives the same triangles, positions, normals
    /// and uvs per material.
    #[test]
    fn shipped_meshes_survive_a_rewrite() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let mut n = 0;
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xcmsh")).step_by(61) {
            let Ok(m) = crate::g3_res::decode_xcmsh(&g.read(&k).unwrap()) else { continue };
            let geo = &m.geo;
            if geo.normals.len() != geo.positions.len() { continue; }
            let parts: Vec<super::Part> = geo.submeshes.iter().map(|s| {
                let idx = &geo.indices[s.first_index as usize..(s.first_index + s.index_count) as usize];
                let lo = idx.iter().min().copied().unwrap_or(0);
                let hi = idx.iter().max().copied().unwrap_or(0);
                let r = lo as usize..=hi as usize;
                super::Part {
                    material: s.material.clone(),
                    positions: geo.positions[r.clone()].to_vec(),
                    normals: geo.normals[r.clone()].to_vec(),
                    uvs: if geo.uvs.len() == geo.positions.len() { geo.uvs[r.clone()].to_vec() } else { vec![] },
                    triangles: idx.chunks_exact(3).map(|c| [c[0] - lo, c[1] - lo, c[2] - lo]).collect(),
                    diffuse: m.diffuse[r.clone()].to_vec(),
                    specular: m.specular[r.clone()].to_vec(),
                }
            }).collect();
            let bm = crate::g3_res::decode_xcmsh(&super::write(&parts)).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert_eq!(bm.streams, vec![0, 12, 5, 4, 3, 1, 64], "{k}");
            let cols = |m: &crate::g3_res::G3Mesh| -> Vec<(u32, u32)> { m.geo.indices.iter().map(|&i| (m.diffuse[i as usize], m.specular[i as usize])).collect() };
            assert_eq!(cols(&m), cols(&bm), "{k} colours");
            let back = bm.geo;
            let tri = |g: &crate::geom::MeshGeometry| -> Vec<(String, Vec<[i64; 3]>)> { g.submeshes.iter().filter(|s| s.index_count > 0).map(|s| (s.material.trim_end_matches(".xshmat").to_lowercase(), g.indices[s.first_index as usize..(s.first_index + s.index_count) as usize].iter().map(|&i| g.positions[i as usize].map(|x| (x * 100.0).round() as i64)).collect())).collect() };
            assert_eq!(tri(geo), tri(&back), "{k}");
            n += 1;
        }
        assert!(n > 20, "{n} meshes");
    }
}

#[cfg(test)]
mod tree_tests {
    /// Every triangle sits in exactly one leaf, inside every sphere on its way down, and the counts add up.
    #[test]
    fn the_sphere_tree_holds_every_triangle() {
        let n = 40;
        let pos: Vec<[f32; 3]> = (0..(n + 1) * (n + 1)).map(|i| [(i % (n + 1)) as f32 * 50.0, ((i * 7919) % 13) as f32, (i / (n + 1)) as f32 * 50.0]).collect();
        let mut tris = vec![];
        for y in 0..n { for x in 0..n { let a = (y * (n + 1) + x) as u32; let b = a + 1; let c = a + n as u32 + 1; tris.push([a, c, b]); tris.push([b, c, c + 1]); } }
        let (nodes, order) = super::sphere_tree(&pos, &tris);
        let mut seen = order.clone(); seen.sort(); assert_eq!(seen, (0..tris.len() as u32).collect::<Vec<_>>());
        assert_eq!(nodes[0].3 as usize, nodes.len());
        let leaves: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].3 == 1).collect();
        assert_eq!(nodes[leaves[0]].2, 0);
        for (j, &i) in leaves.iter().enumerate() {
            let end = leaves.get(j + 1).map(|&l| nodes[l].2).unwrap_or(order.len() as u32);
            assert!(end > nodes[i].2 && end - nodes[i].2 <= 16);
            // the leaf and every ancestor (a node whose subtree spans i) hold its triangles
            for a in 0..=i { if a + nodes[a].3 as usize > i { let (r, c, _, _) = nodes[a]; for &t in &order[nodes[i].2 as usize..end as usize] { for &v in &tris[t as usize] { let p = pos[v as usize]; let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt(); assert!(d <= r + 1e-2); } } } }
        }
    }
}
