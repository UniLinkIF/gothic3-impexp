//! A Gothic 3 actor (`.xact`) read into its fields and written back from them, every part of it: the head, the
//! levels of detail, every FXA section, the colours and tangents after each, the string table.
//!
//! ```text
//! GENOMFLE · u16 1 · u32 table · u16 54 · u32 · u32 priority · FILETIME (compiled) · u32 source bytes · box f32[6]
//! · u32 n · n × (u16, f32[7])                    (4 records in the bare skeletons, none elsewhere)
//! · u32 lods · (1 + lods) × block:              (66 actors carry their _lod1 / _lod2 here as well)
//!     "gena" · u16 4 · u32 FXA bytes · "FXA " u8 1 u8 1 · sections · extra
//!     section  u32 id · u32 size · u32 version · body
//!       16 v1 info      u8 · u8 · u16 · u32 · str exporter · str source file · str date
//!        0 v3 node      f32 pos[3] · rot[4] · scale rotation[4] · scale[3] · f32[3] · str name · str parent
//!        3 v3 mesh      u32 node · vertices · corners · indices · parts · uv sets · u32
//!                       part: u32 (low byte = material) · u32 indices · u32 corners
//!                             · corner: u32 vertex · f32 pos[3] · normal[3] · uv[2 × sets] · u32 index[indices]
//!        4 v1 skin      u32 node · per vertex: u8 n · n × (u16 node, u16, f32 weight)
//!        6 v5 material  f32[16] · u16 · u16 · str name · f32
//!        7 v4 map       u8 kind (1 diffuse, 3 normal, 9 specular) · u8 · u16 material · f32[6] · str texture
//!        8 v1 limit     u32 node · f32 translation min[3] max[3] · rotation min[3] max[3] · scale min[3] max[3] · u8[12]
//!        9 v1 shape     char name[36] (zero-ended) · u32 kind · f32[8]            (a bone's body for physics)
//!     extra    u32 n · n × u16[3] (?, FXA material, string) · u8 1 · u32 meshes
//!              · meshes × (u8 1 · u32 corners · u32 colour[c] · f32 tangent[3·c])      (bare skeletons: no mesh)
//! · DEADBEEF · u8 1 · u32 n · n × (u16 length, bytes)     (the shader materials, `.xshmat`)
//! ```
//! `str` = u32 length + bytes. Some fields hold what the exporter left in memory (the u16 after a skin weight, bytes
//! past the zero in a shape's name): they are kept as read, and a new actor writes zeros there.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Doc { pub version: u16, pub word16: u32, pub priority: u32, pub filetime: u64, pub source: u32, pub bbox: [u32; 6], pub records: Vec<(u16, [u32; 7])>, pub blocks: Vec<Block>, pub table: Vec<Vec<u8>> }

#[derive(Debug, Clone, PartialEq)]
pub struct Block { pub gena: u16, pub fxa_version: [u8; 2], pub sections: Vec<Section>, pub extra: Extra }

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extra { pub materials: Vec<[u16; 3]>, pub flag: u8, pub meshes: Vec<CornerData> }

/// A colour (ARGB) and a tangent per corner of a mesh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CornerData { pub flag: u8, pub colors: Vec<u32>, pub tangents: Vec<[u32; 3]> }

/// Floats are kept as their bits (a written actor is the read one to the byte).
#[derive(Debug, Clone, PartialEq)]
pub struct Node { pub pos: [u32; 3], pub rot: [u32; 4], pub scale_rot: [u32; 4], pub scale: [u32; 3], pub more: [u32; 3], pub name: Vec<u8>, pub parent: Vec<u8> }

#[derive(Debug, Clone, PartialEq)]
pub struct Part { pub head: u32, pub corners: Vec<(u32, Vec<u32>)>, pub indices: Vec<u32> }

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh { pub node: u32, pub vertices: u32, pub uv_sets: u32, pub word: u32, pub parts: Vec<Part> }

#[derive(Debug, Clone, PartialEq)]
pub struct Skin { pub node: u32, pub weights: Vec<Vec<(u16, u16, u32)>> }

