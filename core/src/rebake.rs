//! New lightmap pages for a part whose shape changed. The part gets charts of its own (planar groups of neighbouring
//! triangles, see `charts`), and each placement that lit the old shape by pages gets a page per chart, its texels
//! taken from the old pages at the same place in the model (the nearest old texel facing the same way), else from the
//! old per-vertex light. The game is not asked to bake anything.
//!
//! Texel (i, j) of a page `w` × `h` stands for (u, v) = ((i + ½ − 1) / (w − 2) · u_max, (j + ½ − 1) / (h − 2) · v_max)
//! — one texel of border, the chart's stored maximum filling the rest; per row the span of texels the chart's
//! triangles touch, grown by one texel all round (what most of the game's spans are, see the test).

use crate::charts::{Chart, Tail};
use crate::xcmsh_write::Part;
use std::collections::HashMap;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }
fn unit(a: [f32; 3]) -> Option<[f32; 3]> { let l = dot(a, a).sqrt(); (l > 1e-12).then(|| a.map(|x| x / l)) }

/// Largest side of a chart (cm); bigger groups are cut.
const MAX_SIDE: f32 = 3000.0;
/// Neighbours join a chart while they face within this of its first triangle.
const COS_ANGLE: f32 = 0.82;

/// Charts for `p`, which takes them on: its vertices are split where charts meet (a vertex has one lightmap
/// coordinate), its stream 73 is set, and a triangle without area is left unlit (−1, as the game does).
pub fn make_charts(p: &mut Part) -> Tail {
    let nt = p.triangles.len();
    let has_n = p.normals.len() == p.positions.len();
    // Face normals, turned to the side the vertex normals face.
    let mut fnorm = vec![[0f32; 3]; nt];
    let mut area = vec![0f32; nt];
    for (t, tri) in p.triangles.iter().enumerate() {
        let [a, b, c] = tri.map(|i| p.positions[i as usize]);
        let n = cross(sub(b, a), sub(c, a));
        area[t] = dot(n, n).sqrt() / 2.0;
        if let Some(mut n) = unit(n) {
            if has_n { let vn = tri.iter().fold([0f32; 3], |s, &i| { let q = p.normals[i as usize]; [s[0] + q[0], s[1] + q[1], s[2] + q[2]] }); if dot(n, vn) < 0.0 { n = n.map(|x| -x); } }
            fnorm[t] = n;
        }
    }
    // Neighbours across an edge (by position).
    let key = |i: u32| p.positions[i as usize].map(f32::to_bits);
    let mut by_edge: HashMap<([u32; 3], [u32; 3]), Vec<usize>> = HashMap::new();
    for (t, tri) in p.triangles.iter().enumerate() {
        for k in 0..3 { let (a, b) = (key(tri[k]), key(tri[(k + 1) % 3])); by_edge.entry(if a < b { (a, b) } else { (b, a) }).or_default().push(t); }
    }
    let mut chart = vec![usize::MAX; nt];
    let mut groups: Vec<Vec<usize>> = vec![];
    for seed in 0..nt {
        if chart[seed] != usize::MAX || area[seed] < 1e-4 { continue; }
        let g = groups.len();
        let n0 = fnorm[seed];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut members = vec![];
        let mut stack = vec![seed];
        chart[seed] = g;
        while let Some(t) = stack.pop() {
            let tri = p.triangles[t];
            let (mut l2, mut h2) = (lo, hi);
            for &i in &tri { let q = p.positions[i as usize]; for k in 0..3 { l2[k] = l2[k].min(q[k]); h2[k] = h2[k].max(q[k]); } }
            if !members.is_empty() && (0..3).any(|k| h2[k] - l2[k] > MAX_SIDE) { chart[t] = usize::MAX; continue; }
            (lo, hi) = (l2, h2);
            members.push(t);
            for k in 0..3 {
                let (a, b) = (key(tri[k]), key(tri[(k + 1) % 3]));
                for &o in &by_edge[&if a < b { (a, b) } else { (b, a) }] {
                    if chart[o] == usize::MAX && area[o] >= 1e-4 && dot(fnorm[o], n0) > COS_ANGLE { chart[o] = g; stack.push(o); }
                }
            }
        }
        groups.push(members);
    }
    // Vertices per (vertex, chart).
    let old = std::mem::take(&mut p.positions);
    let (old_n, old_uv) = (std::mem::take(&mut p.normals), std::mem::take(&mut p.uvs));
    let old_x: Vec<(u32, Vec<[f32; 2]>)> = std::mem::take(&mut p.extra).into_iter().filter(|(t, _)| *t != 73).collect();
    let (old_d, old_s) = (std::mem::take(&mut p.diffuse), std::mem::take(&mut p.specular));
    p.extra = old_x.iter().map(|(t, _)| (*t, vec![])).collect();
    let mut at: HashMap<(u32, usize), u32> = HashMap::new();
    let mut tail = Tail::default();
    let mut chart_v: Vec<Vec<u32>> = vec![vec![]; groups.len()];
    for t in 0..nt {
        let c = chart[t];
        let tri = p.triangles[t];
        p.triangles[t] = tri.map(|i| *at.entry((i, c)).or_insert_with(|| {
            let j = p.positions.len() as u32;
            p.positions.push(old[i as usize]);
            if old_n.len() == old.len() { p.normals.push(old_n[i as usize]); }
            if old_uv.len() == old.len() { p.uvs.push(old_uv[i as usize]); }
            for (k, (_, v)) in old_x.iter().enumerate() { if v.len() == old.len() { p.extra[k].1.push(v[i as usize]); } }
            if old_d.len() == old.len() { p.diffuse.push(old_d[i as usize]); }
            if old_s.len() == old.len() { p.specular.push(old_s[i as usize]); }
            if c == usize::MAX { tail.unlit_vertices.push(j); } else { chart_v[c].push(j); }
            j
        }));
        if c == usize::MAX { tail.unlit_triangles.push(t as u32); }
    }
    p.extra.retain(|(_, v)| v.len() == p.positions.len());
    // A frame per chart: its normal, the plane's main direction as U, V = U × N; (u, v) from 0.
    let mut l73 = vec![[-1.0f32, -1.0]; p.positions.len()];
    for (c, members) in groups.iter().enumerate() {
        let mut n = [0f32; 3];
        for &t in members { for k in 0..3 { n[k] += fnorm[t][k] * area[t]; } }
        let n = unit(n).unwrap_or(fnorm[members[0]]);
        let a = unit(cross(n, if n[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] })).unwrap();
        let b = cross(n, a);
        let pts: Vec<[f32; 2]> = chart_v[c].iter().map(|&v| { let q = p.positions[v as usize]; [dot(q, a), dot(q, b)] }).collect();
        let m = pts.iter().fold([0f32; 2], |s, q| [s[0] + q[0], s[1] + q[1]]).map(|x| x / pts.len().max(1) as f32);
        let (mut cxx, mut cyy, mut cxy) = (0f32, 0f32, 0f32);
        for q in &pts { let (x, y) = (q[0] - m[0], q[1] - m[1]); cxx += x * x; cyy += y * y; cxy += x * y; }
        let th = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
        let u = [0, 1, 2].map(|k| a[k] * th.cos() + b[k] * th.sin());
        let v = cross(u, n);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for &j in &chart_v[c] { let q = p.positions[j as usize]; let w = [dot(q, u), dot(q, v)]; for k in 0..2 { lo[k] = lo[k].min(w[k]); hi[k] = hi[k].max(w[k]); } }
        let t = [-lo[0], -lo[1]];
        for &j in &chart_v[c] { let q = p.positions[j as usize]; l73[j as usize] = [dot(q, u) + t[0], dot(q, v) + t[1]]; }
        let mut tris: Vec<u32> = members.iter().map(|&t| t as u32).collect();
        tris.sort();
        tail.charts.push(Chart::new(chart_v[c].clone(), tris, u, v, n, t, [hi[0] - lo[0], hi[1] - lo[1]]));
    }
    p.extra.push((73, l73));
    p.extra.sort_by_key(|x| x.0);
    tail
}

