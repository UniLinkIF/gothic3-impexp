//! PhysX cooked triangle-mesh streams (`NXS\x01MESH`), as Gothic 3 keeps them in `.xnvmsh`: the same layout as
//! Risen's PhysX 2.8 streams, with the version fields (stream and OPC model) at 0.
//!
//! ```text
//! stream   "NXS\x01" "MESH" u32 0 · u32 flags (1 materials, 2 face remap, 8 / 16 = 8- / 16-bit indices)
//!          · f32 convex edge threshold · u32 0xFF (no height field) · f32 0
//!          · u32 nv · u32 nt · f32 vertex[3·nv] (metres, game axes) · indices[3·nt]
//!          · [u16 material[nt]] · [u32 max · remap[nt] (u8/u16/u32 by max): new → original triangle]
//!          · u32 convex parts · u32 flat parts · [u16 convex part[nt]] · [flat part[nt]: u8 if < 256 parts, else u16]
//!          · u32 model bytes · collision tree (below)
//!          · f32 geometric epsilon · f32 sphere[4] (centre, radius) · f32 AABB[6] (min, max)
//!          · f32 mass (volume, density 1) · f32 inertia[9] · f32 centre of mass[3]
//!          · u32 nt · u8 edge flags[nt]
//! model    "OPC\x01" u32 0 · u32 code (4 = single leaf, 3 = quantized no-leaf tree)
//!          · [u32 nodes · nodes × 20 bytes · f32 centre coeff[3] · f32 extents coeff[3]]
//!          · "HBM\x01" u32 0 · u32 leaves · [u32 max · leaf[leaves] (u8/u16/u32 by max)] · u32 0
//! node     i16 centre[3] · u16 extents[3] (× the coefficients) · u32 a · u32 b
//!          a = 0xDEAD: the first child is the next node; else bit 31: the first child is leaf
//!          (a & 0x3FFFFFFF), bit 30: the second child is the leaf after it.
//!          b = number of nodes below this one (so the second child node sits at i + 1 + size of the first).
//! leaf     triangle index << 4 | (count − 1); leaves cover the (remapped) triangles in order.
//! ```

use anyhow::{bail, ensure, Result};

pub struct Reader<'a> { pub d: &'a [u8], pub at: usize }
impl<'a> Reader<'a> {
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let Some(b) = self.d.get(self.at..self.at + n) else { bail!("runs past end: offset 0x{:x} + {n}", self.at) };
        self.at += n;
        Ok(b)
    }
    pub fn u8(&mut self) -> Result<u8> { Ok(self.bytes(1)?[0]) }
    pub fn u16(&mut self) -> Result<u16> { let b = self.bytes(2)?; Ok(u16::from_le_bytes([b[0], b[1]])) }
    pub fn u32(&mut self) -> Result<u32> { let b = self.bytes(4)?; Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])) }
}

/// The stream's and the collision model's (OPC) version fields: 0 in Gothic 3, 1 in Risen.
pub const VERSION: u32 = 0;

#[derive(Debug, Clone, PartialEq)]
pub struct Node { pub center: [i16; 3], pub extents: [u16; 3], pub a: u32, pub b: u32 }

#[derive(Debug, Clone, PartialEq)]
pub struct Model { pub code: u32, pub nodes: Vec<Node>, pub coeffs: [f32; 6], pub leaves: Vec<u32> }

#[derive(Debug, Clone, PartialEq)]
pub struct TriMesh {
    pub flags: u32,
    pub edge_threshold: f32,
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    pub materials: Option<Vec<u16>>,
    pub remap: Option<Vec<u32>>,
    pub convex_parts: u32,
    pub flat_parts: u32,
    pub convex_part: Vec<u16>,
    pub flat_part: Vec<u16>,
    pub model: Model,
    pub geom_epsilon: f32,
    pub sphere: [f32; 4],
    pub aabb: [f32; 6],
    pub mass: f32,
    pub inertia: [f32; 9],
    pub com: [f32; 3],
    pub edge_flags: Vec<u8>,
}