#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    Info { version: u32, a: u8, b: u8, c: u16, d: u32, exporter: Vec<u8>, source: Vec<u8>, date: Vec<u8> },
    Node { version: u32, node: Node },
    Mesh { version: u32, mesh: Mesh },
    Skin { version: u32, skin: Skin },
    Material { version: u32, colors: [u32; 16], a: u16, b: u16, name: Vec<u8>, last: u32 },
    Map { version: u32, kind: u8, b: u8, material: u16, values: [u32; 6], texture: Vec<u8> },
    Limit { version: u32, node: u32, values: [u32; 18], flags: [u8; 12] },
    Shape { version: u32, name: [u8; 36], kind: u32, values: [u32; 8] },
}

struct R<'a> { d: &'a [u8], p: usize }
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> { let s = self.d.get(self.p..self.p + n).with_context(|| format!("actor runs short at 0x{:x}", self.p))?; self.p += n; Ok(s) }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn u64(&mut self) -> Result<u64> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    fn n<const N: usize>(&mut self) -> Result<[u32; N]> { let mut v = [0u32; N]; for x in v.iter_mut() { *x = self.u32()?; } Ok(v) }
    fn s(&mut self) -> Result<Vec<u8>> { let n = self.u32()? as usize; if n > 4096 { bail!("string of {n} bytes at 0x{:x}", self.p); } Ok(self.take(n)?.to_vec()) }
}

fn section(r: &mut R, id: u32, version: u32, nv_of: &dyn Fn(u32) -> Option<u32>) -> Result<Section> {
    Ok(match id {
        16 => Section::Info { version, a: r.u8()?, b: r.u8()?, c: r.u16()?, d: r.u32()?, exporter: r.s()?, source: r.s()?, date: r.s()? },
        0 => Section::Node { version, node: Node { pos: r.n()?, rot: r.n()?, scale_rot: r.n()?, scale: r.n()?, more: r.n()?, name: r.s()?, parent: r.s()? } },
        3 => {
            let (node, vertices, corners, indices, parts, uv_sets, word) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?);
            let mut ps = vec![];
            for _ in 0..parts {
                let (head, ni, nc) = (r.u32()?, r.u32()? as usize, r.u32()? as usize);
                let mut cs = Vec::with_capacity(nc);
                for _ in 0..nc { let v = r.u32()?; cs.push((v, (0..8 + 2 * uv_sets as usize - 2).map(|_| r.u32()).collect::<Result<Vec<_>>>()?)); }
                let idx = (0..ni).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
                ps.push(Part { head, corners: cs, indices: idx });
            }
            let m = Mesh { node, vertices, uv_sets, word, parts: ps };
            if m.parts.iter().map(|p| p.corners.len()).sum::<usize>() != corners as usize || m.parts.iter().map(|p| p.indices.len()).sum::<usize>() != indices as usize { bail!("mesh counts differ from its parts"); }
            Section::Mesh { version, mesh: m }
        }
        4 => {
            let node = r.u32()?;
            let nv = nv_of(node).context("skin before its mesh")?;
            let mut ws = Vec::with_capacity(nv as usize);
            for _ in 0..nv { let n = r.u8()?; ws.push((0..n).map(|_| Ok((r.u16()?, r.u16()?, r.u32()?))).collect::<Result<Vec<_>>>()?); }
            Section::Skin { version, skin: Skin { node, weights: ws } }
        }
        6 => Section::Material { version, colors: r.n()?, a: r.u16()?, b: r.u16()?, name: r.s()?, last: r.u32()? },
        7 => Section::Map { version, kind: r.u8()?, b: r.u8()?, material: r.u16()?, values: r.n()?, texture: r.s()? },
        8 => Section::Limit { version, node: r.u32()?, values: r.n()?, flags: r.take(12)?.try_into().unwrap() },
        9 => Section::Shape { version, name: r.take(36)?.try_into().unwrap(), kind: r.u32()?, values: r.n()? },
        id => bail!("section {id}"),
    })
}