/// The texel size of a placement's lightmap: the size every page of it (with pages) fits, `⌈max / s⌉ + 2`.
pub fn texel_size(tails: &[Option<Tail>], lm: &crate::lightmap::Lightmap) -> Option<f32> {
    let (mut lo, mut hi) = (0f32, f32::MAX);
    let mut any = false;
    for (e, raw) in lm.pages.iter().enumerate() {
        let Some(Some(t)) = tails.get(e) else { continue };
        for (c, (w, h, _, _)) in t.charts.iter().zip(read_pages(raw)) {
            for (mx, n) in [(c.max[0], w as f32), (c.max[1], h as f32)] {
                if n < 3.0 || mx <= 0.0 { continue; }
                any = true;
                lo = lo.max(mx / (n - 2.0));
                if n > 3.0 { hi = hi.min(mx / (n - 3.0)); }
            }
        }
    }
    any.then_some(if lo <= hi { lo.max(1.0) } else { lo })
}

/// The pages of one element as stored: (w, h, texels, spans) each.
pub fn read_pages(raw: &[u8]) -> Vec<(usize, usize, Vec<u32>, Vec<(u16, u16)>)> {
    let u = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap()) as usize;
    let n = u16::from_le_bytes([raw[0], raw[1]]) as usize;
    let mut q = 2;
    let mut out = vec![];
    for _ in 0..n {
        let (w, h, nb) = (u(q + 18), u(q + 22), u(q + 26));
        let px = raw[q + 30..q + 30 + nb].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        q += 30 + nb;
        let mut spans = vec![];
        if nb > 0 { let m = u(q); spans = raw[q + 4..q + 4 + m].chunks_exact(4).map(|c| (u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]]))).collect(); q += 4 + m; }
        out.push((w, h, px, spans));
    }
    out
}

