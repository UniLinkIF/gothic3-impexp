//! Gothic 3 images to PNG for Blender, and the material maps a mesh or actor names.

use crate::g3::G3Ctx;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// RGBA pixels of a compiled image (`_compiledImage/…/<name>.ximg`).
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let img = crate::dds::decode(&crate::g3_res::ximg_dds(bytes)?)?;
    Ok((img.width, img.height, img.rgba))
}

/// A normal map stored as DXT5nm (x in alpha, y in green) or plain RGB, as Blender reads normal maps (RGB, y up).
/// Gothic 3, like Risen, keeps y down (Direct3D), so green is flipped.
pub fn convert_normal(rgba: &[u8]) -> Vec<u8> {
    // DXT5nm leaves red at one value and puts x in alpha.
    let nm = rgba.chunks_exact(4).take(4096).all(|p| p[0] == rgba[0]) && rgba.chunks_exact(4).take(4096).any(|p| p[3] != 255);
    let b = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
    rgba.chunks_exact(4).flat_map(|p| {
        let x = (if nm { p[3] } else { p[0] }) as f32 / 255.0 * 2.0 - 1.0;
        let y = p[1] as f32 / 255.0 * 2.0 - 1.0;
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        [b(x), b(-y), b(z), 255]
    }).collect()
}

/// `_compiledimage` key of a texture named in a material or actor (`X.tga`, `X` or a path). Some actors name a
/// texture the game ships under another suffix (`…_Normal_Amplitude10_01` for `…_Normal_S1`): then the image with
/// the same name up to its role (`_Diffuse`, `_Normal`, `_Specular`) is taken, `_S1` / `_01` first.
pub fn image_key(g: &G3Ctx, tex: &str) -> Option<String> {
    let file = tex.rsplit(['/', '\\']).next()?;
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    if let Some(k) = g.find("_compiledimage", &format!("{stem}.ximg")) { return Some(k); }
    let l = stem.to_lowercase();
    let cut = ["_diffuse", "_normal", "_specular"].iter().filter_map(|r| l.find(r).map(|i| i + r.len())).min()?;
    let base = format!("/{}_", &l[..cut]);
    let mut found: Vec<String> = g.keys().into_iter().filter(|k| k.starts_with("_compiledimage/") && k.contains(&base)).collect();
    found.sort_by_key(|k| (!(k.ends_with("_s1.ximg") || k.ends_with("_01.ximg")), k.len()));
    found.into_iter().next()
}

/// Role of a texture from its name (`_Diffuse_`, `_Normal_`, `_Specular_`: the shipped naming).
pub fn role(name: &str) -> Option<&'static str> {
    let l = name.to_lowercase();
    if l.contains("_diffuse") { Some("diffuse") } else if l.contains("_normal") { Some("normal") } else if l.contains("_specular") { Some("specular") } else { None }
}

/// Diffuse, normal, specular of a material's texture list (the first of each; a lone unnamed one is the diffuse).
pub fn maps(textures: &[String]) -> (Option<String>, Option<String>, Option<String>) {
    let by = |r: &str| textures.iter().find(|t| role(t) == Some(r)).cloned();
    (by("diffuse").or_else(|| textures.iter().find(|t| role(t).is_none()).cloned()), by("normal"), by("specular"))
}

/// The modder's own texture folders (the add-on's preferences), as the add-on hands them over: `G3IE_TEXTURE_DIRS`,
/// separated by `;`.
pub fn user_dirs() -> Vec<PathBuf> {
    std::env::var_os("G3IE_TEXTURE_DIRS").map(|v| std::env::split_paths(&v).filter(|p| p.is_dir()).collect()).unwrap_or_default()
}

/// A picture named `stem` (any case) with one of the extensions the core reads, in `dirs` or their subfolders (four
/// deep), the first folder first.
pub fn user_image(dirs: &[PathBuf], stem: &str) -> Option<PathBuf> {
    fn walk(d: &Path, stem: &str, depth: u32) -> Option<PathBuf> {
        let mut subs = vec![];
        for e in std::fs::read_dir(d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() { if depth < 4 { subs.push(p); } continue; }
            let (Some(s), Some(x)) = (p.file_stem().and_then(|s| s.to_str()), p.extension().and_then(|s| s.to_str())) else { continue };
            if s.eq_ignore_ascii_case(stem) && ["png", "tga", "dds"].iter().any(|e| x.eq_ignore_ascii_case(e)) { return Some(p); }
        }
        subs.sort();
        subs.iter().find_map(|s| walk(s, stem, depth + 1))
    }
    dirs.iter().find_map(|d| walk(d, stem, 0))
}

