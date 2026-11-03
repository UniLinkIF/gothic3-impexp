//! Gothic 3's trees as stand-ins to look at: a `.spt` (`Speedtrees/…`, SpeedTree `__IdvSpt_02_`) is only the recipe the
//! game grows a tree from, so a tree comes in as a bark cylinder and crossed cards of the game's leaf atlas, as tall as
//! the recipe says. For the view only; nothing writes them back.
//!
//! ```text
//! u32 1000 · u32 12 "__IdvSpt_02_" · u32 1002 · u32 2000 · u32 n · bark texture (n bytes, `.tga`) · …
//! · u32 2006 · f32 size (m: XS buckthorn 3, L Douglas fir 30) · u32 2007 · f32 size variance · …
//! · … "G3_Speedtree_Misc_Composite_NN_Diffuse_01.dds" (the leaf atlas, `_compiledImage/SpeedTree/…ximg`)
//! ```
//! Tree placements: `eCSpeedTree_PS.ResourceFilePath` (`bCSpeedTreeResourceString`), all 57 380 in the static cells;
//! their matrices scale by 0.59–1.32, so the recipe's size is in metres.

use crate::g3::G3Ctx;
use anyhow::{bail, Context, Result};
use std::fmt::Write as _;
use std::path::Path;

/// What a recipe says that a stand-in needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe { pub size: f32, pub variance: f32, pub bark: Option<String>, pub leaves: Option<String> }

fn u32_at(d: &[u8], p: usize) -> Option<u32> { d.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())) }

pub fn recipe(d: &[u8]) -> Result<Recipe> {
    if d.get(8..20) != Some(b"__IdvSpt_02_") || u32_at(d, 0) != Some(1000) { bail!("not a SpeedTree recipe (__IdvSpt_02_)"); }
    let bark = (u32_at(d, 24) == Some(2000)).then(|| u32_at(d, 28)).flatten().and_then(|n| d.get(32..32 + n as usize)).map(|s| String::from_utf8_lossy(s).into_owned());
    let at = d.windows(4).position(|w| w == 2006u32.to_le_bytes()).filter(|&p| u32_at(d, p + 8) == Some(2007)).context("no tree size (token 2006)")?;
    let f = |p: usize| f32::from_bits(u32_at(d, p).unwrap());
    let (size, variance) = (f(at + 4), f(at + 12));
    if !(size > 0.0 && size < 200.0) { bail!("tree size {size}"); }
    // The leaf atlas: the last `…Composite…` name.
    let mut leaves = None;
    let mut i = 0;
    while let Some(p) = d[i..].windows(9).position(|w| w.eq_ignore_ascii_case(b"composite")).map(|p| p + i) {
        let s = (0..p).rev().take_while(|&k| d[k].is_ascii_graphic()).last().unwrap_or(p);
        let e = (p..d.len()).find(|&k| !d[k].is_ascii_graphic()).unwrap_or(d.len());
        let name = String::from_utf8_lossy(&d[s..e]).into_owned();
        if let Some(n) = name.to_lowercase().find(".dds").map(|n| name[..n].to_string()) { if !n.to_lowercase().contains("shadow") { leaves = Some(n); } }
        i = e;
    }
    Ok(Recipe { size, variance, bark, leaves })
}

