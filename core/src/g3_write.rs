//! Writing Gothic 3 images (`.ximg`) and materials (`.xshmat`).
//!
//! Image, as the game's (`GENOMFLE` + `G3IMG`, measured on the install):
//! ```text
//! 0   "GENOMFLE" · u16 1 · u32 string-table offset · u16 0x20 · u32 pixel sum · u32 (varies; 0 here) · FILETIME
//!     · u32 source size (its TGA: w·h·3 + 44, or w·h·4 + 44 with alpha) · u16 1
//! 38  "G3IMG" · u16 2 · u16 47 · u32 width · u32 height · u64 (varies; 0 here) · u32 mip levels · u32 0 · FourCC
//!     · u32 1 · u32 pixel sum (Σ w·h over the levels, halved for DXT1) · u32 0
//! 87  mip levels, smallest first, full size last (DXT blocks; a level under 4×4 still takes one block), down to
//!     where the shorter side is 1
//!     string table: u32 0xDEADBEEF · u8 1 · u32 0 (no strings)
//! ```
//! Material: a shipped `GENOMFLE` material is the template; its texture names (`*.tga` in the string table) become
//! ours, `BlendMode` the look's, and an alpha-tested one gets our cut-off in `MaskReference`.

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
    // The game's chains end where the shorter side reaches 1 (128×256 has 8 levels, down to 1×2).
    let mut levels = crate::dds::build_mip_chain(&base, nw, nh);
    levels.truncate(nw.min(nh).trailing_zeros() as usize + 1);
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
    o.extend(0x20u16.to_le_bytes());
    o.extend(sum.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(filetime.to_le_bytes());
    // The size of the source picture (a TGA: 18 head + pixels + 26 foot), 32-bit when it has an alpha channel.
    let opaque = base.chunks_exact(4).all(|p| p[3] == 255);
    o.extend((nw * nh * if opaque { 3 } else { 4 } + 44).to_le_bytes());
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
    o.extend(0u32.to_le_bytes());
    debug_assert_eq!(o.len(), 87);
    o.extend(data);
    o.extend(0xDEADBEEFu32.to_le_bytes());
    o.push(1);
    o.extend(0u32.to_le_bytes());
    Ok(o)
}

/// A Blender normal map (RGB, y up) as Gothic 3 keeps them (RGB, y down).
pub fn normal_to_game(rgba: &[u8]) -> Vec<u8> { rgba.chunks_exact(4).flat_map(|p| [p[0], 255 - p[1], p[2], 255]).collect() }