pub const F_MATERIALS: u32 = 1;
pub const F_REMAP: u32 = 2;
pub const F_8BIT: u32 = 8;
pub const F_16BIT: u32 = 16;

fn f32r(r: &mut Reader) -> Result<f32> { Ok(f32::from_bits(r.u32()?)) }
fn arr<const N: usize>(r: &mut Reader) -> Result<[f32; N]> { let mut a = [0f32; N]; for x in &mut a { *x = f32r(r)?; } Ok(a) }

fn width_for(max: u32) -> usize { if max < 256 { 1 } else if max < 65536 { 2 } else { 4 } }
fn read_uint(r: &mut Reader, w: usize) -> Result<u32> { Ok(match w { 1 => r.u8()? as u32, 2 => r.u16()? as u32, _ => r.u32()? }) }
fn put_uint(o: &mut Vec<u8>, w: usize, v: u32) { match w { 1 => o.push(v as u8), 2 => o.extend((v as u16).to_le_bytes()), _ => o.extend(v.to_le_bytes()) } }

/// `u32 max` + values sized by max (remap tables, leaf lists).
fn read_packed(r: &mut Reader, n: usize) -> Result<Vec<u32>> {
    let max = r.u32()?;
    let w = width_for(max);
    (0..n).map(|_| read_uint(r, w)).collect()
}
fn put_packed(o: &mut Vec<u8>, v: &[u32]) {
    let max = v.iter().copied().max().unwrap_or(0);
    o.extend(max.to_le_bytes());
    let w = width_for(max);
    for &x in v { put_uint(o, w, x); }
}

fn read_model(r: &mut Reader) -> Result<Model> {
    ensure!(r.bytes(4)? == b"OPC\x01", "no OPC header: offset 0x{:x}", r.at - 4);
    ensure!(r.u32()? == VERSION, "OPC version");
    let code = r.u32()?;
    let (mut nodes, mut coeffs) = (vec![], [0f32; 6]);
    if code != 4 {
        let n = r.u32()? as usize;
        for _ in 0..n {
            let c = [r.u16()? as i16, r.u16()? as i16, r.u16()? as i16];
            let e = [r.u16()?, r.u16()?, r.u16()?];
            nodes.push(Node { center: c, extents: e, a: r.u32()?, b: r.u32()? });
        }
        coeffs = arr(r)?;
    }
    ensure!(r.bytes(4)? == b"HBM\x01", "no HBM header: offset 0x{:x}", r.at - 4);
    ensure!(r.u32()? == 0, "HBM version");
    let nl = r.u32()? as usize;
    let leaves = if nl > 1 { read_packed(r, nl)? } else { vec![0; nl] };
    ensure!(r.u32()? == 0, "HBM primitive table is not empty: offset 0x{:x}", r.at - 4);
    Ok(Model { code, nodes, coeffs, leaves })
}

fn write_model(m: &Model) -> Vec<u8> {
    let mut o = b"OPC\x01".to_vec();
    o.extend(VERSION.to_le_bytes());
    o.extend(m.code.to_le_bytes());
    if m.code != 4 {
        o.extend((m.nodes.len() as u32).to_le_bytes());
        for n in &m.nodes {
            for c in n.center { o.extend(c.to_le_bytes()); }
            for e in n.extents { o.extend(e.to_le_bytes()); }
            o.extend(n.a.to_le_bytes());
            o.extend(n.b.to_le_bytes());
        }
        for c in m.coeffs { o.extend(c.to_le_bytes()); }
    }
    o.extend(b"HBM\x01");
    o.extend(0u32.to_le_bytes());
    o.extend((m.leaves.len() as u32).to_le_bytes());
    if m.leaves.len() > 1 { put_packed(&mut o, &m.leaves); }
    o.extend(0u32.to_le_bytes());
    o
}