/// Every part of an actor file.
pub fn read(d: &[u8]) -> Result<Doc> {
    if !d.starts_with(b"GENOMFLE") || d.get(8..10) != Some(&[1, 0][..]) { bail!("not a GENOMFLE actor"); }
    let mut r = R { d, p: 10 };
    let table = r.u32()? as usize;
    let (version, word16, priority, filetime, source) = (r.u16()?, r.u32()?, r.u32()?, r.u64()?, r.u32()?);
    let bbox = r.n()?;
    let records = (0..r.u32()?).map(|_| Ok((r.u16()?, r.n()?))).collect::<Result<Vec<_>>>()?;
    let lods = r.u32()?;
    let mut blocks = vec![];
    for _ in 0..=lods {
        if r.take(4)? != b"gena" { bail!("no gena block at 0x{:x}", r.p - 4); }
        let gena = r.u16()?;
        let len = r.u32()? as usize;
        let end = r.p + len;
        if r.take(4)? != b"FXA " { bail!("no FXA at 0x{:x}", r.p - 4); }
        let fxa_version = [r.u8()?, r.u8()?];
        let mut sections: Vec<Section> = vec![];
        while r.p < end {
            let (id, size, ver) = (r.u32()?, r.u32()? as usize, r.u32()?);
            let at = r.p;
            let nv = |node: u32| sections.iter().rev().find_map(|s| match s { Section::Mesh { mesh, .. } if mesh.node == node => Some(mesh.vertices), _ => None });
            let s = section(&mut r, id, ver, &nv).with_context(|| format!("section {id} at 0x{:x}", at - 12))?;
            if r.p != at + size { bail!("section {id} at 0x{:x}: {} bytes read of {size}", at - 12, r.p - at); }
            sections.push(s);
        }
        if r.p != end { bail!("sections run past the FXA"); }
        let materials = (0..r.u32()?).map(|_| Ok([r.u16()?, r.u16()?, r.u16()?])).collect::<Result<Vec<_>>>()?;
        let flag = r.u8()?;
        let mut meshes = vec![];
        for _ in 0..r.u32()? {
            let f = r.u8()?;
            let n = r.u32()? as usize;
            let colors = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
            let tangents = (0..n).map(|_| r.n()).collect::<Result<Vec<_>>>()?;
            meshes.push(CornerData { flag: f, colors, tangents });
        }
        blocks.push(Block { gena, fxa_version, sections, extra: Extra { materials, flag, meshes } });
    }
    if r.p != table { bail!("{} bytes between the last block and the string table", table as i64 - r.p as i64); }
    if r.u32()? != 0xDEADBEEF || r.u8()? != 1 { bail!("no string table"); }
    let n = r.u32()?;
    let mut strings = vec![];
    for _ in 0..n { let l = r.u16()? as usize; strings.push(r.take(l)?.to_vec()); }
    if r.p != d.len() { bail!("{} bytes after the string table", d.len() - r.p); }
    Ok(Doc { version, word16, priority, filetime, source, bbox, records, blocks, table: strings })
}

fn put(o: &mut Vec<u8>, x: u32) { o.extend(x.to_le_bytes()); }
fn put_s(o: &mut Vec<u8>, s: &[u8]) { put(o, s.len() as u32); o.extend(s); }

fn body(s: &Section) -> (u32, u32, Vec<u8>) {
    let mut o = vec![];
    let (id, version) = match s {
        Section::Info { version, a, b, c, d, exporter, source, date } => { o.push(*a); o.push(*b); o.extend(c.to_le_bytes()); put(&mut o, *d); put_s(&mut o, exporter); put_s(&mut o, source); put_s(&mut o, date); (16, *version) }
        Section::Node { version, node: n } => { for x in n.pos.iter().chain(&n.rot).chain(&n.scale_rot).chain(&n.scale).chain(&n.more) { put(&mut o, *x); } put_s(&mut o, &n.name); put_s(&mut o, &n.parent); (0, *version) }
        Section::Mesh { version, mesh: m } => {
            let corners: usize = m.parts.iter().map(|p| p.corners.len()).sum();
            let indices: usize = m.parts.iter().map(|p| p.indices.len()).sum();
            for x in [m.node, m.vertices, corners as u32, indices as u32, m.parts.len() as u32, m.uv_sets, m.word] { put(&mut o, x); }
            for p in &m.parts {
                for x in [p.head, p.indices.len() as u32, p.corners.len() as u32] { put(&mut o, x); }
                for (v, f) in &p.corners { put(&mut o, *v); for x in f { put(&mut o, *x); } }
                for i in &p.indices { put(&mut o, *i); }
            }
            (3, *version)
        }
        Section::Skin { version, skin } => {
            put(&mut o, skin.node);
            for w in &skin.weights { o.push(w.len() as u8); for (n, c, x) in w { o.extend(n.to_le_bytes()); o.extend(c.to_le_bytes()); put(&mut o, *x); } }
            (4, *version)
        }
        Section::Material { version, colors, a, b, name, last } => { for x in colors { put(&mut o, *x); } o.extend(a.to_le_bytes()); o.extend(b.to_le_bytes()); put_s(&mut o, name); put(&mut o, *last); (6, *version) }
        Section::Map { version, kind, b, material, values, texture } => { o.push(*kind); o.push(*b); o.extend(material.to_le_bytes()); for x in values { put(&mut o, *x); } put_s(&mut o, texture); (7, *version) }
        Section::Limit { version, node, values, flags } => { put(&mut o, *node); for x in values { put(&mut o, *x); } o.extend(flags); (8, *version) }
        Section::Shape { version, name, kind, values } => { o.extend(name); put(&mut o, *kind); for x in values { put(&mut o, *x); } (9, *version) }
    };
    (id, version, o)
}

