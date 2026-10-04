//! Mods in the game: each installed mod keeps its files under `<game>/gothic3_impexp/mods/<mod>/<archive>/<path>`,
//! and for every archive one volume of ours, `Data/<archive>.pNN`, holds the files of all installed mods (a later
//! mod wins). NN is the first number after the game's own volumes: the engine mounts `%s.p00`, `%s.p01`, … over
//! the `.pak` in order. Installing or removing a mod rebuilds those volumes; the archives are never touched.
//!
//! `registry.json` lists the mods in install order and the volumes we own.
//!
//! A mod package is the volumes of one mod plus `INSTALL.bat`, which copies each to the next free `.pNN` of its
//! archive and notes it, and `ROLLBACK.bat`, which deletes what was noted.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const ARCHIVES: &[&str] = &["_compiledMesh", "_compiledMaterial", "_compiledImage", "_compiledPhysic", "_compiledAnimation", "Lightmaps"];

#[derive(Serialize, Deserialize, Default)]
pub struct Registry {
    /// Mod names in install order.
    pub mods: Vec<String>,
    /// Our volumes, archive -> file name in Data.
    pub volumes: BTreeMap<String, String>,
}

fn root(game: &Path) -> PathBuf { game.join("gothic3_impexp") }
fn reg_path(game: &Path) -> PathBuf { root(game).join("registry.json") }

pub fn load(game: &Path) -> Registry {
    std::fs::read_to_string(reg_path(game)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(game: &Path, r: &Registry) -> Result<()> {
    std::fs::create_dir_all(root(game))?;
    std::fs::write(reg_path(game), serde_json::to_string_pretty(r)?)?;
    Ok(())
}

pub fn game_running() -> bool {
    std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq Gothic3.exe", "/NH"]).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("gothic3.exe")).unwrap_or(false)
}

/// Volume number in a `<archive>.pNN` file name.
fn volume_index(name: &str, archive: &str) -> Option<u32> {
    let l = name.to_lowercase();
    let rest = l.strip_prefix(&format!("{}.p", archive.to_lowercase()))?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) { return None; }
    rest.parse().ok()
}

fn volume_name(archive: &str, i: u32) -> String { if i < 10 { format!("{archive}.p0{i}") } else { format!("{archive}.p{i}") } }

/// The files of mod `name`: (archive, path inside it, source file).
fn mod_files(game: &Path, name: &str) -> Vec<(String, String, PathBuf)> {
    let base = root(game).join("mods").join(name);
    let mut out = vec![];
    for a in ARCHIVES {
        let dir = base.join(a);
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { stack.push(p); } else if let Ok(rel) = p.strip_prefix(&dir) {
                    out.push((a.to_string(), rel.to_string_lossy().replace('\\', "/"), p.clone()));
                }
            }
        }
    }
    out
}

/// Rebuild our volume of every archive from the installed mods (a later mod's file wins).
fn rebuild(game: &Path, reg: &mut Registry) -> Result<()> {
    let data = game.join("Data");
    let mut per: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for m in &reg.mods { for (a, rel, src) in mod_files(game, m) { per.entry(a).or_default().insert(rel.to_lowercase(), src); } }
    for a in ARCHIVES {
        let ours = reg.volumes.get(*a).cloned();
        if let Some(v) = &ours { let _ = std::fs::remove_file(data.join(v)); }
        reg.volumes.remove(*a);
        let Some(files) = per.get(*a) else { continue };
        // First number after the game's own volumes (ours was just removed).
        let next = std::fs::read_dir(&data)?.flatten().filter_map(|e| volume_index(&e.file_name().to_string_lossy(), a)).max().map(|x| x + 1).unwrap_or(0);
        let mut list = vec![];
        for (rel, src) in files {
            let bytes = std::fs::read(src).with_context(|| format!("read {}", src.display()))?;
            let rel = src.strip_prefix(root(game).join("mods")).ok().and_then(|p| p.components().skip(2).map(|c| c.as_os_str().to_string_lossy().into_owned()).reduce(|x, y| format!("{x}/{y}"))).unwrap_or_else(|| rel.clone());
            list.push((rel, bytes));
        }
        let name = volume_name(a, next);
        std::fs::write(data.join(&name), crate::volume::write(a, &list, crate::volume::filetime_now()))?;
        reg.volumes.insert(a.to_string(), name);
    }
    Ok(())
}