pub fn read_trimesh(r: &mut Reader) -> Result<TriMesh> {
    let at = r.at;
    ensure!(r.bytes(8)? == b"NXS\x01MESH", "no NXS MESH header: offset 0x{at:x}");
    ensure!(r.u32()? == VERSION, "NXS version");
    let flags = r.u32()?;
    let edge_threshold = f32r(r)?;
    ensure!(r.u32()? == 0xff && f32r(r)? == 0.0, "height-field meshes are not supported");
    let (nv, nt) = (r.u32()? as usize, r.u32()? as usize);
    let verts = (0..nv).map(|_| arr::<3>(r)).collect::<Result<Vec<_>>>()?;
    let w = if flags & F_8BIT != 0 { 1 } else if flags & F_16BIT != 0 { 2 } else { 4 };
    let tris = (0..nt).map(|_| Ok([read_uint(r, w)?, read_uint(r, w)?, read_uint(r, w)?])).collect::<Result<Vec<_>>>()?;
    let materials = if flags & F_MATERIALS != 0 { Some((0..nt).map(|_| r.u16()).collect::<Result<Vec<_>>>()?) } else { None };
    let remap = if flags & F_REMAP != 0 { Some(read_packed(r, nt)?) } else { None };
    let (convex_parts, flat_parts) = (r.u32()?, r.u32()?);
    let convex_part = if convex_parts > 0 { (0..nt).map(|_| r.u16()).collect::<Result<Vec<_>>>()? } else { vec![] };
    let flat_part = if flat_parts > 0 { let fw = if flat_parts < 256 { 1 } else { 2 }; (0..nt).map(|_| Ok(read_uint(r, fw)? as u16)).collect::<Result<Vec<_>>>()? } else { vec![] };
    let model_bytes = r.u32()? as usize;
    let m_at = r.at;
    let model = read_model(r)?;
    ensure!(r.at - m_at == model_bytes, "model is {} bytes, header says {model_bytes}", r.at - m_at);
    let geom_epsilon = f32r(r)?;
    let sphere = arr(r)?;
    let aabb = arr(r)?;
    let mass = f32r(r)?;
    let inertia = arr(r)?;
    let com = arr(r)?;
    let ne = r.u32()? as usize;
    let edge_flags = r.bytes(ne)?.to_vec();
    Ok(TriMesh { flags, edge_threshold, verts, tris, materials, remap, convex_parts, flat_parts, convex_part, flat_part, model, geom_epsilon, sphere, aabb, mass, inertia, com, edge_flags })
}

pub fn write_trimesh(m: &TriMesh) -> Vec<u8> {
    let mut o = b"NXS\x01MESH".to_vec();
    let u = |o: &mut Vec<u8>, v: u32| o.extend(v.to_le_bytes());
    let f = |o: &mut Vec<u8>, v: f32| o.extend(v.to_le_bytes());
    u(&mut o, VERSION);
    u(&mut o, m.flags);
    f(&mut o, m.edge_threshold);
    u(&mut o, 0xff);
    f(&mut o, 0.0);
    u(&mut o, m.verts.len() as u32);
    u(&mut o, m.tris.len() as u32);
    for v in &m.verts { for &x in v { f(&mut o, x); } }
    let w = if m.flags & F_8BIT != 0 { 1 } else if m.flags & F_16BIT != 0 { 2 } else { 4 };
    for t in &m.tris { for &i in t { put_uint(&mut o, w, i); } }
    if let Some(ms) = &m.materials { for &x in ms { o.extend(x.to_le_bytes()); } }
    if let Some(r) = &m.remap { put_packed(&mut o, r); }
    u(&mut o, m.convex_parts);
    u(&mut o, m.flat_parts);
    if m.convex_parts > 0 { for &x in &m.convex_part { o.extend(x.to_le_bytes()); } }
    if m.flat_parts > 0 { let fw = if m.flat_parts < 256 { 1 } else { 2 }; for &x in &m.flat_part { put_uint(&mut o, fw, x as u32); } }
    let model = write_model(&m.model);
    u(&mut o, model.len() as u32);
    o.extend(model);
    f(&mut o, m.geom_epsilon);
    for x in m.sphere.iter().chain(&m.aabb) { f(&mut o, *x); }
    f(&mut o, m.mass);
    for x in m.inertia.iter().chain(&m.com) { f(&mut o, *x); }
    u(&mut o, m.edge_flags.len() as u32);
    o.extend(&m.edge_flags);
    o
}