/// (u, v) of the centre of texel (i, j) of a `w` × `h` page of `c`.
fn texel_uv(c: &Chart, w: usize, h: usize, i: usize, j: usize) -> [f32; 2] {
    [(i as f32 - 0.5) / (w as f32 - 2.0).max(1.0) * c.max[0], (j as f32 - 0.5) / (h as f32 - 2.0).max(1.0) * c.max[1]]
}

/// The point of chart `c` at (u, v), on the plane through `on`.
fn point(c: &Chart, uv: [f32; 2], on: [f32; 3]) -> [f32; 3] {
    let (u, v, n) = c.axes();
    let m = &c.matrix;
    let d = dot(on, n);
    [0, 1, 2].map(|k| u[k] * (uv[0] - m[12]) + v[k] * (uv[1] - m[13]) + n[k] * d)
}

/// The lit texels of a placement's old pages: points in the model, their facing and colour.
pub struct OldTexels { pub points: Vec<[f32; 3]>, pub normals: Vec<[f32; 3]>, pub colours: Vec<u32> }

pub fn old_texels(orig: &crate::g3_res::G3Mesh, tails: &[Option<Tail>], lm: &crate::lightmap::Lightmap) -> OldTexels {
    let mut o = OldTexels { points: vec![], normals: vec![], colours: vec![] };
    for (e, raw) in lm.pages.iter().enumerate() {
        let (Some(Some(t)), Some(&(first, _))) = (tails.get(e), orig.elements.get(e)) else { continue };
        let pages = read_pages(raw);
        if pages.len() != t.charts.len() { continue; }
        for (c, (w, h, px, spans)) in t.charts.iter().zip(pages) {
            if px.len() != w * h || c.vertices.is_empty() { continue; }
            let on = orig.geo.positions[first as usize + c.vertices[0] as usize];
            for j in 0..h {
                let (a, b) = spans.get(j).map_or((0, w), |s| (s.0 as usize, (s.1 as usize).min(w)));
                for i in a..b {
                    o.points.push(point(c, texel_uv(c, w, h, i, j), on));
                    o.normals.push(c.normal);
                    o.colours.push(px[j * w + i]);
                }
            }
        }
    }
    o
}

fn tri_box(t: [[f32; 2]; 3], lo: [f32; 2], hi: [f32; 2]) -> bool {
    for k in 0..2 { if t.iter().all(|p| p[k] < lo[k]) || t.iter().all(|p| p[k] > hi[k]) { return false; } }
    let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [lo[0], hi[1]], [hi[0], hi[1]]];
    for i in 0..3 {
        let (a, b, c) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
        let n = [b[1] - a[1], a[0] - b[0]];
        let d = |p: [f32; 2]| n[0] * (p[0] - a[0]) + n[1] * (p[1] - a[1]);
        let side = d(c);
        if corners.iter().all(|&q| d(q) * side < 0.0) { return false; }
    }
    true
}