/// Install `files` (archive, path, bytes) as mod `name` (replacing an installed mod of that name).
pub fn install(game: &Path, name: &str, files: &[(String, String, Vec<u8>)]) -> Result<Vec<String>> {
    if game_running() { bail!("close Gothic 3 first"); }
    let dir = root(game).join("mods").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    for (a, rel, bytes) in files {
        let p = dir.join(a).join(rel.replace('/', "\\"));
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(&p, bytes)?;
    }
    let mut reg = load(game);
    reg.mods.retain(|m| m != name);
    reg.mods.push(name.to_string());
    rebuild(game, &mut reg)?;
    save(game, &reg)?;
    Ok(reg.volumes.values().cloned().collect())
}

pub fn uninstall(game: &Path, name: &str) -> Result<()> {
    if game_running() { bail!("close Gothic 3 first"); }
    let mut reg = load(game);
    if !reg.mods.iter().any(|m| m == name) { bail!("{name} is not installed by Gothic 3 ImpExp"); }
    reg.mods.retain(|m| m != name);
    let _ = std::fs::remove_dir_all(root(game).join("mods").join(name));
    rebuild(game, &mut reg)?;
    save(game, &reg)
}

/// Installed mods with their files.
pub fn installed(game: &Path) -> Vec<(String, Vec<String>)> {
    load(game).mods.iter().map(|m| (m.clone(), mod_files(game, m).into_iter().map(|(a, rel, _)| format!("{a}/{rel}")).collect())).collect()
}

