//! Writing Gothic 3 images (`.ximg`) and materials (`.xshmat`).
//!
//! Image, as the game's (`GENOMFLE` + `G3IMG`, measured on the install):
//! ```text
//! 0   "GENOMFLE" · u16 1 · u32 string-table offset · u32 0x20 · u32 pixel sum · u32 0 · FILETIME · u32 source size
//!     · u16 1
//! 40  "G3IMG" · u16 2 · u16 47 · u32 width · u32 height · u64 0 · u32 mip levels · u32 0 · FourCC · u32 1
//!     · u32 pixel sum (Σ w·h over the levels, halved for DXT1) · u16 0
//! 87  mip levels, smallest first, full size last (DXT blocks; a level under 4×4 still takes one block)
//!     string table: u32 0xDEADBEEF · u8 1 · u32 0 (no strings)
//! ```
//! Material: a shipped `GENOMFLE` material is the template; its texture names (`*.tga` in the string table) become
//! ours, and an alpha-tested one gets our cut-off in `MaskReference`.

use anyhow::{bail, Context, Result};
use texpresso::{Format, Params};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dxt { Dxt1, Dxt5 }

fn pow2_at_most(v: u32, cap: u32) -> u32 { let mut p = 4; while p * 2 <= v && p * 2 <= cap { p *= 2; } p }