/// The stand-in of tree `spt` (file name, any case) as OBJ + MTL + PNGs in `out_dir`, in the model import's form (game
/// axes mirrored in Z, centimetres): bark material, leaf material (alpha test at 64).
pub fn stand_in(g: &G3Ctx, spt: &str, out_dir: &Path) -> Result<crate::staticmesh::MeshOut> {
    let key = g.find("speedtrees", &format!("{}.spt", spt.trim_end_matches(".spt"))).with_context(|| format!("{spt}: no such tree in Gothic 3"))?;
    let stem = crate::stem_of(g.path_of(&key).unwrap_or(&key));
    let r = recipe(&g.read(&key)?).with_context(|| key.clone())?;
    std::fs::create_dir_all(out_dir)?;
    let mut warnings = vec![];
    let tex = |name: &Option<String>, normal: bool, warnings: &mut Vec<String>| -> Option<String> {
        let n = name.as_ref()?.trim_end_matches(".tga").trim_end_matches(".dds").to_string();
        match crate::textures::png(g, &n, out_dir, normal) { Ok(Some(p)) => Some(p.file_name().unwrap().to_string_lossy().into_owned()), Ok(None) => { warnings.push(format!("{stem}: image {n} not in the game")); None } Err(e) => { warnings.push(format!("{stem}: {n}: {e:#}")); None } }
    };
    let bark = crate::staticmesh::Material { name: format!("SPT_Bark_{stem}"), diffuse: tex(&r.bark, false, &mut warnings), normal: None, specular: None, blend: 0, mask: 128 };
    let leaves = crate::staticmesh::Material { name: format!("SPT_Leaves_{}", r.leaves.clone().unwrap_or_else(|| stem.clone())), diffuse: tex(&r.leaves, false, &mut warnings), normal: None, specular: None, blend: crate::g3_res::BLEND_MASKED, mask: 64 };
    let h = r.size * 100.0;
    let mut obj = format!("# Gothic 3 SpeedTree {stem}: a stand-in to look at (size {} m), game axes mirrored in Z, centimetres\nmtllib {stem}.mtl\no {stem}\n", r.size);
    // Trunk: 8 sides up to 70 % of the height, 3 % of it thick.
    let (rad, top, sides) = (h * 0.03, h * 0.7, 8);
    for k in 0..sides { let a = k as f32 / sides as f32 * std::f32::consts::TAU; for y in [0.0, top] { let _ = writeln!(obj, "v {} {} {}", rad * a.cos(), y, -(rad * a.sin())); } }
    for k in 0..=sides { for v in [0.0, 3.0] { let _ = writeln!(obj, "vt {} {}", k as f32 / sides as f32, v); } }
    // Crown: three crossed cards, 70 % of the height wide, from 35 % to the top.
    let (w, y0) = (h * 0.35, h * 0.35);
    for k in 0..3 {
        let a = k as f32 / 3.0 * std::f32::consts::PI;
        let (dx, dz) = (w * a.cos(), w * a.sin());
        for (x, y, z) in [(-dx, y0, -dz), (dx, y0, dz), (dx, h, dz), (-dx, h, -dz)] { let _ = writeln!(obj, "v {x} {y} {}", -z); }
    }
    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] { let _ = writeln!(obj, "vt {u} {v}"); }
    let _ = writeln!(obj, "usemtl {}", bark.name);
    for k in 0..sides {
        let (a, b) = (1 + k * 2, 1 + ((k + 1) % sides) * 2);
        let (ta, tb) = (1 + k * 2, 1 + (k + 1) * 2);
        let _ = writeln!(obj, "f {}/{} {}/{} {}/{} {}/{}", a, ta, b, tb, b + 1, tb + 1, a + 1, ta + 1);
    }
    let _ = writeln!(obj, "usemtl {}", leaves.name);
    let (vc, tc) = (sides * 2 + 1, (sides + 1) * 2 + 1);
    for k in 0..3 { let b = vc + k * 4; let _ = writeln!(obj, "f {}/{} {}/{} {}/{} {}/{}", b, tc, b + 1, tc + 1, b + 2, tc + 2, b + 3, tc + 3); }
    let mut mtl = String::new();
    for m in [&bark, &leaves] {
        let _ = writeln!(mtl, "newmtl {}\nKd 0.8 0.8 0.8", m.name);
        if let Some(d) = &m.diffuse { let _ = writeln!(mtl, "map_Kd {d}"); }
    }
    let obj_path = out_dir.join(format!("{stem}.obj"));
    std::fs::write(&obj_path, obj)?;
    std::fs::write(out_dir.join(format!("{stem}.mtl")), mtl)?;
    Ok(crate::staticmesh::MeshOut { entry: key, obj: obj_path.to_string_lossy().into_owned(), vertices: sides * 2 + 12, triangles: sides * 2 + 6, materials: vec![bark, leaves], warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every recipe of the game gives a size, its bark and its leaf atlas, and the atlas is an image of the game; the
    /// sizes follow the classes in the names (XS < S < M < L < XXL, on average).
    #[test]
    fn every_recipe_reads() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let mut by_class: std::collections::BTreeMap<&str, Vec<f32>> = Default::default();
        let mut n = 0;
        for k in g.keys().into_iter().filter(|k| k.ends_with(".spt")) {
            let r = recipe(&g.read(&k).unwrap()).unwrap_or_else(|e| panic!("{k}: {e:#}"));
            assert!(r.bark.is_some(), "{k}");
            let l = r.leaves.as_deref().unwrap_or_else(|| panic!("{k}: no leaf atlas"));
            assert!(g.find("_compiledimage", &format!("{l}.ximg")).is_some(), "{k}: {l}");
            for c in ["_xs_", "_s_", "_m_", "_l_", "_xxl_"] { if k.contains(c) { by_class.entry(c).or_default().push(r.size); } }
            n += 1;
        }
        assert_eq!(n, 98);
        let mean = |c: &str| { let v = &by_class[c]; v.iter().sum::<f32>() / v.len() as f32 };
        eprintln!("{:?}", ["_xs_", "_s_", "_m_", "_l_", "_xxl_"].map(|c| (c, mean(c))));
        assert!(mean("_xs_") < mean("_s_") && mean("_s_") < mean("_m_") && mean("_m_") < mean("_l_") && mean("_l_") < mean("_xxl_"));
    }

    #[test]
    fn a_tree_stands_in_as_trunk_and_cards() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = G3Ctx::open_original(Path::new(&root)).unwrap();
        let dir = std::env::temp_dir().join(format!("g3ie_test_spt_{}", std::process::id()));
        let o = stand_in(&g, "G3_Tree_L_DouglasFir_01", &dir).unwrap();
        assert!(o.materials.iter().all(|m| m.diffuse.is_some()), "{:?} {:?}", o.materials, o.warnings);
        assert_eq!((o.materials[1].blend, o.materials[1].mask), (crate::g3_res::BLEND_MASKED, 64));
        let obj = std::fs::read_to_string(&o.obj).unwrap();
        let top = obj.lines().filter_map(|l| l.strip_prefix("v ")).map(|l| l.split(' ').nth(1).unwrap().parse::<f32>().unwrap()).fold(0f32, f32::max);
        assert!((top - 3000.0).abs() < 1.0, "{top}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