/// The texture as `<cache>/<stem>.png` (`<stem>_n.png` for a normal map), written once; None when the game has
/// no such image. A picture of that name in the modder's texture folders comes first ([`user_dirs`]).
pub fn png(g: &G3Ctx, tex: &str, cache: &Path, normal: bool) -> Result<Option<PathBuf>> { png_in(g, tex, cache, normal, &user_dirs()) }

/// [`png`] with the modder's folders `dirs`. Their PNG and TGA pictures are taken as Blender shows them (a normal map
/// with y up), a DDS as the game keeps its images (y down, turned like the game's own); each is written again on every
/// call (as `<stem>.user.png`), so an edited picture shows on the next import.
pub fn png_in(g: &G3Ctx, tex: &str, cache: &Path, normal: bool, dirs: &[PathBuf]) -> Result<Option<PathBuf>> {
    let file = tex.rsplit(['/', '\\']).next().unwrap_or(tex);
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    if let Some(p) = user_image(dirs, stem) {
        let dds = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds"));
        let (w, h, rgba) = if dds {
            let img = crate::dds::decode(&std::fs::read(&p)?).with_context(|| format!("image {}", p.display()))?;
            (img.width, img.height, if normal { convert_normal(&img.rgba) } else { img.rgba })
        } else {
            let img = image::open(&p).with_context(|| format!("image {}", p.display()))?.to_rgba8();
            (img.width(), img.height(), img.into_raw())
        };
        std::fs::create_dir_all(cache)?;
        // Apart from the game's own in the cache, which stays for when the picture is gone again.
        let out = cache.join(format!("{stem}{}.user.png", if normal { "_n" } else { "" }));
        let tmp = out.with_extension("png.tmp");
        image::save_buffer_with_format(&tmp, &rgba, w, h, image::ExtendedColorType::Rgba8, image::ImageFormat::Png)?;
        std::fs::rename(&tmp, &out)?;
        return Ok(Some(out));
    }
    let Some(key) = image_key(g, tex) else { return Ok(None) };
    let stem = key.rsplit('/').next().unwrap().trim_end_matches(".ximg");
    let out = cache.join(format!("{stem}{}.png", if normal { "_n" } else { "" }));
    if out.is_file() { return Ok(Some(out)); }
    let (w, h, rgba) = decode(&g.read(&key)?).with_context(|| format!("image {key}"))?;
    let rgba = if normal { convert_normal(&rgba) } else { rgba };
    std::fs::create_dir_all(cache)?;
    let tmp = out.with_extension("png.tmp");
    image::save_buffer_with_format(&tmp, &rgba, w, h, image::ExtendedColorType::Rgba8, image::ImageFormat::Png)?;
    std::fs::rename(&tmp, &out)?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    /// A picture in the modder's folders (any subfolder, any case) stands in for the game's image of that name, as it
    /// is; without the folders the game's own comes back.
    #[test]
    fn the_modders_folders_come_first() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let base = std::env::temp_dir().join(format!("g3ie_test_texdirs_{}", std::process::id()));
        let (dir, cache) = (base.join("my textures"), base.join("cache"));
        std::fs::create_dir_all(dir.join("wood").join("planks")).unwrap();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 200, 30, 255])).save(dir.join("wood").join("planks").join("g3_objects_wood_01_diffuse_02.PNG")).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])).save(dir.join("Unrelated.tga")).unwrap();
        let mine = super::png_in(&g, "G3_Objects_Wood_01_Diffuse_02.tga", &cache, false, &[dir.clone()]).unwrap().unwrap();
        assert!(mine.to_string_lossy().ends_with(".user.png"), "{}", mine.display());
        let px = image::open(&mine).unwrap().to_rgba8();
        assert_eq!((px.dimensions(), px.get_pixel(0, 0).0), ((4, 2), [10, 200, 30, 255]));
        let game = super::png_in(&g, "G3_Objects_Wood_01_Diffuse_02.tga", &cache, false, &[]).unwrap().unwrap();
        assert_ne!(game, mine);
        assert!(image::open(&game).unwrap().width() > 4);
        // A texture the game lacks, found in the folders: a TGA.
        assert!(super::png_in(&g, "Unrelated.tga", &cache, false, &[dir.clone()]).unwrap().is_some());
        assert!(super::png_in(&g, "Unrelated.tga", &cache, false, &[]).unwrap().is_none());
        std::fs::remove_dir_all(&base).ok();
    }
}