/// The pages (as stored, from the u16 count on) of part `p` with charts `tail`, at texel size `s`, lit from `old`
/// (or, where it has nothing facing that way, `fallback`: a colour per vertex of `p`). Without old texels the pages
/// carry none, as the game's own pages without texels do.
pub fn pages(p: &Part, tail: &Tail, s: f32, old: &OldTexels, fallback: &[u32]) -> Vec<u8> {
    let l73 = p.extra.iter().find(|x| x.0 == 73).map(|x| x.1.as_slice()).unwrap_or(&[]);
    // The old texels by the axis their chart faces most, and all of them.
    let axis = |n: [f32; 3]| { let k = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap(); k * 2 + (n[k] < 0.0) as usize };
    let all = crate::export::Nearest::new(&old.points, 0..old.points.len() as u32);
    let by_axis: Vec<crate::export::Nearest> = (0..6).map(|a| crate::export::Nearest::new(&old.points, (0..old.points.len() as u32).filter(|&i| axis(old.normals[i as usize]) == a))).collect();
    let verts = crate::export::Nearest::new(&p.positions, 0..p.positions.len() as u32);
    let mut o = (tail.charts.len() as u16).to_le_bytes().to_vec();
    for (k, c) in tail.charts.iter().enumerate() {
        let w = (c.max[0] / s).ceil().max(1.0) as usize + 2;
        let h = (c.max[1] / s).ceil().max(1.0) as usize + 2;
        o.extend([0x44, 0x33, 0x22, 0x11]);
        o.extend(1u16.to_le_bytes());
        o.extend((k as u32).to_le_bytes());
        o.extend(u32::MAX.to_le_bytes());
        o.extend(u32::MAX.to_le_bytes());
        o.extend((w as u32).to_le_bytes());
        o.extend((h as u32).to_le_bytes());
        if old.points.is_empty() { o.extend(0u32.to_le_bytes()); continue; }
        // Texels the chart's triangles touch, grown by one all round.
        let tris: Vec<[[f32; 2]; 3]> = c.triangles.iter().map(|&t| p.triangles[t as usize].map(|i| l73[i as usize])).collect();
        let (su, sv) = (c.max[0] / (w as f32 - 2.0), c.max[1] / (h as f32 - 2.0));
        let mut hit = vec![false; w * h];
        for j in 0..h { for i in 0..w {
            let lo = [(i as f32 - 1.0) * su, (j as f32 - 1.0) * sv];
            if tris.iter().any(|t| tri_box(*t, lo, [lo[0] + su, lo[1] + sv])) { hit[j * w + i] = true; }
        } }
        let mut grown = hit.clone();
        for j in 0..h { for i in 0..w { if hit[j * w + i] { for y in j.saturating_sub(1)..(j + 2).min(h) { for x in i.saturating_sub(1)..(i + 2).min(w) { grown[y * w + x] = true; } } } } }
        let on = p.positions[c.vertices[0] as usize];
        let ax = &by_axis[axis(c.normal)];
        o.extend(((w * h * 4) as u32).to_le_bytes());
        for j in 0..h { for i in 0..w {
            let q = point(c, texel_uv(c, w, h, i, j), on);
            let col = ax.find(q).filter(|&t| dot(old.normals[t as usize], c.normal) > 0.5)
                .map(|t| old.colours[t as usize])
                .or_else(|| verts.find(q).and_then(|v| fallback.get(v as usize).copied()))
                .or_else(|| all.find(q).map(|t| old.colours[t as usize]))
                .unwrap_or(0xff80_8080);
            o.extend(col.to_le_bytes());
        } }
        o.extend(((h * 4) as u32).to_le_bytes());
        for j in 0..h {
            let row: Vec<usize> = (0..w).filter(|&i| grown[j * w + i]).collect();
            let (a, b) = row.first().zip(row.last()).map_or((0, 0), |(a, b)| (*a as u16, *b as u16 + 1));
            o.extend(a.to_le_bytes());
            o.extend(b.to_le_bytes());
        }
    }
    o
}