/// The actor file of `doc`.
pub fn write(doc: &Doc) -> Vec<u8> {
    let mut o = b"GENOMFLE".to_vec();
    o.extend(1u16.to_le_bytes());
    put(&mut o, 0);
    o.extend(doc.version.to_le_bytes());
    for x in [doc.word16, doc.priority] { put(&mut o, x); }
    o.extend(doc.filetime.to_le_bytes());
    put(&mut o, doc.source);
    for x in doc.bbox { put(&mut o, x); }
    put(&mut o, doc.records.len() as u32);
    for (i, v) in &doc.records { o.extend(i.to_le_bytes()); for x in v { put(&mut o, *x); } }
    put(&mut o, doc.blocks.len().saturating_sub(1) as u32);
    for b in &doc.blocks {
        let mut fxa = b"FXA ".to_vec();
        fxa.extend(b.fxa_version);
        for s in &b.sections { let (id, ver, body) = body(s); put(&mut fxa, id); put(&mut fxa, body.len() as u32); put(&mut fxa, ver); fxa.extend(body); }
        o.extend(b"gena");
        o.extend(b.gena.to_le_bytes());
        put(&mut o, fxa.len() as u32);
        o.extend(fxa);
        let e = &b.extra;
        put(&mut o, e.materials.len() as u32);
        for m in &e.materials { for x in m { o.extend(x.to_le_bytes()); } }
        o.push(e.flag);
        put(&mut o, e.meshes.len() as u32);
        for m in &e.meshes {
            o.push(m.flag);
            put(&mut o, m.colors.len() as u32);
            for c in &m.colors { put(&mut o, *c); }
            for t in &m.tangents { for x in t { put(&mut o, *x); } }
        }
    }
    let table = o.len() as u32;
    o[10..14].copy_from_slice(&table.to_le_bytes());
    put(&mut o, 0xDEADBEEF);
    o.push(1);
    put(&mut o, doc.table.len() as u32);
    for s in &doc.table { o.extend((s.len() as u16).to_le_bytes()); o.extend(s); }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every actor of the game, read into its fields and written from them, is the same file to the byte.
    #[test]
    fn every_actor_of_the_game_writes_back_to_the_byte() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (mut n, mut lods, mut limits, mut shapes) = (0, 0, 0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xact")) {
            let d = g.read(&k).unwrap();
            let doc = read(&d).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            let w = write(&doc);
            if w != d { let at = w.iter().zip(&d).position(|(a, b)| a != b).unwrap_or(w.len().min(d.len())); panic!("{k}: differs at 0x{at:x} ({} vs {} bytes)", w.len(), d.len()); }
            n += 1;
            lods += doc.blocks.len() - 1;
            for s in doc.blocks.iter().flat_map(|b| &b.sections) { match s { Section::Limit { .. } => limits += 1, Section::Shape { .. } => shapes += 1, _ => {} } }
        }
        eprintln!("{n} actors to the byte; {lods} levels of detail inside, {limits} limits, {shapes} shapes");
        assert_eq!(n, 379);
    }
}
