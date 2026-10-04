//! Gothic 3 resources: static mesh `.xcmsh`, material `.xshmat`, image `.ximg`, collision `.xnvmsh`.
//!
//! Gothic 3 writes them as plain Genome archives (`u16 1, u8 1, u16 1, u8 1, <class name>, ...`, strings = u16
//! length + bytes) or, for materials and images, as `GENOMFLE` (string table at the end). The layouts were checked
//! on every file of the install.
use anyhow::{anyhow, bail, Context, Result};

/// Reader over a plain archive (strings inline) or a `GENOMFLE` one (strings = u16 index into the table at the end;
/// the archive itself starts at byte 14).
struct Rd<'a> { d: &'a [u8], p: usize, table: Option<Vec<String>> }
impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> { let s = self.d.get(self.p..self.p + n).ok_or_else(|| anyhow!("runs past the end at 0x{:x}", self.p))?; self.p += n; Ok(s) }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn f32(&mut self) -> Result<f32> { Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn skip(&mut self, n: usize) -> Result<()> { self.take(n).map(|_| ()) }
    fn str(&mut self) -> Result<String> {
        if self.table.is_some() { let i = self.u16()? as usize; return self.table.as_ref().unwrap().get(i).cloned().ok_or_else(|| anyhow!("string index {i} out of the table")); }
        let n = self.u16()? as usize; Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn open(d: &'a [u8]) -> Result<Self> { if d.starts_with(b"GENOMFLE") { Ok(Rd { d, p: 14, table: Some(genomfle_strings(d)?) }) } else { Ok(Rd { d, p: 0, table: None }) } }
    fn v3(&mut self) -> Result<[f32; 3]> { Ok([self.f32()?, self.f32()?, self.f32()?]) }
}

/// The common archive head of a plain Gothic 3 resource, up to the class data (property section skipped).
fn resource_head(r: &mut Rd, class: &str) -> Result<()> {
    if r.u16()? != 1 || r.u8()? != 1 || r.u16()? != 1 || r.u8()? != 1 { bail!("not a Gothic 3 resource archive"); }
    let c = r.str()?;
    if c != class { bail!("class {c}, expected {class}"); }
    if r.u8()? != 1 || r.u16()? != 0 { bail!("{class}: unexpected archive flags"); }
    let ver = r.u16()?;
    r.skip(6)?;
    if ver < 81 { r.str()?; }
    if ver < 82 { r.skip(20)?; }
    r.u16()?;
    for _ in 0..r.u32()? { r.str()?; r.str()?; r.u16()?; let n = r.u32()? as usize; r.skip(n)?; }
    Ok(())
}

/// A decoded `.xcmsh` (`eCResourceMeshComplex_PS`): one submesh per mesh element, the vertices of all elements
/// concatenated. Material names keep their `.xshmat` extension. Vertex colours (streams 4 and 5) per vertex, white /
/// opaque black where an element has none. `elements`: (first vertex, vertices) of each element.
pub struct G3Mesh { pub geo: crate::geom::MeshGeometry, pub streams: Vec<u32>, pub colors: bool, pub diffuse: Vec<u32>, pub specular: Vec<u32>, pub elements: Vec<(u32, u32)> }

pub fn decode_xcmsh(bytes: &[u8]) -> Result<G3Mesh> {
    let mut r = Rd::open(bytes)?;
    resource_head(&mut r, "eCResourceMeshComplex_PS")?;
    let ver = r.u16()?;
    let ps = r.u16()?;
    if ps > 22 { r.u32()?; }
    if ps < 30 && r.u16()? > 1 { r.u8()?; }
    r.f32()?;
    let elements = r.u32()?;
    if ver < 34 { bail!("mesh class version {ver} (elements by name only)"); }
    let mut g = crate::geom::MeshGeometry { positions: vec![], normals: vec![], uvs: vec![], indices: vec![], submeshes: vec![] };
    let mut streams = vec![];
    let mut colors = false;
    let (mut diffuse, mut specular) = (vec![], vec![]);
    let mut ranges = vec![];
    for _ in 0..elements {
        let ever = r.u16()?;
        r.u32()?;
        r.skip(28)?;
        let material = r.str()?;
        let base = g.positions.len() as u32;
        let first_index = g.indices.len() as u32;
        let (mut pos, mut nrm, mut uv, mut idx, mut dif, mut spe) = (vec![], vec![], vec![], vec![], vec![], vec![]);
        for _ in 0..r.u32()? {
            let ty = r.u32()?;
            r.skip(3)?;
            let n = r.u32()? as usize;
            if !streams.contains(&ty) { streams.push(ty); }
            match ty {
                0 => for _ in 0..n { idx.push(r.u32()?) },
                1 => for _ in 0..n { pos.push(r.v3()?) },
                3 => for _ in 0..n { nrm.push(r.v3()?) },
                12 => for _ in 0..n { uv.push([r.f32()?, r.f32()?]) },
                4 => { colors = true; for _ in 0..n { dif.push(r.u32()?) } }
                5 => { colors = true; for _ in 0..n { spe.push(r.u32()?) } }
                6 => r.skip(n * 4)?,
                15 | 18 | 21 | 73 => r.skip(n * 8)?,
                64 | 72 => r.skip(n * 12)?,
                2 => r.skip(n * 16)?,
                t => bail!("unknown vertex stream type {t}"),
            }
        }
        if ever > 2 { for _ in 0..2 { r.u8()?; let n = r.u32()? as usize; r.skip(n * 4)?; } }
        if ever > 1 { r.u8()?; for _ in 0..r.u32()? { for _ in 0..2 { r.u8()?; let n = r.u32()? as usize; r.skip(n * 4)?; } r.skip(84)?; } }
        if ever > 3 { let n = r.u32()? as usize; r.skip(n * 24)?; r.u8()?; let n = r.u32()? as usize; r.skip(n * 4)?; }
        let nv = pos.len();
        if (!nrm.is_empty() && nrm.len() != nv) || (!uv.is_empty() && uv.len() != nv) { bail!("{material}: stream lengths differ ({nv} positions, {} normals, {} uvs)", nrm.len(), uv.len()); }
        if idx.len() % 3 != 0 { bail!("{material}: {} indices", idx.len()); }
        if let Some(b) = idx.iter().find(|&&i| i as usize >= nv) { bail!("{material}: index {b} >= {nv}"); }
        // The merged buffers stay parallel: an element without normals/uvs gets defaults, and so do the
        // elements before the first one that has them.
        let before = g.positions.len();
        if !nrm.is_empty() || !g.normals.is_empty() { g.normals.resize(before, [0.0, 1.0, 0.0]); if nrm.is_empty() { nrm.resize(nv, [0.0, 1.0, 0.0]); } }
        if !uv.is_empty() || !g.uvs.is_empty() { g.uvs.resize(before, [0.0, 0.0]); if uv.is_empty() { uv.resize(nv, [0.0, 0.0]); } }
        dif.resize(nv, 0xffff_ffff);
        spe.resize(nv, 0xff00_0000);
        ranges.push((before as u32, nv as u32));
        diffuse.extend(dif);
        specular.extend(spe);
        g.positions.extend(pos);
        g.normals.extend(nrm);
        g.uvs.extend(uv);
        g.indices.extend(idx.iter().map(|i| i + base));
        g.submeshes.push(crate::geom::SubRange { material, first_index, index_count: g.indices.len() as u32 - first_index });
    }
    Ok(G3Mesh { geo: g, streams, colors, diffuse, specular, elements: ranges })
}

/// Strings of a `GENOMFLE` file's string table (u32 table offset at 10; at the table: u32 magic, u8, u32 count,
/// then u16 length + bytes each).
pub fn genomfle_strings(d: &[u8]) -> Result<Vec<String>> {
    if d.get(..8) != Some(b"GENOMFLE") { bail!("not a GENOMFLE file"); }
    let at = u32::from_le_bytes(d.get(10..14).context("table offset")?.try_into().unwrap()) as usize;
    let mut r = Rd { d, p: at, table: None };
    r.u32()?;
    r.u8()?;
    let n = r.u32()?;
    if n > 100_000 { bail!("string table count {n}"); }
    let mut v = Vec::with_capacity(n as usize);
    for _ in 0..n { v.push(r.str()?); }
    Ok(v)
}

/// Every u16-length-prefixed printable string of a plain archive (names, types, values), in file order.
pub fn inline_strings(d: &[u8]) -> Vec<String> {
    let mut v = vec![];
    let mut i = 0;
    while i + 2 < d.len() {
        let n = u16::from_le_bytes([d[i], d[i + 1]]) as usize;
        if (3..=260).contains(&n) && i + 2 + n <= d.len() && d[i + 2..i + 2 + n].iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
            v.push(String::from_utf8_lossy(&d[i + 2..i + 2 + n]).into_owned());
            i += 2 + n;
        } else { i += 1; }
    }
    v
}

/// A Gothic 3 material (`.xshmat`, `eCResourceShaderMaterial_PS`): the shader class and its textures (`.tga`
/// names in the string table; the compiled image is `<stem>.ximg` in `_compiledImage`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct G3Material { pub shader: Option<String>, pub textures: Vec<String>, pub blend: Option<u32> }

pub fn parse_xshmat(bytes: &[u8]) -> Result<G3Material> {
    let s = if bytes.starts_with(b"GENOMFLE") { genomfle_strings(bytes)? } else { inline_strings(bytes) };
    if !s.iter().any(|x| x == "eCResourceShaderMaterial_PS") { bail!("not a shader material"); }
    let shader = s.iter().find(|x| x.starts_with("eCShader")).cloned();
    let textures = s.into_iter().filter(|x| { let l = x.to_lowercase(); l.ends_with(".tga") || l.ends_with(".dds") || l.ends_with(".ximg") }).collect();
    Ok(G3Material { shader, textures, blend: enum_property(bytes, "BlendMode")? })
}

/// `eEShaderMaterialBlendMode`: 0 normal (opaque), 1 masked (alpha test), 2 alpha blend, 3 modulate,
/// 4 alpha modulate, 5 translucent, 6 darken, 7 brighten, 8 invisible.
pub const BLEND_NORMAL: u32 = 0;
pub const BLEND_MASKED: u32 = 1;

/// Value of the first enum property `name` (`[name][type][u16 30][u32 6][u16 version][u32 value]`, names inline or
/// as table indices). None when the material does not write it.
pub fn enum_property(bytes: &[u8], name: &str) -> Result<Option<u32>> {
    let base = Rd::open(bytes)?;
    for p in base.p..bytes.len().saturating_sub(16) {
        let mut r = Rd { d: bytes, p, table: base.table.clone() };
        if r.str().ok().as_deref() != Some(name) { continue; }
        let Ok(ty) = r.str() else { continue };
        if !ty.contains("enum") || r.u16()? != 30 || r.u32()? != 6 { continue; }
        r.u16()?;
        return Ok(Some(r.u32()?));
    }
    Ok(None)
}

/// A Gothic 3 image (`GENOMFLE` + `G3IMG`): width, height, mip count and a DXT FourCC, then the mip chain smallest
/// first, the full-size level last, ending where the string table starts. Rebuilt as a plain DDS of the full-size
/// level so the DDS decoder reads it.
pub fn ximg_dds(d: &[u8]) -> Result<Vec<u8>> {
    let g = d.windows(5).take(256).position(|w| w == b"G3IMG").ok_or_else(|| anyhow!("no G3IMG header"))?;
    let mut r = Rd { d, p: g + 5, table: None };
    r.skip(4)?;
    let (w, h) = (r.u32()?, r.u32()?);
    r.skip(8)?;
    let _mips = r.u32()?;
    r.skip(4)?;
    let four: [u8; 4] = r.take(4)?.try_into().unwrap();
    let block = match &four { b"DXT1" => 8, b"DXT3" | b"DXT5" => 16, f => bail!("pixel format {:?}", String::from_utf8_lossy(f)) };
    let top = (w.max(4) / 4) as usize * (h.max(4) / 4) as usize * block;
    let end = if d.starts_with(b"GENOMFLE") { u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize } else { d.len() };
    if end > d.len() || end < r.p + top { bail!("{w}x{h} {} does not fit ({} bytes)", String::from_utf8_lossy(&four), d.len()); }
    let px = &d[end - top..end];
    let mut dds = Vec::with_capacity(128 + top);
    dds.extend(b"DDS ");
    let u = |v: &mut Vec<u8>, x: u32| v.extend(x.to_le_bytes());
    u(&mut dds, 124); u(&mut dds, 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000); u(&mut dds, h); u(&mut dds, w); u(&mut dds, top as u32); u(&mut dds, 0); u(&mut dds, 1);
    for _ in 0..11 { u(&mut dds, 0); }
    u(&mut dds, 32); u(&mut dds, 4); dds.extend(four); for _ in 0..5 { u(&mut dds, 0); }
    u(&mut dds, 0x1000); for _ in 0..4 { u(&mut dds, 0); }
    dds.extend(px);
    Ok(dds)
}

/// Triangles (cm) of every PhysX cooked triangle mesh (`NXS\x01MESH`) in a `.xnvmsh`. The same stream as Risen 1/2
/// `._xcom` except the version field: 0 here (PhysX 2.5), 1 there.
pub fn xnvmsh_triangles(d: &[u8]) -> Result<Vec<[[f32; 3]; 3]>> {
    let mut out = vec![];
    let mut i = 0;
    let u = |at: usize| -> Result<u32> { d.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| anyhow!("NXS stream runs past the end")) };
    let f = |at: usize| f32::from_le_bytes(d[at..at + 4].try_into().unwrap());
    while let Some(p) = d.get(i..).and_then(|t| t.windows(8).position(|w| w == b"NXS\x01MESH")).map(|p| p + i) {
        if u(p + 8)? > 1 { bail!("NXS version {} at 0x{p:x}", u(p + 8)?); }
        let flags = u(p + 12)?;
        if u(p + 20)? != 0xff { bail!("NXS height field at 0x{p:x}"); }
        let (nv, nt) = (u(p + 28)? as usize, u(p + 32)? as usize);
        let vs = p + 36;
        let w = if flags & 8 != 0 { 1 } else if flags & 16 != 0 { 2 } else { 4 };
        let is = vs + nv * 12;
        if d.len() < is + nt * 3 * w { bail!("NXS stream at 0x{p:x} runs past the end"); }
        let v = |k: usize| -> [f32; 3] { let a = vs + k * 12; [f(a) * 100.0, f(a + 4) * 100.0, f(a + 8) * 100.0] };
        let idx = |k: usize| -> usize { let a = is + k * w; match w { 1 => d[a] as usize, 2 => u16::from_le_bytes([d[a], d[a + 1]]) as usize, _ => u32::from_le_bytes(d[a..a + 4].try_into().unwrap()) as usize } };
        for t in 0..nt {
            let tri = [idx(t * 3), idx(t * 3 + 1), idx(t * 3 + 2)];
            if tri.iter().any(|&k| k >= nv) { bail!("NXS index out of range at 0x{p:x}"); }
            out.push(tri.map(v));
        }
        i = is + nt * 3 * w;
    }
    Ok(out)
}