pub fn mass_properties(verts: &[[f32; 3]], tris: &[[u32; 3]]) -> (f64, [f64; 3], [f64; 9]) {
    let mult = [1.0 / 6.0, 1.0 / 24.0, 1.0 / 24.0, 1.0 / 24.0, 1.0 / 60.0, 1.0 / 60.0, 1.0 / 60.0, 1.0 / 120.0, 1.0 / 120.0, 1.0 / 120.0];
    let mut intg = [0f64; 10];
    let sub = |w0: f64, w1: f64, w2: f64| {
        let (t0, t1) = (w0 + w1, w0 * w0);
        let t2 = t1 + w1 * t0;
        let f1 = t0 + w2;
        let f2 = t2 + w2 * f1;
        let f3 = w0 * t1 + w1 * t2 + w2 * f2;
        let g0 = f2 + w0 * (f1 + w0);
        let g1 = f2 + w1 * (f1 + w1);
        let g2 = f2 + w2 * (f1 + w2);
        (f1, f2, f3, g0, g1, g2)
    };
    for t in tris {
        let [p0, p1, p2] = t.map(|i| verts[i as usize].map(|x| x as f64));
        let (a1, b1, c1) = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
        let (a2, b2, c2) = (p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]);
        let (d0, d1, d2) = (b1 * c2 - b2 * c1, a2 * c1 - a1 * c2, a1 * b2 - a2 * b1);
        let (f1x, f2x, f3x, g0x, g1x, g2x) = sub(p0[0], p1[0], p2[0]);
        let (_f1y, f2y, f3y, g0y, g1y, g2y) = sub(p0[1], p1[1], p2[1]);
        let (_f1z, f2z, f3z, g0z, g1z, g2z) = sub(p0[2], p1[2], p2[2]);
        intg[0] += d0 * f1x;
        intg[1] += d0 * f2x; intg[2] += d1 * f2y; intg[3] += d2 * f2z;
        intg[4] += d0 * f3x; intg[5] += d1 * f3y; intg[6] += d2 * f3z;
        intg[7] += d0 * (p0[1] * g0x + p1[1] * g1x + p2[1] * g2x);
        intg[8] += d1 * (p0[2] * g0y + p1[2] * g1y + p2[2] * g2y);
        intg[9] += d2 * (p0[0] * g0z + p1[0] * g1z + p2[0] * g2z);
    }
    for i in 0..10 { intg[i] *= mult[i]; }
    let mass = intg[0];
    let cm = if mass != 0.0 { [intg[1] / mass, intg[2] / mass, intg[3] / mass] } else { [0.0; 3] };
    let ixx = intg[5] + intg[6] - mass * (cm[1] * cm[1] + cm[2] * cm[2]);
    let iyy = intg[4] + intg[6] - mass * (cm[2] * cm[2] + cm[0] * cm[0]);
    let izz = intg[4] + intg[5] - mass * (cm[0] * cm[0] + cm[1] * cm[1]);
    let ixy = -(intg[7] - mass * cm[0] * cm[1]);
    let iyz = -(intg[8] - mass * cm[1] * cm[2]);
    let ixz = -(intg[9] - mass * cm[2] * cm[0]);
    (mass, cm, [ixx, ixy, ixz, ixy, iyy, iyz, ixz, iyz, izz])
}
