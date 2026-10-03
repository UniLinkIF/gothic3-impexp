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

/// `_compiledimage` key of a texture named in a material or actor (`X.tga`, `X` or a path).
pub fn image_key(g: &G3Ctx, tex: &str) -> Option<String> {
    let file = tex.rsplit(['/', '\\']).next()?;
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    g.find("_compiledimage", &format!("{stem}.ximg"))
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

/// The texture as `<cache>/<stem>.png` (`<stem>_n.png` for a normal map), written once; None when the game has
/// no such image.
pub fn png(g: &G3Ctx, tex: &str, cache: &Path, normal: bool) -> Result<Option<PathBuf>> {
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
