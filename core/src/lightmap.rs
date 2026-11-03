//! Baked lighting of one placed mesh (`Lightmaps/<mesh>_{guid}.xlmp`, `eCResourceLightmap_PS`). Every instance of
//! a mesh in the world has its own, made for that mesh's vertices, so a replaced mesh needs new ones.
//!
//! ```text
//! GENOMFLE header (u32 table offset at 10) · head (65 bytes: archive header, u32 size at 15 = table − 33,
//!   ResourcePriority, …, u32 textured at 53, f32, u32 elements at 61)
//! per mesh element:
//!   01 03 00 01 · u32 n · n × u32 colour (ARGB) · u8 1 · u32 n · n × f32[3] light direction
//!   · u16 pages · per page: 44 33 22 11 · u16 1 · u32 page number (0, 1, …) · u32 −1 · u32 −1 · u32 w · u32 h
//!                            · u32 bytes · bytes (w·h ARGB texels, or none) · when there are texels: u32 4·h · per row
//!                              u16 first · u16 end of its lit texels (alpha 255; an unlit texel is 0)
//! u8 0 · string table (the mesh path among the strings)
//! ```
//! Every one of the 11138 shipped files reads to its last byte so (see the test). `textured` is the placement's own
//! setting, kept as it is: 1 also in files without pages. Three in four files have no pages (vertex lighting only); the rest light some elements by pages,
//! page k for the element's chart k (see `charts`: which triangles, and the plane their coordinates, stream 73, are in).
//! The same mesh appears in the world with either form.

use anyhow::{bail, Result};

pub struct Lightmap {
    head: Vec<u8>,
    table: Vec<u8>,
    /// Per element: colours and directions, one per vertex.
    pub elements: Vec<(Vec<u32>, Vec<[f32; 3]>)>,
    /// Per element: its pages as stored, from the u16 count on (`[0, 0]`: none).
    pub pages: Vec<Vec<u8>>,
    /// Elements the file has.
    pub count: usize,
}

const HEAD: usize = 65;
const ELEMENT: [u8; 4] = [1, 3, 0, 1];
const PAGE: [u8; 4] = [0x44, 0x33, 0x22, 0x11];
/// No pages.
pub const NONE: [u8; 2] = [0, 0];

impl Lightmap {
    /// Whether element `e` is lit by pages.
    pub fn has_pages(&self, e: usize) -> bool { self.pages.get(e).is_some_and(|p| p[..2] != NONE) }
}

pub fn parse(d: &[u8]) -> Result<Lightmap> {
    if !d.starts_with(b"GENOMFLE") || d.len() < 14 + HEAD { bail!("not a GENOMFLE lightmap"); }
    let u = |p: usize| -> Result<usize> { d.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(|| anyhow::anyhow!("lightmap runs past the end at 0x{p:x}")) };
    let tab = u(10)?;
    if tab > d.len() || tab == 0 { bail!("lightmap table past the end"); }
    let count = u(14 + 61)?;
    let mut p = 14 + HEAD;
    let (mut elements, mut pages) = (vec![], vec![]);
    for e in 0..count {
        if d.get(p..p + 4) != Some(&ELEMENT[..]) { bail!("lightmap element {e} at 0x{p:x}"); }
        let n = u(p + 4)?;
        let a = p + 8 + n * 4;
        if d.get(a) != Some(&1) || u(a + 1)? != n || a + 5 + n * 12 + 2 > tab { bail!("lightmap element {e}: directions at 0x{a:x}"); }
        let cols = (0..n).map(|i| u(p + 8 + i * 4).map(|x| x as u32)).collect::<Result<Vec<_>>>()?;
        let f = |q: usize| f32::from_le_bytes(d[q..q + 4].try_into().unwrap());
        let dirs = (0..n).map(|i| { let q = a + 5 + i * 12; [f(q), f(q + 4), f(q + 8)] }).collect();
        elements.push((cols, dirs));
        p = a + 5 + n * 12;
        let start = p;
        let np = u16::from_le_bytes([d[p], d[p + 1]]) as usize;
        p += 2;
        for pg in 0..np {
            if d.get(p..p + 4) != Some(&PAGE[..]) || u(p + 6)? != pg { bail!("lightmap element {e}: page {pg} at 0x{p:x}"); }
            let bytes = u(p + 26)?;
            p += 30 + bytes;
            if bytes > 0 { p += 4 + u(p)?; }
            if p > tab { bail!("lightmap element {e}: page {pg} runs past the table"); }
        }
        pages.push(d[start..p].to_vec());
    }
    if p + 1 != tab || d[p] != 0 { bail!("lightmap ends at 0x{p:x}, its table at 0x{tab:x}"); }
    Ok(Lightmap { head: d[14..14 + HEAD].to_vec(), table: d[tab..].to_vec(), elements, pages, count })
}

/// `lm` with `elements` in place of its own, each with the pages in `pages` (as stored; [`NONE`] for vertex lighting).
pub fn write(lm: &Lightmap, elements: &[(Vec<u32>, Vec<[f32; 3]>)], pages: &[Vec<u8>]) -> Vec<u8> {
    let mut o = b"GENOMFLE".to_vec();
    o.extend(1u16.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(&lm.head);
    for (i, (cols, dirs)) in elements.iter().enumerate() {
        o.extend(ELEMENT);
        o.extend((cols.len() as u32).to_le_bytes());
        for c in cols { o.extend(c.to_le_bytes()); }
        o.push(1);
        o.extend((dirs.len() as u32).to_le_bytes());
        for d in dirs { for x in d { o.extend(x.to_le_bytes()); } }
        let p = pages.get(i).map(Vec::as_slice).filter(|p| p.len() >= 2).unwrap_or(&NONE);
        o.extend(p);
    }
    o.push(0);
    let tab = o.len() as u32;
    o[10..14].copy_from_slice(&tab.to_le_bytes());
    o[14 + 15..14 + 19].copy_from_slice(&(tab - 33).to_le_bytes());
    o[14 + 61..14 + 65].copy_from_slice(&(elements.len() as u32).to_le_bytes());
    o.extend(&lm.table);
    o
}

/// The mesh path a lightmap is made for (`/Data/_compiledMesh/...xcmsh` among its strings), lower case.
pub fn mesh_path(d: &[u8]) -> Option<String> {
    crate::g3_res::genomfle_strings(d).ok()?.into_iter().find(|s| s.to_lowercase().ends_with(".xcmsh")).map(|s| s.to_lowercase())
}

#[cfg(test)]
mod tests {
    /// Every lightmap the game ships, vertex-lit or with pages, is read whole and written back byte for byte.
    #[test]
    fn shipped_lightmaps_survive_a_rewrite() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut n, mut textured) = (0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xlmp")) {
            let d = g.read(&k).unwrap();
            let lm = super::parse(&d).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert_eq!(lm.elements.len(), lm.count, "{k}");
            assert!(super::write(&lm, &lm.elements, &lm.pages) == d, "{k}: written back differently");
            assert!(super::mesh_path(&d).is_some(), "{k}");
            if (0..lm.count).any(|e| lm.has_pages(e)) { textured += 1; }
            n += 1;
        }
        assert!(n > 11000 && textured > 2000, "{n} lightmaps, {textured} with pages");
    }
}
