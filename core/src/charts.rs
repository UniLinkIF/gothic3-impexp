//! The lightmap charts of a mesh element: what ties its triangles to the pages of a lightmap (`.xlmp`).
//!
//! An element's tail, after its vertex streams (versions 3 to 5):
//! ```text
//! u8 1 · u32 n · n × u32 vertex   } vertices and triangles without lightmap (lightmap coordinates −1):
//! u8 1 · u32 n · n × u32 triangle }   lit per vertex
//! u8 1 · u32 charts · per chart (page k of every lightmap of the mesh is chart k):
//!   u8 1 · u32 n · n × u32 vertex · u8 1 · u32 n · n × u32 triangle
//!   · f32 N[3] (the chart's normal)
//!   · f32 M[16], a 4×4 matrix for row vectors: (x, y, z, 1)·M = (u, v, −N·p + w, 1), rows
//!     (U.x V.x −N.x 0) (U.y V.y −N.y 0) (U.z V.z −N.z 0) (t_u t_v w 1)
//!   · f32 u_max · f32 v_max
//! ```
//! (u, v) is the chart's plane in centimetres with its minimum at 0: the vertices' stream 73 (lightmap coordinates).
//! Each placement's lightmap has a page per chart, `w = ⌈u_max / s⌉ + 2` by `h = ⌈v_max / s⌉ + 2` texels at that
//! placement's `UnitsPerLightmapTexel` s (a border of one texel), and per texel row the span of texels it covers.

use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Chart { pub vertices: Vec<u32>, pub triangles: Vec<u32>, pub normal: [f32; 3], pub matrix: [f32; 16], pub max: [f32; 2] }

impl Chart {
    /// (u, v) of a point.
    pub fn uv(&self, p: [f32; 3]) -> [f32; 2] {
        let m = &self.matrix;
        [p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12], p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13]]
    }
    /// The chart's axes U, V, N.
    pub fn axes(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let m = &self.matrix;
        ([m[0], m[4], m[8]], [m[1], m[5], m[9]], self.normal)
    }
    /// A chart of plane axes `u`, `v` (orthonormal), normal `n` and translation `t` (u, v of the origin), reaching `max`.
    pub fn new(vertices: Vec<u32>, triangles: Vec<u32>, u: [f32; 3], v: [f32; 3], n: [f32; 3], t: [f32; 2], max: [f32; 2]) -> Self {
        let matrix = [u[0], v[0], -n[0], 0.0, u[1], v[1], -n[1], 0.0, u[2], v[2], -n[2], 0.0, t[0], t[1], 0.0, 1.0];
        Chart { vertices, triangles, normal: n, matrix, max }
    }
}

/// An element's tail: vertices and triangles without lightmap, and the charts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tail { pub unlit_vertices: Vec<u32>, pub unlit_triangles: Vec<u32>, pub charts: Vec<Chart> }