/// A `.xnvmsh` as the game writes it: cooked PhysX streams with their boxes (cm), the box of all (mostly empty:
/// FLT_MAX, -FLT_MAX), a convex flag, and the shape table — 4 bytes each (material, ignored by trace ray, no
/// collision, no response). The landscape keeps one stream per material and one shape per stream; objects may
/// list fewer or none.
pub struct Xnv { pub streams: Vec<(Vec<u8>, ([f32; 3], [f32; 3]))>, pub all: ([f32; 3], [f32; 3]), pub convex: u8, pub shapes: Vec<[u8; 4]> }

/// The parts of a `.xnvmsh`; old plain files without the tail give the streams alone.
pub fn parse_xnvmsh(d: &[u8]) -> Result<Xnv> {
    let p0 = d.windows(4).position(|w| w == b"NXS\x01").ok_or_else(|| anyhow!("no PhysX stream"))?;
    if p0 < 17 { bail!("PhysX stream at 0x{p0:x}"); }
    let get = |at: usize, n: usize| d.get(at..at + n).ok_or_else(|| anyhow!("collision runs past the end"));
    let u32_ = |at: usize| -> Result<u32> { Ok(u32::from_le_bytes(get(at, 4)?.try_into().unwrap())) };
    let f = |at: usize| -> Result<f32> { Ok(f32::from_le_bytes(get(at, 4)?.try_into().unwrap())) };
    let bx = |at: usize| -> Result<([f32; 3], [f32; 3])> { Ok(([f(at)?, f(at + 4)?, f(at + 8)?], [f(at + 12)?, f(at + 16)?, f(at + 20)?])) };
    let n = u32_(p0 - 12)? as usize;
    let mut p = p0 - 8;
    let mut x = Xnv { streams: vec![], all: ([0.0; 3], [0.0; 3]), convex: d[p0 - 17], shapes: vec![] };
    for _ in 0..n {
        let len = u64::from_le_bytes(get(p, 8)?.try_into().unwrap()) as usize;
        x.streams.push((get(p + 8, len)?.to_vec(), ([0.0; 3], [0.0; 3])));
        p += 8 + len;
    }
    if d.get(p..p + 2) != Some(&30u16.to_le_bytes()[..]) { return Ok(x); }
    x.all = bx(p + 6)?;
    for i in 0..n { x.streams[i].1 = bx(p + 30 + 24 * i)?; }
    let q = p + 30 + 24 * n;
    x.convex = *d.get(q).ok_or_else(|| anyhow!("collision runs past the end"))?;
    for i in 0..u32_(q + 1)? as usize { x.shapes.push(get(q + 5 + 4 * i, 4)?.try_into().unwrap()); }
    Ok(x)
}