/// RGBA (`w`×`h`) as a Gothic 3 image: resized to powers of two (at most 2048), with a full mip chain.
pub fn ximg(rgba: &[u8], w: u32, h: u32, kind: Dxt, filetime: u64) -> Result<Vec<u8>> {
    if rgba.len() != (w * h * 4) as usize { bail!("{w}x{h} image with {} bytes", rgba.len()); }
    let (nw, nh) = (pow2_at_most(w, 2048), pow2_at_most(h, 2048));
    let base = if (nw, nh) == (w, h) { rgba.to_vec() } else {
        let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).context("rgba")?;
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle).into_raw()
    };
    let levels = crate::dds::build_mip_chain(&base, nw, nh);
    let fmt = match kind { Dxt::Dxt1 => Format::Bc1, Dxt::Dxt5 => Format::Bc3 };
    let mut data = vec![];
    for (lw, lh, px) in levels.iter().rev() {
        let mut out = vec![0u8; fmt.compressed_size(*lw as usize, *lh as usize)];
        fmt.compress(px, *lw as usize, *lh as usize, Params::default(), &mut out);
        data.extend(out);
    }
    let sum: u64 = levels.iter().map(|(a, b, _)| (*a as u64) * (*b as u64)).sum();
    let sum = if kind == Dxt::Dxt1 { sum / 2 } else { sum } as u32;
    let mut o = Vec::with_capacity(87 + data.len() + 9);
    o.extend(b"GENOMFLE");
    o.extend(1u16.to_le_bytes());
    let table = 87 + data.len() as u32;
    o.extend(table.to_le_bytes());
    o.extend(0x20u32.to_le_bytes());
    o.extend(sum.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(filetime.to_le_bytes());
    o.extend((nw * nh * 4 + 18).to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(b"G3IMG");
    o.extend(2u16.to_le_bytes());
    o.extend(47u16.to_le_bytes());
    o.extend(nw.to_le_bytes());
    o.extend(nh.to_le_bytes());
    o.extend(0u64.to_le_bytes());
    o.extend((levels.len() as u32).to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(match kind { Dxt::Dxt1 => b"DXT1", Dxt::Dxt5 => b"DXT5" });
    o.extend(1u32.to_le_bytes());
    o.extend(sum.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    debug_assert_eq!(o.len(), 87);
    o.extend(data);
    o.extend(0xDEADBEEFu32.to_le_bytes());
    o.push(1);
    o.extend(0u32.to_le_bytes());
    Ok(o)
}

/// A Blender normal map (RGB, y up) as Gothic 3 keeps them (RGB, y down).
pub fn normal_to_game(rgba: &[u8]) -> Vec<u8> { rgba.chunks_exact(4).flat_map(|p| [p[0], 255 - p[1], p[2], 255]).collect() }

/// A material template: the shipped material and the role of each of its texture strings, in table order.
pub struct Template { pub material: &'static str, pub roles: &'static [&'static str], pub masked: bool }

/// Opaque, diffuse + normal (wood).
pub const OPAQUE: Template = Template { material: "G3_Objects_Wood_01_B", roles: &["diffuse", "normal"], masked: false };
/// Opaque with a specular map (the metal barrel).
pub const OPAQUE_SPECULAR: Template = Template { material: "G3_Objects_Barrelmetal_01_A", roles: &["diffuse", "normal", "specular"], masked: false };
/// Alpha test (a straw roof; BlendMode 1, the cut-off in MaskReference).
pub const MASKED: Template = Template { material: "G3_Architecture_Strawroof_02_A", roles: &["diffuse", "normal"], masked: true };

fn table(d: &[u8]) -> Result<(usize, Vec<String>)> {
    let at = u32::from_le_bytes(d.get(10..14).context("table offset")?.try_into().unwrap()) as usize;
    Ok((at, crate::g3_res::genomfle_strings(d)?))
}

/// `template` with its textures replaced by `textures` (role -> image name without extension) and, when masked,
/// `MaskReference` = `cutoff`. A role the template has but `textures` lacks keeps the template's own image.
pub fn material(template_bytes: &[u8], t: &Template, textures: &[(&str, String)], cutoff: Option<u8>) -> Result<Vec<u8>> {
    if !template_bytes.starts_with(b"GENOMFLE") { bail!("template {} is not a GENOMFLE material", t.material); }
    let (at, mut strings) = table(template_bytes)?;
    let tex_slots: Vec<usize> = strings.iter().enumerate().filter(|(_, s)| { let l = s.to_lowercase(); l.ends_with(".tga") || l.ends_with(".dds") }).map(|(i, _)| i).collect();
    if tex_slots.len() != t.roles.len() { bail!("template {}: {} texture strings, expected {}", t.material, tex_slots.len(), t.roles.len()); }
    for (slot, role) in tex_slots.iter().zip(t.roles) {
        if let Some((_, name)) = textures.iter().find(|(r, _)| r == role) { strings[*slot] = format!("{name}.tga"); }
    }
    let mut out = template_bytes[..at].to_vec();
    if let (true, Some(cut)) = (t.masked, cutoff) {
        let (mi, ci) = (strings.iter().position(|s| s == "MaskReference").context("no MaskReference")?, strings.iter().position(|s| s == "char").context("no char type")?);
        let pat = [(mi as u16).to_le_bytes(), (ci as u16).to_le_bytes(), 30u16.to_le_bytes()].concat();
        let p = out.windows(pat.len()).position(|w| w == pat.as_slice()).context("MaskReference value not found")? + pat.len();
        // u32 size (1) then the char.
        if u32::from_le_bytes(out[p..p + 4].try_into().unwrap()) != 1 { bail!("MaskReference size"); }
        out[p + 4] = cut;
    }
    out.extend(0xDEADBEEFu32.to_le_bytes());
    out.push(1);
    out.extend((strings.len() as u32).to_le_bytes());
    for s in &strings { out.extend((s.len() as u16).to_le_bytes()); out.extend(s.as_bytes()); }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_images_decode_to_their_pixels() {
        let (w, h) = (64u32, 32u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % w * 4) as u8, (i / w * 8) as u8, 128, 255]).collect();
        for kind in [Dxt::Dxt1, Dxt::Dxt5] {
            let x = ximg(&px, w, h, kind, 0).unwrap();
            let (dw, dh, back) = crate::textures::decode(&x).unwrap();
            assert_eq!((dw, dh), (w, h));
            let err = back.iter().zip(&px).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
            assert!(err < 24, "{kind:?}: max error {err}");
        }
    }

    #[test]
    fn templates_take_our_textures() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        for t in [&OPAQUE, &OPAQUE_SPECULAR, &MASKED] {
            let k = g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).unwrap();
            let d = g.read(&k).unwrap();
            let tex: Vec<(&str, String)> = t.roles.iter().map(|r| (*r, format!("My_Crate_{r}"))).collect();
            let m = material(&d, t, &tex, Some(77)).unwrap();
            let parsed = crate::g3_res::parse_xshmat(&m).unwrap();
            let want: Vec<String> = t.roles.iter().map(|r| format!("My_Crate_{r}.tga")).collect();
            assert_eq!(parsed.textures, want, "{}", t.material);
            assert_eq!(parsed.blend, Some(if t.masked { 1 } else { 0 }));
            // Same bytes before the table: only names and the cut-off change.
            assert_eq!(&m[..14], &d[..14]);
        }
    }
}

/// Shape materials (`eEShapeMaterial`), the surface sounds and effects of a collision mesh, by value. 16–19 are
/// not surfaces (springs, damage); the game's ground uses 4 stone, 5 earth, 8 clay (grass, forest floor),
/// 11 snow, 12 debris (gravel), 13 foliage, 15 grass, 20 sand.
pub const SHAPE_MATERIALS: &[&str] = &["none", "wood", "metal", "water", "stone", "earth", "ice", "leather", "clay", "glass", "flesh", "snow", "debris", "foliage", "magic", "grass", "spring1", "spring2", "spring3", "damage", "sand", "movement", "axe"];

pub fn shape_index(name: &str) -> Option<u8> { SHAPE_MATERIALS.iter().position(|x| x.eq_ignore_ascii_case(name)).map(|i| i as u8) }

/// A collision mesh (`.xnvmsh`, `eCResourceCollisionMesh_PS`) of triangle meshes (centimetres, game axes), each
/// with a shape material:
/// ```text
/// u16 1 · u8 1 · u16 1 · u8 1 · "eCResourceCollisionMesh_PS" · u8 1 · u16 0 · u16 83 · u16 83 · u32 size (rest)
/// · u16 30 · u32 1 · ResourcePriority float 0 · u16 64 · u8 convex (0) · f32 0 · u32 meshes
/// · per mesh: u64 stream bytes · PhysX stream (metres)
/// · u16 30 · u32 bytes of all streams · box of all (mostly empty: FLT_MAX, -FLT_MAX) · box per mesh (cm) · u8 convex · u32 meshes
/// · per mesh: u8 shape material · u8 ignored by trace ray · u8 no collision · u8 no response
/// ```
/// The tail is the game's own byte for byte (see the test); a different one makes the game drop the collision.
pub fn xnvmsh(meshes: &[(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)]) -> Result<Vec<u8>> {
    let mut x = crate::g3_res::Xnv { streams: vec![], all: EMPTY_BOX, convex: 0, shapes: vec![] };
    for (verts, tris, mat) in meshes {
        let m: Vec<[f32; 3]> = verts.iter().map(|v| v.map(|x| x / 100.0)).collect();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in verts { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
        x.streams.push((crate::nxs::write_trimesh(&crate::cook::cook(&m, tris, &[])?), (lo, hi)));
        x.shapes.push([*mat, 0, 0, 0]);
    }
    Ok(xnvmsh_write(&x))
}

/// The box of all meshes as the game mostly writes it: empty.
pub const EMPTY_BOX: ([f32; 3], [f32; 3]) = ([f32::MAX; 3], [-f32::MAX; 3]);

pub fn xnvmsh_write(x: &crate::g3_res::Xnv) -> Vec<u8> {
    let mut o = vec![];
    let s = |o: &mut Vec<u8>, x: &str| { o.extend((x.len() as u16).to_le_bytes()); o.extend(x.as_bytes()); };
    o.extend(1u16.to_le_bytes()); o.push(1); o.extend(1u16.to_le_bytes()); o.push(1);
    s(&mut o, "eCResourceCollisionMesh_PS");
    o.push(1); o.extend(0u16.to_le_bytes()); o.extend(83u16.to_le_bytes()); o.extend(83u16.to_le_bytes());
    let size_at = o.len();
    o.extend(0u32.to_le_bytes());
    o.extend(30u16.to_le_bytes()); o.extend(1u32.to_le_bytes());
    s(&mut o, "ResourcePriority"); s(&mut o, "float"); o.extend(30u16.to_le_bytes()); o.extend(4u32.to_le_bytes()); o.extend(0f32.to_le_bytes());
    o.extend(64u16.to_le_bytes()); o.push(x.convex); o.extend(0f32.to_le_bytes());
    o.extend(xnvmsh_body(x));
    let rest = (o.len() - size_at - 4) as u32;
    o[size_at..size_at + 4].copy_from_slice(&rest.to_le_bytes());
    o
}

/// From the stream count to the end: streams, then the tail.
fn xnvmsh_body(x: &crate::g3_res::Xnv) -> Vec<u8> {
    let mut o = vec![];
    o.extend((x.streams.len() as u32).to_le_bytes());
    let mut total = 0u32;
    for (st, _) in &x.streams { o.extend((st.len() as u64).to_le_bytes()); o.extend(st); total += st.len() as u32; }
    o.extend(30u16.to_le_bytes()); o.extend(total.to_le_bytes());
    let bx = |o: &mut Vec<u8>, b: &([f32; 3], [f32; 3])| for v in b.0.iter().chain(&b.1) { o.extend(v.to_le_bytes()); };
    bx(&mut o, &x.all);
    for (_, b) in &x.streams { bx(&mut o, b); }
    o.push(x.convex);
    o.extend((x.shapes.len() as u32).to_le_bytes());
    for m in &x.shapes { o.extend(m); }
    o
}

#[cfg(test)]
mod collision_tests {
    #[test]
    fn a_written_collision_reads_back() {
        let verts = vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0], [0.0, 0.0, 100.0]];
        let tris = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let d = super::xnvmsh(&[(verts.clone(), tris.clone(), 4)]).unwrap();
        let back = crate::g3_res::xnvmsh_triangles(&d).unwrap();
        assert_eq!(back.len(), 4);
        for t in &back { for p in t { assert!(verts.iter().any(|v| (0..3).all(|k| (v[k] - p[k]).abs() < 1e-3)), "{p:?}"); } }
    }
}