/// A material template: the shipped material, the role of each of its texture strings in table order, and the
/// `BlendMode` the written material has (the template's own, but for the glow, which is the blend's with 7).
///
/// Measured on the install's 1143 materials: a material shows how it is drawn by `BlendMode` (0 opaque, 1 alpha
/// test with `MaskReference`, 2 alpha blend, 7 additive) and what it is drawn on by `TransformationType` (0 static,
/// 2 skinned: 192 of the 193 materials actors use, every one static meshes use is 0 or unset). Each template is a
/// material of the most common make (the same samplers and combiners) among the shipped ones of its look.
pub struct Template { pub material: &'static str, pub roles: &'static [&'static str], pub masked: bool, pub blend: u32 }

/// Opaque, diffuse + normal (61 of the 97 such static materials are made like this wood; the rest add parallax,
/// scaling or reflections).
pub static OPAQUE: Template = Template { material: "G3_Architecture_Wood_08_A", roles: &["diffuse", "normal"], masked: false, blend: 0 };
/// Opaque with a specular map (68 of 76 are made like this dungeon floor).
pub static OPAQUE_SPECULAR: Template = Template { material: "G3_Varant_Dungeon_Ground_01", roles: &["diffuse", "normal", "specular"], masked: false, blend: 0 };
/// Alpha test (a straw roof; BlendMode 1, the cut-off in MaskReference).
pub static MASKED: Template = Template { material: "G3_Architecture_Strawroof_02_A", roles: &["diffuse", "normal"], masked: true, blend: 1 };
/// Alpha test with a specular map (a drapery).
pub static MASKED_SPECULAR: Template = Template { material: "G3_Architecture_Drapery_02_A", roles: &["diffuse", "normal", "specular"], masked: true, blend: 1 };
/// Alpha blend, see-through by the diffuse's alpha (BlendMode 2; the shipped ones are diffuse only).
pub static BLEND: Template = Template { material: "G3_Nature_Tree_Root_01_E", roles: &["diffuse"], masked: false, blend: 2 };
/// Alpha blend with normal and specular maps (the one shipped: a well's glass).
pub static BLEND_SPECULAR: Template = Template { material: "G3_Desert_Well_Alpha_01", roles: &["diffuse", "normal", "specular"], masked: false, blend: 2 };
/// Additive glow: BlendMode 7, which Engine.dll registers as `Brighten` (6 is `Darken`), the mode of the game's light
/// streaks, magic barrier, glowing crystal and waterfall foam. Those four all move their texture (rotate, scroll,
/// oscillate), so the glow is the plain blend template with BlendMode 7.
pub static GLOW: Template = Template { material: "G3_Nature_Tree_Root_01_E", roles: &["diffuse"], masked: false, blend: 7 };
/// Skinned (TransformationType 2), for actors: opaque — a ranger's body.
pub static ACTOR_OPAQUE: Template = Template { material: "G3_Animation_Body_Ranger_01_A", roles: &["diffuse", "normal"], masked: false, blend: 0 };
/// Skinned, opaque with a specular map — the stalker.
pub static ACTOR_SPECULAR: Template = Template { material: "G3_Animation_Monster_Stalker_01", roles: &["diffuse", "normal", "specular"], masked: false, blend: 0 };
/// Skinned alpha test — the hair 110 actors wear (diffuse only, as every skinned alpha test the game has).
pub static ACTOR_MASKED: Template = Template { material: "HAIR", roles: &["diffuse"], masked: true, blend: 1 };
/// Skinned alpha blend — the scavenger's feathers.
pub static ACTOR_BLEND: Template = Template { material: "Scavenger_Feathers", roles: &["diffuse"], masked: false, blend: 2 };
/// Skinned additive glow: the feathers with BlendMode 7.
pub static ACTOR_GLOW: Template = Template { material: "Scavenger_Feathers", roles: &["diffuse"], masked: false, blend: 7 };

/// How a material is drawn, from the Blender material.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look { Opaque, AlphaTest, Blend, Glow }

/// The template for a new material of `look` with these maps, for an actor (skinned) or a static mesh, and the maps it
/// has no place for.
pub fn template_for(actor: bool, look: Look, normal: bool, specular: bool) -> (&'static Template, Vec<&'static str>) {
    let t = match (actor, look) {
        (false, Look::Opaque) => if specular { &OPAQUE_SPECULAR } else { &OPAQUE },
        (false, Look::AlphaTest) => if specular { &MASKED_SPECULAR } else { &MASKED },
        (false, Look::Blend) => if normal && specular { &BLEND_SPECULAR } else { &BLEND },
        (false, Look::Glow) => &GLOW,
        (true, Look::Opaque) => if specular { &ACTOR_SPECULAR } else { &ACTOR_OPAQUE },
        (true, Look::AlphaTest) => &ACTOR_MASKED,
        (true, Look::Blend) => &ACTOR_BLEND,
        (true, Look::Glow) => &ACTOR_GLOW,
    };
    let dropped = [("normal", normal), ("specular", specular)].into_iter().filter(|(r, has)| *has && !t.roles.contains(r)).map(|(r, _)| r).collect();
    (t, dropped)
}

/// What the report calls a look.
pub fn look_name(t: &Template) -> &'static str {
    match (t.blend, t.roles.contains(&"specular")) { (7, _) => "glow", (2, _) => "blend", (1, true) => "alpha test, specular", (1, false) => "alpha test", (_, true) => "specular", _ => "opaque" }
}

fn table(d: &[u8]) -> Result<(usize, Vec<String>)> {
    let at = u32::from_le_bytes(d.get(10..14).context("table offset")?.try_into().unwrap()) as usize;
    Ok((at, crate::g3_res::genomfle_strings(d)?))
}

/// Where the value of property `name` of type `ty` starts in a GENOMFLE resource (`[u16 name][u16 type][u16 30]
/// [u32 size]` then the value), and its size.
fn property_at(d: &[u8], strings: &[String], name: &str, ty: &str) -> Option<(usize, usize)> {
    let (ni, ti) = (strings.iter().position(|s| s == name)? as u16, strings.iter().position(|s| s == ty)? as u16);
    let pat = [ni.to_le_bytes(), ti.to_le_bytes(), 30u16.to_le_bytes()].concat();
    let p = d.windows(pat.len()).position(|w| w == pat.as_slice())? + pat.len();
    Some((p + 4, u32::from_le_bytes(d.get(p..p + 4)?.try_into().unwrap()) as usize))
}

/// Neutral pictures for a map a template samples and the Blender material lacks: a flat normal, no shine.
pub fn neutral(role: &str) -> Option<[u8; 4]> { match role { "normal" => Some([128, 128, 255, 255]), "specular" => Some([0, 0, 0, 255]), _ => None } }

/// `template` with its textures replaced by `textures` (role -> image name without extension), `BlendMode` = the
/// template's look and, when masked, `MaskReference` = `cutoff`. A role the template has but `textures` lacks keeps the
/// template's own image. Only these values and the names change: the bytes before the table keep their length.
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
        // u32 size (1) then the char.
        let (p, n) = property_at(&out, &strings, "MaskReference", "char").context("MaskReference value not found")?;
        if n != 1 { bail!("MaskReference size"); }
        out[p] = cut;
    }
    // An enum: u16 version, u32 value.
    let (p, n) = property_at(&out, &strings, "BlendMode", "bTPropertyContainer<enum eEShaderMaterialBlendMode>").context("BlendMode value not found")?;
    if n != 6 { bail!("BlendMode size"); }
    out[p + 2..p + 6].copy_from_slice(&t.blend.to_le_bytes());
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

    /// Our image head is laid out as the game's: the same fields at the same offsets, data from 87.
    #[test]
    fn written_images_have_the_games_head() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let u32_ = |d: &[u8], at: usize| u32::from_le_bytes(d[at..at + 4].try_into().unwrap());
        let (mut seen, mut odd) = (0, vec![]);
        // Textures of models (the interface's and the far-view pictures have one level and other heads).
        for k in g.keys().into_iter().filter(|k| k.ends_with(".ximg") && !k.contains("/__gui") && !k.contains("/_special/")).step_by(5) {
            let d = g.read(&k).unwrap();
            let four = &d[71..75];
            if four != b"DXT1" && four != b"DXT5" { continue; }
            let (w, h) = (u32_(&d, 47), u32_(&d, 51));
            let kind = if four == b"DXT1" { Dxt::Dxt1 } else { Dxt::Dxt5 };
            let ours = ximg(&vec![128u8; (w * h * 4) as usize], w, h, kind, 0).unwrap();
            let same = |a: usize, n: usize| d[a..a + n] == ours[a..a + n];
            // Magic and version; 0x20 and the pixel sum; 1; "G3IMG", 2 and the size (the u16 between varies: 47, 212); levels;
            // FourCC, 1, sum, 0; length.
            let why: Vec<&str> = [("head", same(0, 10)), ("sum", same(14, 6)), ("one", same(36, 2)), ("g3img", same(38, 7) && same(47, 8)), ("levels", same(63, 4)),
                ("tail", same(71, 16)), ("length", d.len() == ours.len() && u32_(&d, 10) == u32_(&ours, 10))].iter().filter(|x| !x.1).map(|x| x.0).collect();
            if !why.is_empty() { odd.push(format!("{k} {w}x{h} {why:?}")); }
            seen += 1;
        }
        assert!(seen > 100, "{seen} images");
        assert!(odd.len() * 100 <= seen * 3, "{} of {seen} differ: {odd:#?}", odd.len());
        // The source size: its TGA, 24-bit without alpha.
        assert_eq!(u32_(&ximg(&[9u8; 64 * 32 * 4], 64, 32, Dxt::Dxt5, 0).unwrap(), 32), 64 * 32 * 4 + 44);
        assert_eq!(u32_(&ximg(&[255u8; 64 * 32 * 4], 64, 32, Dxt::Dxt1, 0).unwrap(), 32), 64 * 32 * 3 + 44);
    }


    pub const ALL: [&Template; 12] = [&OPAQUE, &OPAQUE_SPECULAR, &MASKED, &MASKED_SPECULAR, &BLEND, &BLEND_SPECULAR, &GLOW, &ACTOR_OPAQUE, &ACTOR_SPECULAR, &ACTOR_MASKED, &ACTOR_BLEND, &ACTOR_GLOW];
    const ACTORS: [&Template; 5] = [&ACTOR_OPAQUE, &ACTOR_SPECULAR, &ACTOR_MASKED, &ACTOR_BLEND, &ACTOR_GLOW];

    /// Every template written with our textures reads back strictly: the bytes before the string table are the
    /// template's but for the cut-off and the blend mode, the table is the template's but for the texture names, and the
    /// material says the look it was written for (blend mode, mask, static or skinned).
    #[test]
    fn templates_take_our_textures() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        for t in ALL {
            let k = g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).unwrap();
            let d = g.read(&k).unwrap();
            let tex: Vec<(&str, String)> = t.roles.iter().map(|r| (*r, format!("My_Crate_{r}"))).collect();
            let m = material(&d, t, &tex, Some(77)).unwrap();
            let parsed = crate::g3_res::parse_xshmat(&m).unwrap();
            let want: Vec<String> = t.roles.iter().map(|r| format!("My_Crate_{r}.tga")).collect();
            assert_eq!(parsed.textures, want, "{}", t.material);
            assert_eq!(parsed.blend, Some(t.blend), "{}", t.material);
            let trans = |b: &[u8]| crate::g3_res::enum_property(b, "TransformationType").unwrap();
            assert_eq!(trans(&m), trans(&d));
            assert_eq!(trans(&m), Some(if ACTORS.iter().any(|a| std::ptr::eq(*a, t)) { 2 } else { 0 }), "{}", t.material);
            let (at, mt) = (u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize, u32::from_le_bytes(m[10..14].try_into().unwrap()) as usize);
            assert_eq!(at, mt, "{}: the table moved", t.material);
            let strings = crate::g3_res::genomfle_strings(&d).unwrap();
            let (bp, _) = property_at(&d, &strings, "BlendMode", "bTPropertyContainer<enum eEShaderMaterialBlendMode>").unwrap();
            let mask = if t.masked { Some(property_at(&d, &strings, "MaskReference", "char").unwrap().0) } else { None };
            let diff: Vec<usize> = (0..at).filter(|&i| d[i] != m[i]).collect();
            assert!(diff.iter().all(|&i| (bp + 2..bp + 6).contains(&i) || Some(i) == mask), "{}: bytes {diff:?} changed", t.material);
            if t.masked { assert_eq!(m[mask.unwrap()], 77); }
            let (ours, game) = (crate::g3_res::genomfle_strings(&m).unwrap(), strings);
            assert_eq!(ours.len(), game.len());
            for (a, b) in ours.iter().zip(&game) { if a != b { assert!(want.contains(a), "{}: {b} became {a}", t.material); } }
        }
    }

    /// Each template is the look it stands for among the game's own materials: the blend mode and maps it has, static
    /// (TransformationType 0) or skinned (2) as the materials of static meshes and of actors are, and made the most
    /// common way among the shipped materials of that look (the same samplers, combiners, texture-coordinate sources).
    #[test]
    fn templates_are_made_like_the_games_materials_of_their_look() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        // What static meshes and actors use.
        let mut on_static = std::collections::HashSet::new();
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xcmsh")) {
            let Ok(m) = crate::g3_res::decode_xcmsh(&g.read(&k).unwrap()) else { continue };
            on_static.extend(m.geo.submeshes.iter().map(|s| s.material.trim_end_matches(".xshmat").to_lowercase()));
        }
        let mut on_actor = std::collections::HashSet::new();
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xact")) {
            let Ok(a) = crate::g3_actor::decode_xact(&g.read(&k).unwrap()) else { continue };
            on_actor.extend(a.materials.iter().map(|m| m.name.split('|').next().unwrap().trim_end_matches(".xshmat").to_lowercase()));
        }
        // (blend, transformation, roles) → make → count, over the shipped GENOMFLE materials.
        let mut makes: std::collections::HashMap<(Option<u32>, Option<u32>, Vec<String>), std::collections::HashMap<Vec<String>, usize>> = Default::default();
        let (mut actor_skinned, mut actor_other, mut static_skinned) = (0, 0, 0);
        let make_of = |d: &[u8]| -> (Option<u32>, Option<u32>, Vec<String>, Vec<String>) {
            let strings = crate::g3_res::genomfle_strings(d).unwrap();
            let roles = crate::g3_res::parse_xshmat(d).unwrap().textures.iter().map(|t| crate::textures::role(t).unwrap_or("?").to_string()).collect();
            let mut make: Vec<String> = strings.into_iter().filter(|s| s.starts_with("eC") && !s.starts_with("eCResource") && !s.starts_with("eCShader")).collect();
            make.sort(); make.dedup();
            (crate::g3_res::enum_property(d, "BlendMode").unwrap(), crate::g3_res::enum_property(d, "TransformationType").unwrap(), roles, make)
        };
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xshmat")) {
            let d = g.read(&k).unwrap();
            let stem = k.rsplit('/').next().unwrap().trim_end_matches(".xshmat").to_string();
            let trans = crate::g3_res::enum_property(&d, "TransformationType").unwrap();
            if on_actor.contains(&stem) { if trans == Some(2) { actor_skinned += 1 } else { actor_other += 1 } }
            else if on_static.contains(&stem) && trans == Some(2) { static_skinned += 1; }
            if !d.starts_with(b"GENOMFLE") { continue; }
            let (b, t, roles, make) = make_of(&d);
            *makes.entry((b, t, roles)).or_default().entry(make).or_default() += 1;
        }
        assert!(actor_skinned > 180 && actor_other <= 1 && static_skinned == 0, "actors: {actor_skinned} skinned, {actor_other} not; static meshes: {static_skinned} skinned");
        for t in ALL {
            let d = g.read(&g.find("_compiledmaterial", &format!("{}.xshmat", t.material)).unwrap()).unwrap();
            let (b, tr, roles, make) = make_of(&d);
            assert_eq!(roles, t.roles, "{}", t.material);
            assert_eq!(b, Some(if t.blend == 7 { 2 } else { t.blend }), "{}", t.material);
            assert_eq!(t.masked, t.blend == 1);
            let group = &makes[&(b, tr, roles)];
            let most = group.values().max().unwrap();
            assert_eq!(group[&make], *most, "{}: {make:?} is not the most common make of its look {group:?}", t.material);
        }
    }

    /// The look picks the template; maps it has no place for are named.
    #[test]
    fn looks_pick_their_templates() {
        assert!(std::ptr::eq(template_for(false, Look::Opaque, true, true).0, &OPAQUE_SPECULAR));
        assert!(std::ptr::eq(template_for(false, Look::AlphaTest, true, false).0, &MASKED));
        let (t, dropped) = template_for(false, Look::Blend, true, false);
        assert!(std::ptr::eq(t, &BLEND) && dropped == ["normal"]);
        assert!(std::ptr::eq(template_for(false, Look::Blend, true, true).0, &BLEND_SPECULAR));
        assert_eq!(template_for(true, Look::AlphaTest, true, true).1, vec!["normal", "specular"]);
        assert_eq!(look_name(template_for(false, Look::Glow, false, false).0), "glow");
        assert_eq!(look_name(template_for(true, Look::Opaque, true, true).0), "specular");
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
pub fn xnvmsh(meshes: &[(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)]) -> Result<Vec<u8>> { Ok(xnvmsh_write(&xnv_of(meshes, None)?)) }

/// Cooked meshes, one per surface, by the game's own PhysX cooking library (`g3cook.exe` next to this program
/// runs the 32-bit `NxCooking.dll` from the game folder `game`): the streams the game's PhysX reads. Our own
/// cooker (`cook.rs`) is only used where no game is given (tests); Gothic 3 drops streams it did not cook.
pub fn xnv_of(meshes: &[(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)], game: Option<&std::path::Path>) -> Result<crate::g3_res::Xnv> {
    let mut x = crate::g3_res::Xnv { streams: vec![], all: EMPTY_BOX, convex: 0, shapes: vec![] };
    let streams = match game {
        Some(g) => game_cook(g, meshes, false)?,
        None => meshes.iter().map(|(v, t, _)| Ok(crate::nxs::write_trimesh(&crate::cook::cook(&v.iter().map(|p| p.map(|c| c / 100.0)).collect::<Vec<_>>(), t, &[])?))).collect::<Result<_>>()?,
    };
    for ((verts, _, mat), st) in meshes.iter().zip(streams) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in verts { for k in 0..3 { lo[k] = lo[k].min(v[k]); hi[k] = hi[k].max(v[k]); } }
        x.streams.push((st, (lo, hi)));
        x.shapes.push([*mat, 0, 0, 0]);
    }
    Ok(x)
}