fn list(d: &[u8], p: &mut usize) -> Result<Vec<u32>> {
    if d.get(*p) != Some(&1) { bail!("chart list at {p}"); }
    let n = d.get(*p + 1..*p + 5).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(|| anyhow::anyhow!("chart list runs past the end"))?;
    let b = d.get(*p + 5..*p + 5 + n * 4).ok_or_else(|| anyhow::anyhow!("chart list runs past the end"))?;
    *p += 5 + n * 4;
    Ok(b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
}

pub fn parse(d: &[u8]) -> Result<Tail> {
    let mut p = 0;
    let (unlit_vertices, unlit_triangles) = (list(d, &mut p)?, list(d, &mut p)?);
    if d.get(p) != Some(&1) { bail!("chart count"); }
    let n = u32::from_le_bytes(d[p + 1..p + 5].try_into().unwrap()) as usize;
    p += 5;
    let mut charts = vec![];
    for _ in 0..n {
        let (vertices, triangles) = (list(d, &mut p)?, list(d, &mut p)?);
        let f: Vec<f32> = d.get(p..p + 84).ok_or_else(|| anyhow::anyhow!("chart runs past the end"))?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        p += 84;
        charts.push(Chart { vertices, triangles, normal: [f[0], f[1], f[2]], matrix: f[3..19].try_into().unwrap(), max: [f[19], f[20]] });
    }
    if p != d.len() { bail!("element tail: {} bytes after the charts", d.len() - p); }
    Ok(Tail { unlit_vertices, unlit_triangles, charts })
}

pub fn write(t: &Tail) -> Vec<u8> {
    let mut o = vec![];
    let list = |o: &mut Vec<u8>, v: &[u32]| { o.push(1); o.extend((v.len() as u32).to_le_bytes()); for x in v { o.extend(x.to_le_bytes()); } };
    list(&mut o, &t.unlit_vertices);
    list(&mut o, &t.unlit_triangles);
    o.push(1);
    o.extend((t.charts.len() as u32).to_le_bytes());
    for c in &t.charts {
        list(&mut o, &c.vertices);
        list(&mut o, &c.triangles);
        for x in c.normal.iter().chain(&c.matrix).chain(&c.max) { o.extend(x.to_le_bytes()); }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    /// Every element tail of the game's meshes reads and writes back byte for byte; every chart's frame gives its
    /// vertices' lightmap coordinates, from 0 up to its stored maximum; and every lightmap made for the mesh has a page
    /// per chart, `⌈max / s⌉ + 2` texels at one texel size s for the whole lightmap.
    #[test]
    fn charts_are_the_lightmaps_pages() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let mut by_mesh: HashMap<String, Vec<String>> = HashMap::new();
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xlmp")) {
            if let Some(m) = crate::lightmap::mesh_path(&g.read(&k).unwrap()) { by_mesh.entry(m.trim_start_matches("/data/_compiledmesh/").to_string()).or_default().push(k); }
        }
        let (mut tails, mut charts, mut verts, mut off, mut lms, mut sized, mut pages) = (0, 0, 0, 0, 0, 0, 0);
        let mut sizes: BTreeMap<String, usize> = BTreeMap::new();
        let mut bad = vec![];
        let mut offs: BTreeMap<String, usize> = BTreeMap::new();
        let (mut far_lit, mut far_off) = (0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xcmsh")) {
            let Ok(m) = crate::g3_res::decode_xcmsh(&g.read(&k).unwrap()) else { continue };
            let l73 = m.extra_uvs.iter().find(|x| x.0 == 73).map(|x| &x.1);
            let mut per_element = vec![];
            for (e, raw) in m.element_tails.iter().enumerate() {
                if raw.is_empty() { per_element.push(vec![]); continue; }
                let t = parse(raw).unwrap_or_else(|err| panic!("{k} element {e}: {err:#}"));
                assert!(write(&t) == *raw, "{k} element {e}: written back differently");
                tails += 1;
                let first = m.elements[e].0 as usize;
                for c in &t.charts {
                    charts += 1;
                    let Some(l73) = l73 else { continue };
                    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
                    for &v in &c.vertices {
                        let (p, q) = (m.geo.positions[first + v as usize], l73[first + v as usize]);
                        let uv = c.uv(p);
                        verts += 1;
                        if (uv[0] - q[0]).abs() > 0.5 || (uv[1] - q[1]).abs() > 0.5 { if k.contains("/g3_world_lowpoly_") { far_off += 1; continue; } off += 1; *offs.entry(k.split('/').nth(1).unwrap().to_string()).or_default() += 1; if bad.len() < 5 { bad.push(format!("{k} e{e}: {uv:?} stored {q:?}")); } }
                        for i in 0..2 { lo[i] = lo[i].min(q[i]); hi[i] = hi[i].max(q[i]); }
                    }
                    if lo[0].abs() > 0.5 || lo[1].abs() > 0.5 || (hi[0] - c.max[0]).abs() > 0.5 || (hi[1] - c.max[1]).abs() > 0.5 { if bad.len() < 10 { bad.push(format!("{k} e{e}: span {lo:?}..{hi:?} max {:?}", c.max)); } }
                }
                per_element.push(t.charts);
            }
            let path = g.path_of(&k).unwrap().to_lowercase();
            if k.contains("/g3_world_lowpoly_") { far_lit += by_mesh.get(&path).map_or(0, Vec::len); }
            for lk in by_mesh.get(&path).into_iter().flatten() {
                let lm = crate::lightmap::parse(&g.read(lk).unwrap()).unwrap();
                lms += 1;
                // Every texel size that fits every page of this lightmap.
                let (mut lo, mut hi) = (0f32, f32::MAX);
                let mut all = true;
                for (e, p) in lm.pages.iter().enumerate() {
                    let n = u16::from_le_bytes([p[0], p[1]]) as usize;
                    let cs = per_element.get(e).map(Vec::as_slice).unwrap_or(&[]);
                    if n == 0 { continue; }
                    if n != cs.len() { all = false; if bad.len() < 15 { bad.push(format!("{lk} e{e}: {n} pages, {} charts", cs.len())); } continue; }
                    let mut q = 2;
                    for c in cs {
                        let u = |o: usize| u32::from_le_bytes(p[q + o..q + o + 4].try_into().unwrap()) as f32;
                        let (w, h, nb) = (u(18), u(22), u(26) as usize);
                        pages += 1;
                        // ⌈max / s⌉ = w − 2  ⇔  max / (w − 2) ≤ s < max / (w − 3)
                        for (mx, n) in [(c.max[0], w), (c.max[1], h)] {
                            if n < 3.0 { all = false; continue; }
                            lo = lo.max(mx / (n - 2.0) - 1e-3);
                            if n > 3.0 { hi = hi.min(mx / (n - 3.0)); }
                        }
                        q += 30 + nb;
                        if nb > 0 { let mm = u32::from_le_bytes(p[q..q + 4].try_into().unwrap()) as usize; assert_eq!(mm, 4 * h as usize, "{lk}: a span per row"); q += 4 + mm; }
                    }
                }
                if all && lo <= hi { sized += 1; *sizes.entry(format!("{:.2}", (lo + hi.min(lo * 2.0)) / 2.0)).or_default() += 0; *sizes.entry(format!("{lo:.1}..{:.1}", hi.min(9999.0))).or_default() += 1; }
                else if bad.len() < 20 { bad.push(format!("{lk}: no one texel size ({lo}..{hi})")); }
            }
        }
        sizes.retain(|_, v| *v > 0);
        let mut top: Vec<(&String, &usize)> = sizes.iter().collect(); top.sort_by(|a, b| b.1.cmp(a.1));
        eprintln!("off by folder {offs:?}; far versions: {far_off} vertices off, {far_lit} lightmaps");
        eprintln!("{tails} tails, {charts} charts, {verts} chart vertices ({off} off their frame); {lms} lightmaps, {sized} with one texel size over their {pages} pages; sizes {:?}\n{bad:#?}", &top[..top.len().min(12)]);
        // The far versions (G3_World_Lowpoly_*) keep charts their coordinates no longer follow; nothing lights them by pages.
        assert!(tails > 1000 && charts > 10_000 && off == 0 && far_lit == 0 && sized == lms, "{bad:#?}");
    }
}