#[cfg(test)]
mod collision_game_tests {
    /// Parsing a shipped collision (triangle meshes or convex) and writing it again gives the game's bytes, from the mesh count
    /// to the string table.
    #[test]
    fn shipped_collision_tails_survive_a_rewrite() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let mut n = 0;
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xnvmsh")).step_by(5) {
            let d = g.read(&k).unwrap();
            if !d.starts_with(b"GENOMFLE") { continue; }
            let p0 = d.windows(8).position(|w| w == b"NXS\x01MESH");
            let Some(p0) = p0 else { continue };
            let meshes = crate::g3_res::parse_xnvmsh(&d).unwrap();
            let x = meshes;
            let tab = u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize;
            let (a, b) = (super::xnvmsh_body(&x), &d[p0 - 12..tab]);
            if let Some(i) = (0..a.len().min(b.len())).find(|&i| a[i] != b[i]).or((a.len() != b.len()).then(|| a.len().min(b.len()))) { panic!("{k}: differs at {i} of {}/{}: ours {:?} game {:?}", a.len(), b.len(), &a[i.saturating_sub(8)..(i + 24).min(a.len())], &b[i.saturating_sub(8)..(i + 24).min(b.len())]); }
            n += 1;
        }
        assert!(n > 500, "{n} files");
    }
}