/// Convex hulls of point sets (cm), cooked by the game's PhysX as its own `*_CV` bodies are (CVXM streams).
pub fn convex_streams(game: &std::path::Path, hulls: &[Vec<[f32; 3]>]) -> Result<Vec<Vec<u8>>> {
    let meshes: Vec<(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)> = hulls.iter().map(|p| (p.clone(), vec![], 0)).collect();
    game_cook(game, &meshes, true)
}

/// The meshes (cm) cooked by `g3cook.exe` with the game's `NxCooking.dll`; every triangle gets material 1, as in
/// the game's own streams. `convex`: the hull of each mesh's vertices instead.
fn game_cook(game: &std::path::Path, meshes: &[(Vec<[f32; 3]>, Vec<[u32; 3]>, u8)], convex: bool) -> Result<Vec<Vec<u8>>> {
    // Next to gothic3-core.exe in the add-on; in a development checkout, the built helper in cook32/.
    let here = std::env::current_exe()?;
    let exe = [here.with_file_name("g3cook.exe"), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("cook32").join("g3cook.exe")]
        .into_iter().find(|p| p.is_file()).context("g3cook.exe is missing next to gothic3-core.exe — reinstall the add-on")?;
    if !game.join("NxCooking.dll").is_file() { bail!("NxCooking.dll is not in the game folder {}", game.display()); }
    static RUN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("g3ie_cook_{}_{}", std::process::id(), RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir)?;
    let (inp, out) = (dir.join("in.bin"), dir.join("out.bin"));
    let mut b = (meshes.len() as u32).to_le_bytes().to_vec();
    for (verts, tris, _) in meshes {
        b.extend((verts.len() as u32).to_le_bytes());
        b.extend((tris.len() as u32).to_le_bytes());
        for v in verts { for c in v { b.extend((c / 100.0).to_le_bytes()); } }
        for t in tris { for c in t { b.extend(c.to_le_bytes()); } }
        for _ in tris { b.extend(1u16.to_le_bytes()); }
    }
    std::fs::write(&inp, b)?;
    #[cfg(windows)] use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg(game).arg(&inp).arg(&out);
    if convex { cmd.arg("convex"); }
    #[cfg(windows)] cmd.creation_flags(0x0800_0000);
    let r = cmd.output().context("run g3cook.exe");
    let d = std::fs::read(&out);
    let _ = std::fs::remove_dir_all(&dir);
    let r = r?;
    if !r.status.success() { bail!("the game's PhysX cooking failed: {}", String::from_utf8_lossy(&r.stderr).trim()); }
    let d = d?;
    let n = u32::from_le_bytes(d[0..4].try_into().unwrap()) as usize;
    if n != meshes.len() { bail!("g3cook returned {n} streams for {} meshes", meshes.len()); }
    let mut o = 4;
    let mut v = vec![];
    for _ in 0..n {
        let l = u32::from_le_bytes(d.get(o..o + 4).context("g3cook output")?.try_into().unwrap()) as usize;
        v.push(d.get(o + 4..o + 4 + l).context("g3cook output")?.to_vec());
        o += 4 + l;
    }
    Ok(v)
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

/// `x` in the game's own file `template`: its head (and, in a GENOMFLE file, its string table), our streams and tail.
/// The game keeps both forms (about half of the convex `*_CV` files are plain).
pub fn xnvmsh_in(template: &[u8], x: &crate::g3_res::Xnv) -> Result<Vec<u8>> {
    let p0 = template.windows(4).position(|w| w == b"NXS\x01").context("collision template has no stream")?;
    if p0 < 17 { bail!("collision template: stream at 0x{p0:x}"); }
    let mut o = template[..p0 - 12].to_vec();
    o[p0 - 17] = x.convex;
    o.extend(xnvmsh_body(x));
    if template.starts_with(b"GENOMFLE") {
        let tab = u32::from_le_bytes(template[10..14].try_into().unwrap()) as usize;
        let new_tab = o.len() as u32;
        o.extend(template.get(tab..).context("collision template: string table past the end")?);
        o[10..14].copy_from_slice(&new_tab.to_le_bytes());
        o[29..33].copy_from_slice(&(new_tab - 33).to_le_bytes());
    } else {
        // Plain: the size of the rest sits after the class name and its version words, as `xnvmsh_write` puts it.
        let size = template.get(41..45).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
        if template.get(8..34) != Some(&b"eCResourceCollisionMesh_PS"[..]) || size != Some(template.len() - 45) { bail!("collision template: unknown plain head"); }
        let rest = (o.len() - 45) as u32;
        o[41..45].copy_from_slice(&rest.to_le_bytes());
    }
    Ok(o)
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
            assert_eq!(super::xnvmsh_in(&d, &x).unwrap(), d, "{k}: in its own file");
            let (a, b) = (super::xnvmsh_body(&x), &d[p0 - 12..tab]);
            if let Some(i) = (0..a.len().min(b.len())).find(|&i| a[i] != b[i]).or((a.len() != b.len()).then(|| a.len().min(b.len()))) { panic!("{k}: differs at {i} of {}/{}: ours {:?} game {:?}", a.len(), b.len(), &a[i.saturating_sub(8)..(i + 24).min(a.len())], &b[i.saturating_sub(8)..(i + 24).min(b.len())]); }
            n += 1;
        }
        assert!(n > 500, "{n} files");
    }

    /// Every collision file of the game models use, either form, with triangle meshes or hulls, comes back byte for
    /// byte when written into itself — the frame `xnvmsh_in` puts our streams in.
    #[test]
    fn every_collision_rewrites_into_itself() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut plain, mut genome, mut bad) = (0, 0, vec![]);
        for k in g.keys().into_iter().filter(|k| k.ends_with("_col.xnvmsh") || k.ends_with("_cv.xnvmsh")) {
            let d = g.read(&k).unwrap();
            let x = crate::g3_res::parse_xnvmsh(&d).unwrap();
            // The few older plain files without a shape table are written in today's form.
            if !d.starts_with(b"GENOMFLE") && x.shapes.is_empty() && super::xnvmsh_in(&d, &x).is_ok_and(|o| o.len() == d.len() + 5) { continue; }
            match super::xnvmsh_in(&d, &x) {
                Ok(o) if o == d => if d.starts_with(b"GENOMFLE") { genome += 1 } else { plain += 1 },
                Ok(_) => bad.push(format!("{k}: differs")),
                Err(e) => bad.push(format!("{k}: {e:#}")),
            }
        }
        assert!(bad.is_empty(), "{} of them, e.g. {:#?}", bad.len(), &bad[..bad.len().min(6)]);
        assert!(plain > 300 && genome > 1000, "{plain} plain, {genome} GENOMFLE");
    }
}

