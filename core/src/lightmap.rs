//! Baked lighting of one placed mesh (`Lightmaps/<mesh>_{guid}.xlmp`, `eCResourceLightmap_PS`). Every instance of
//! a mesh in the world has its own, made for that mesh's vertices, so a replaced mesh needs new ones.
//!
//! ```text
//! GENOMFLE header (u32 table offset at 10) · head (65 bytes: archive header, u32 size at 15 = table − 33,
//!   ResourcePriority, …, u32 textured at 53, f32, u32 elements at 61)
//! per mesh element (vertex lighting):
//!   01 03 00 01 · u32 n · n × u32 colour (ARGB) · u8 1 · u32 n · n × f32[3] light direction · u16 0
//! u8 0 · string table (the mesh path among the strings)
//! ```
//! Three in four shipped files are this vertex form; the rest ("textured", 1 at 53) carry lightmap pages after an
//! element's arrays instead of `u16 0`, and the same mesh appears in the world with either form. We write the
//! vertex form.

use anyhow::{bail, Result};

pub struct Lightmap {
    head: Vec<u8>,
    table: Vec<u8>,
    /// Per element: colours and directions, one per vertex; elements after a textured one are not read.
    pub elements: Vec<(Vec<u32>, Vec<[f32; 3]>)>,
    /// Elements the file has (some may be unread).
    pub count: usize,
}

const HEAD: usize = 65;
const ELEMENT: [u8; 4] = [1, 3, 0, 1];

pub fn parse(d: &[u8]) -> Result<Lightmap> {
    if !d.starts_with(b"GENOMFLE") || d.len() < 14 + HEAD { bail!("not a GENOMFLE lightmap"); }
    let u = |p: usize| -> Result<usize> { d.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize).ok_or_else(|| anyhow::anyhow!("lightmap runs past the end")) };
    let tab = u(10)?;
    if tab > d.len() { bail!("lightmap table past the end"); }
    let count = u(14 + 61)?;
    let mut p = 14 + HEAD;
    let mut elements = vec![];
    for _ in 0..count {
        if d.get(p..p + 4) != Some(&ELEMENT[..]) { break; }
        let n = u(p + 4)?;
        let a = p + 8 + n * 4;
        if d.get(a) != Some(&1) || u(a + 1)? != n || a + 5 + n * 12 + 2 > tab { break; }
        let cols = (0..n).map(|i| u(p + 8 + i * 4).map(|x| x as u32)).collect::<Result<Vec<_>>>()?;
        let f = |q: usize| f32::from_le_bytes(d[q..q + 4].try_into().unwrap());
        let dirs = (0..n).map(|i| { let q = a + 5 + i * 12; [f(q), f(q + 4), f(q + 8)] }).collect();
        elements.push((cols, dirs));
        p = a + 5 + n * 12;
        let pages = u16::from_le_bytes([d[p], d[p + 1]]);
        p += 2;
        if pages != 0 { break; }
    }
    Ok(Lightmap { head: d[14..14 + HEAD].to_vec(), table: d[tab..].to_vec(), elements, count })
}

/// The vertex form of `lm` with `elements` in place of its own.
pub fn write(lm: &Lightmap, elements: &[(Vec<u32>, Vec<[f32; 3]>)]) -> Vec<u8> {
    let mut o = b"GENOMFLE".to_vec();
    o.extend(1u16.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(&lm.head);
    for (cols, dirs) in elements {
        o.extend(ELEMENT);
        o.extend((cols.len() as u32).to_le_bytes());
        for c in cols { o.extend(c.to_le_bytes()); }
        o.push(1);
        o.extend((dirs.len() as u32).to_le_bytes());
        for d in dirs { for x in d { o.extend(x.to_le_bytes()); } }
        o.extend(0u16.to_le_bytes());
    }
    o.push(0);
    let tab = o.len() as u32;
    o[10..14].copy_from_slice(&tab.to_le_bytes());
    o[14 + 15..14 + 19].copy_from_slice(&(tab - 33).to_le_bytes());
    o[14 + 53..14 + 57].copy_from_slice(&0u32.to_le_bytes());
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
    /// Every vertex-form lightmap the game ships is read whole and written back byte for byte.
    #[test]
    fn shipped_vertex_lightmaps_survive_a_rewrite() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut n, mut textured) = (0, 0);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xlmp")).step_by(7) {
            let d = g.read(&k).unwrap();
            let lm = super::parse(&d).unwrap();
            if u32::from_le_bytes(d[14 + 53..14 + 57].try_into().unwrap()) != 0 || lm.elements.len() != lm.count { textured += 1; continue; }
            assert_eq!(super::write(&lm, &lm.elements), d, "{k}");
            assert!(super::mesh_path(&d).is_some(), "{k}");
            n += 1;
        }
        assert!(n > 1000 && textured > 100, "{n} vertex, {textured} textured");
    }
}