#[cfg(test)]
mod tests {
    /// The game's own charts, given new pages here from the game's pages: the pages come out the size the game's are
    /// (the texel size read back from them), their spans mostly the game's, and their texels the game's light.
    #[test]
    fn pages_made_for_the_games_charts_are_the_games() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut pages, mut sized, mut rows, mut same_rows, mut diff, mut texels, mut lms) = (0, 0, 0, 0, 0f64, 0, 0);
        for lk in g.keys().into_iter().filter(|k| k.ends_with(".xlmp")).step_by(3) {
            let d = g.read(&lk).unwrap();
            let lm = crate::lightmap::parse(&d).unwrap();
            if !(0..lm.count).any(|e| lm.has_pages(e)) { continue; }
            let Some(mk) = crate::lightmap::mesh_path(&d).and_then(|p| g.find("_compiledmesh", p.rsplit('/').next().unwrap())) else { continue };
            let m = crate::g3_res::decode_xcmsh(&g.read(&mk).unwrap()).unwrap();
            let Some(l73) = m.uv_extra.iter().find(|x| x.0 == 73).map(|x| &x.1) else { continue };
            let tails: Vec<Option<crate::charts::Tail>> = m.element_tails.iter().map(|t| crate::charts::parse(t).ok()).collect();
            let Some(s) = super::texel_size(&tails, &lm) else { continue };
            let old = super::old_texels(&m, &tails, &lm);
            lms += 1;
            if lms > 40 { break; }
            for (e, t) in tails.iter().enumerate() {
                let Some(t) = t else { continue };
                if !lm.has_pages(e) || t.charts.is_empty() { continue; }
                let (first, n) = (m.elements[e].0 as usize, m.elements[e].1 as usize);
                let sm = &m.geo.submeshes[e];
                let part = crate::xcmsh_write::Part {
                    material: String::new(), positions: m.geo.positions[first..first + n].to_vec(), normals: vec![], uvs: vec![],
                    triangles: m.geo.indices[sm.first_index as usize..(sm.first_index + sm.index_count) as usize].chunks_exact(3).map(|c| [c[0], c[1], c[2]].map(|i| i - first as u32)).collect(),
                    diffuse: vec![], specular: vec![], extra: vec![(73, l73[first..first + n].to_vec())], tail: vec![],
                };
                let ours = super::read_pages(&super::pages(&part, t, s, &old, &[]));
                for (a, b) in ours.iter().zip(super::read_pages(&lm.pages[e])) {
                    pages += 1;
                    if (a.0, a.1) != (b.0, b.1) { continue; }
                    sized += 1;
                    if b.2.is_empty() { continue; }
                    rows += b.1;
                    same_rows += a.3.iter().zip(&b.3).filter(|(x, y)| x == y).count();
                    for j in 0..b.1 { let (lo, hi) = (b.3[j].0 as usize, (b.3[j].1 as usize).min(b.0)); for i in lo..hi {
                        let (x, y) = (a.2[j * b.0 + i], b.2[j * b.0 + i]);
                        diff += (0..3).map(|k| ((x >> (k * 8) & 255) as f64 - (y >> (k * 8) & 255) as f64).abs()).sum::<f64>() / 3.0;
                        texels += 1;
                    } }
                }
            }
        }
        eprintln!("{lms} lightmaps: {sized} of {pages} pages the game's size; spans {same_rows} of {rows} rows; texels mean |Δ| {:.2} over {texels}", diff / texels.max(1) as f64);
        assert!(lms > 30 && sized * 100 >= pages * 99 && same_rows * 10 >= rows * 6 && diff / (texels as f64) < 4.0);
    }

    /// Charts made for a building's element: every triangle with area in one chart, the rest unlit; every chart
    /// vertex's lightmap coordinates are its frame's, from 0 to the chart's maximum, and the tail reads back.
    #[test]
    fn charts_made_for_a_model_hold_every_triangle() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let m = crate::g3_res::decode_xcmsh(&g.read(&crate::staticmesh::mesh_key(&g, "G3_Myrtana_House_Mill_Water_01").unwrap()).unwrap()).unwrap();
        for (e, sm) in m.geo.submeshes.iter().enumerate() {
            let (first, n) = (m.elements[e].0 as usize, m.elements[e].1 as usize);
            let mut p = crate::xcmsh_write::Part { material: String::new(), positions: m.geo.positions[first..first + n].to_vec(), normals: m.geo.normals[first..first + n].to_vec(), uvs: vec![],
                triangles: m.geo.indices[sm.first_index as usize..(sm.first_index + sm.index_count) as usize].chunks_exact(3).map(|c| [c[0], c[1], c[2]].map(|i| i - first as u32)).collect(), diffuse: vec![], specular: vec![], extra: vec![], tail: vec![] };
            let nt = p.triangles.len();
            let t = super::make_charts(&mut p);
            assert_eq!(crate::charts::parse(&crate::charts::write(&t)).unwrap(), t);
            let mut seen = vec![0; nt];
            for c in &t.charts { for &x in &c.triangles { seen[x as usize] += 1; } }
            for &x in &t.unlit_triangles { seen[x as usize] += 1; }
            assert!(seen.iter().all(|&k| k == 1), "element {e}: a triangle in none or two charts");
            let l73 = &p.extra.iter().find(|x| x.0 == 73).unwrap().1;
            assert_eq!(l73.len(), p.positions.len());
            for c in &t.charts {
                assert!(c.max[0] <= super::MAX_SIDE * 1.8 && c.max[1] <= super::MAX_SIDE * 1.8);
                for &v in &c.vertices { let (a, b) = (c.uv(p.positions[v as usize]), l73[v as usize]); assert!((0..2).all(|k| (a[k] - b[k]).abs() < 0.05 && b[k] > -0.05 && b[k] <= c.max[k] + 0.05), "{a:?} {b:?}"); }
                for &x in &c.triangles { assert!(p.triangles[x as usize].iter().all(|v| c.vertices.contains(v))); }
            }
        }
    }
}