#[cfg(test)]
mod engine_tests {
    /// The value the game's Engine.dll registers for each `eEShaderMaterialBlendMode` name (each name has a small
    /// function: `push offset name` … `push value`).
    fn blend_mode_values(dll: &[u8]) -> Vec<(String, u8)> {
        let pe = u32::from_le_bytes(dll[0x3c..0x40].try_into().unwrap()) as usize;
        let (nsec, opt) = (u16::from_le_bytes([dll[pe + 6], dll[pe + 7]]) as usize, u16::from_le_bytes([dll[pe + 20], dll[pe + 21]]) as usize);
        let base = u32::from_le_bytes(dll[pe + 52..pe + 56].try_into().unwrap());
        let secs: Vec<(u32, u32, u32)> = (0..nsec).map(|i| { let o = pe + 24 + opt + i * 40; let r = |k: usize| u32::from_le_bytes(dll[o + k..o + k + 4].try_into().unwrap()); (r(12), r(16), r(20)) }).collect();
        let va = |off: usize| secs.iter().find(|s| off as u32 >= s.2 && (off as u32) < s.2 + s.1).map(|s| base + s.0 + off as u32 - s.2);
        let mut out = vec![];
        let pre = b"eEShaderMaterialBlendMode_";
        let mut i = 0;
        while let Some(p) = dll[i..].windows(pre.len()).position(|w| w == pre).map(|p| p + i) {
            let end = p + dll[p..].iter().position(|&b| b == 0).unwrap();
            i = end;
            if dll[p - 1] != 0 { continue; }
            let name = String::from_utf8_lossy(&dll[p + pre.len()..end]).into_owned();
            let push = [&[0x68u8][..], &va(p).unwrap().to_le_bytes()].concat();
            let Some(c) = dll.windows(5).position(|w| w == push.as_slice()) else { continue };
            // The first `push imm8` after it (Normal registers with two pushes of 0).
            if let Some(k) = (c + 5..c + 40).find(|&k| dll[k] == 0x6a) { out.push((name, dll[k + 1])); }
        }
        out
    }

    /// Engine.dll registers BlendMode 7 as Brighten (6 is Darken): the look the game's own four materials of mode 7 —
    /// light streaks, a magic barrier, a glowing crystal, a waterfall's foam — have, and what the glow templates write.
    #[test]
    fn the_engine_calls_blend_mode_7_brighten() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let dll = std::fs::read(std::path::Path::new(&root).join("Engine.dll")).unwrap();
        let v = blend_mode_values(&dll);
        let want: Vec<(String, u8)> = [("Normal", 0), ("Masked", 1), ("AlphaBlend", 2), ("Modulate", 3), ("AlphaModulate", 4), ("Translucent", 5), ("Brighten", 7), ("Darken", 6), ("Invisible", 8)].iter().map(|(n, x)| (n.to_string(), *x)).collect();
        assert_eq!(v, want);
        assert!(super::GLOW.blend == 7 && super::ACTOR_GLOW.blend == 7);
    }
}