/// A package folder: one volume per archive (`<archive>.vol`) plus INSTALL.bat and ROLLBACK.bat.
pub fn package(out: &Path, title: &str, files: &[(String, String, Vec<u8>)]) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut per: BTreeMap<&str, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for (a, rel, bytes) in files { per.entry(a.as_str()).or_default().push((rel.clone(), bytes.clone())); }
    for (a, list) in &per { std::fs::write(out.join(format!("{a}.vol")), crate::volume::write(a, list, crate::volume::filetime_now()))?; }
    let pct = |s: &str| s.replace('%', "%%");
    let title = pct(title);
    let mut install = format!("@echo off\r\nsetlocal EnableDelayedExpansion\r\nrem {title} - Gothic 3 mod. Usage: INSTALL.bat [Gothic 3 folder]\r\nset \"GAME=%~1\"\r\nif \"%GAME%\"==\"\" set \"GAME=C:\\Program Files (x86)\\Steam\\steamapps\\common\\Gothic 3\"\r\nif not exist \"%GAME%\\Data\" (echo Gothic 3 not found at %GAME% - run INSTALL.bat \"<game folder>\" & pause & exit /b 1)\r\ntasklist /FI \"IMAGENAME eq Gothic3.exe\" | \"%SystemRoot%\\System32\\find.exe\" /I \"Gothic3.exe\" >nul && (echo Close Gothic 3 first. & pause & exit /b 1)\r\nif exist \"%~dp0installed.txt\" (echo Already installed - run ROLLBACK.bat first. & pause & exit /b 1)\r\n");
    for a in per.keys() {
        install += &format!("set \"N=\"\r\nfor /L %%i in (0,1,99) do if not defined N (set \"I=%%i\" & if %%i LSS 10 set \"I=0%%i\" & if not exist \"%GAME%\\Data\\{a}.p!I!\" set \"N=!I!\")\r\ncopy /Y \"%~dp0{a}.vol\" \"%GAME%\\Data\\{a}.p!N!\" >nul || (echo Copy failed & pause & exit /b 1)\r\necho %GAME%\\Data\\{a}.p!N!>>\"%~dp0installed.txt\"\r\n");
    }
    install += &format!("echo {title} installed.\r\npause\r\n");
    let rollback = format!("@echo off\r\nrem {title} - remove what INSTALL.bat added.\r\nif not exist \"%~dp0installed.txt\" (echo Nothing to remove. & pause & exit /b 0)\r\ntasklist /FI \"IMAGENAME eq Gothic3.exe\" | \"%SystemRoot%\\System32\\find.exe\" /I \"Gothic3.exe\" >nul && (echo Close Gothic 3 first. & pause & exit /b 1)\r\nfor /F \"usebackq delims=\" %%f in (\"%~dp0installed.txt\") do del \"%%f\"\r\ndel \"%~dp0installed.txt\"\r\necho {title} removed.\r\npause\r\n");
    std::fs::write(out.join("INSTALL.bat"), install)?;
    std::fs::write(out.join("ROLLBACK.bat"), rollback)?;
    std::fs::write(out.join("README.txt"), format!("{title}\r\n\r\nGothic 3 mod made with Gothic 3 ImpExp.\r\nInstall: run INSTALL.bat (or INSTALL.bat \"<Gothic 3 folder>\"). Remove: ROLLBACK.bat.\r\nEach .vol becomes the next free patch volume (.pNN) of its archive in Gothic 3\\Data; the game's archives are not changed.\r\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_game() -> PathBuf {
        let g = std::env::temp_dir().join(format!("g3mods-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(g.join("Data")).unwrap();
        std::fs::write(g.join("Data").join("_compiledMesh.pak"), b"").unwrap();
        std::fs::write(g.join("Data").join("_compiledImage.p00"), b"").unwrap();
        std::fs::write(g.join("Data").join("_compiledImage.p01"), b"").unwrap();
        g
    }

    #[test]
    fn volumes_follow_the_games_own_and_go_with_the_last_mod() {
        let g = fake_game();
        let a = vec![("_compiledMesh".to_string(), "x/A.xcmsh".to_string(), b"a".to_vec()), ("_compiledImage".to_string(), "x/A.ximg".to_string(), b"ai".to_vec())];
        let b = vec![("_compiledMesh".to_string(), "x/A.xcmsh".to_string(), b"b".to_vec())];
        install(&g, "A", &a).unwrap();
        install(&g, "B", &b).unwrap();
        let reg = load(&g);
        assert_eq!(reg.volumes.get("_compiledMesh").map(String::as_str), Some("_compiledMesh.p00"));
        assert_eq!(reg.volumes.get("_compiledImage").map(String::as_str), Some("_compiledImage.p02"));
        // B's file wins in the mesh volume.
        let v = crate::g3::read_volume(&g.join("Data").join("_compiledMesh.p00")).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path, "x/A.xcmsh");
        let vol = std::fs::read(g.join("Data").join("_compiledMesh.p00")).unwrap();
        let mut out = vec![];
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&vol[v[0].offset as usize..(v[0].offset + v[0].stored) as usize]), &mut out).unwrap();
        assert_eq!(out, b"b");
        uninstall(&g, "B").unwrap();
        uninstall(&g, "A").unwrap();
        assert!(!g.join("Data").join("_compiledMesh.p00").exists());
        assert!(!g.join("Data").join("_compiledImage.p02").exists());
        assert!(g.join("Data").join("_compiledImage.p01").exists(), "the game's own volume stays");
        std::fs::remove_dir_all(&g).ok();
    }

    #[test]
    fn the_package_installer_takes_the_next_free_volume() {
        let g = fake_game();
        let pkg = g.join("pkg 100%");
        package(&pkg, "Test 50% mod", &[("_compiledImage".to_string(), "x/A.ximg".to_string(), b"ai".to_vec())]).unwrap();
        let st = std::process::Command::new("cmd").arg("/c").arg(pkg.join("INSTALL.bat")).arg(&g).stdin(std::process::Stdio::null()).output().unwrap();
        assert!(g.join("Data").join("_compiledImage.p02").exists(), "{}", String::from_utf8_lossy(&st.stdout));
        std::process::Command::new("cmd").arg("/c").arg(pkg.join("ROLLBACK.bat")).stdin(std::process::Stdio::null()).output().unwrap();
        assert!(!g.join("Data").join("_compiledImage.p02").exists());
        assert!(g.join("Data").join("_compiledImage.p01").exists());
        std::fs::remove_dir_all(&g).ok();
    }
}
