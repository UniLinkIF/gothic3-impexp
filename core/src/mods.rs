//! Mods in the game: each installed mod keeps its files under `<game>/gothic3_impexp/mods/<mod>/<archive>/<path>`,
//! and for every archive one volume of ours, `Data/<archive>.pNN`, holds the files of all installed mods (a later
//! mod wins). The engine mounts `%s.p00`, `%s.p01`, … over the `.pak` in order and **stops at the first missing
//! number**, so the numbers must never have a gap: ours keeps its number from one rebuild to the next, a new one takes
//! the first free number, and when ours is emptied while a volume above it remains, it stays as an empty volume
//! (removed once it is the last). Installing or removing a mod rebuilds those volumes; the archives are never touched.
//!
//! `registry.json` lists the mods in install order and the volumes we own; it is written after every volume.
//!
//! A mod package is the volumes of one mod plus `INSTALL.bat`, which copies each to the first free `.pNN` of its
//! archive and notes it, and `ROLLBACK.bat`, which removes what was noted — or, when a volume above it has come since,
//! leaves an empty volume in its place.

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

/// The registry; none yet is an empty one. One that cannot be read is an error: forgetting it would leave our volumes
/// in the game untracked, and later exports would take them for the game's own files.
pub fn load(game: &Path) -> Result<Registry> {
    let p = reg_path(game);
    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} is damaged — fix or delete it (our volumes in Data are listed in it)", p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(e).with_context(|| format!("read {}", p.display())),
    }
}

fn save(game: &Path, r: &Registry) -> Result<()> {
    std::fs::create_dir_all(root(game))?;
    let p = reg_path(game);
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(r)?)?;
    std::fs::rename(&tmp, &p).with_context(|| format!("write {}", p.display()))?;
    Ok(())
}

/// The volumes other tools of ours keep in the game (RisenLab Starter's), so a re-export does not read them as the
/// game's own files either.
pub fn other_volumes(game: &Path) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(game.join("RisenLab-mods").join("g3state.json")) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else { return vec![] };
    v["volumes"].as_object().map(|m| m.values().filter_map(|x| x.get(0).and_then(|f| f.as_str()).map(str::to_string)).collect()).unwrap_or_default()
}

/// A mod name is a folder of its own under `mods`: no path, no characters Windows forbids.
fn check_name(name: &str) -> Result<()> {
    let bad = name.trim().is_empty() || name == "." || name == ".." || name.ends_with('.') || name.ends_with(' ')
        || name.chars().any(|c| c < ' ' || r#"\/:*?"<>|"#.contains(c));
    if bad { bail!(r#"mod name {name:?}: give it a plain name (no \ / : * ? " < > |)"#); }
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
    if rest.len() < 2 || !rest.chars().all(|c| c.is_ascii_digit()) { return None; }
    rest.parse().ok()
}

fn volume_name(archive: &str, i: u32) -> String { if i < 10 { format!("{archive}.p0{i}") } else { format!("{archive}.p{i}") } }

/// The numbers of the volumes of `archive` in Data.
fn numbers(data: &Path, archive: &str) -> Result<Vec<u32>> {
    let mut v: Vec<u32> = std::fs::read_dir(data).with_context(|| format!("read {}", data.display()))?.flatten()
        .filter_map(|e| volume_index(&e.file_name().to_string_lossy(), archive)).collect();
    v.sort();
    Ok(v)
}

/// Write `bytes` to `path` whole or not at all (a half-written volume would break the game's start).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("g3ie_tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replace {} (is the game or a tool holding it?)", path.display()));
    }
    Ok(())
}

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

/// Rebuild our volume of every archive from the installed mods (a later mod's file wins), keeping the volume numbers
/// without gaps (see the module notes). The registry is saved after each volume.
fn rebuild(game: &Path, reg: &mut Registry) -> Result<()> {
    let data = game.join("Data");
    let mut per: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for m in &reg.mods { for (a, rel, src) in mod_files(game, m) { per.entry(a).or_default().insert(rel.to_lowercase(), src); } }
    for a in ARCHIVES {
        let taken = numbers(&data, a)?;
        let ours = reg.volumes.get(*a).cloned();
        let mine = ours.as_deref().and_then(|v| volume_index(v, a));
        let mut list = vec![];
        for (rel, src) in per.get(*a).into_iter().flatten() {
            let bytes = std::fs::read(src).with_context(|| format!("read {}", src.display()))?;
            let rel = src.strip_prefix(root(game).join("mods")).ok().and_then(|p| p.components().skip(2).map(|c| c.as_os_str().to_string_lossy().into_owned()).reduce(|x, y| format!("{x}/{y}"))).unwrap_or_else(|| rel.clone());
            list.push((rel, bytes));
        }
        if list.is_empty() {
            let Some(v) = ours else { continue };
            if taken.iter().any(|&n| Some(n) > mine) {
                // Another volume above ours: an empty one keeps the numbers after it mounted.
                write_atomic(&data.join(&v), &crate::volume::write(a, &[], crate::volume::filetime_now()))?;
            } else {
                match std::fs::remove_file(data.join(&v)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e).with_context(|| format!("remove {} (is the game or a tool holding it?)", data.join(&v).display())),
                }
                reg.volumes.remove(*a);
            }
        } else {
            // Ours keeps its number; a new one takes the first free number (which also closes a gap left by others).
            let name = match ours { Some(v) => v, None => volume_name(a, (0..).find(|n| !taken.contains(n)).unwrap()) };
            write_atomic(&data.join(&name), &crate::volume::write(a, &list, crate::volume::filetime_now()))?;
            reg.volumes.insert(a.to_string(), name);
        }
        save(game, reg)?;
    }
    Ok(())
}

/// Install `files` (archive, path, bytes) as mod `name` (replacing an installed mod of that name).
pub fn install(game: &Path, name: &str, files: &[(String, String, Vec<u8>)]) -> Result<Vec<String>> {
    check_name(name)?;
    if game_running() { bail!("close Gothic 3 first"); }
    let mut reg = load(game)?;
    let dir = root(game).join("mods").join(name);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("remove the old files of {name} in {}", dir.display())),
    }
    for (a, rel, bytes) in files {
        let p = dir.join(a).join(rel.replace('/', "\\"));
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(&p, bytes)?;
    }
    reg.mods.retain(|m| m != name);
    reg.mods.push(name.to_string());
    rebuild(game, &mut reg)?;
    Ok(reg.volumes.values().cloned().collect())
}

pub fn uninstall(game: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    if game_running() { bail!("close Gothic 3 first"); }
    let mut reg = load(game)?;
    if !reg.mods.iter().any(|m| m == name) { bail!("{name} is not installed by Gothic 3 ImpExp"); }
    reg.mods.retain(|m| m != name);
    let _ = std::fs::remove_dir_all(root(game).join("mods").join(name));
    rebuild(game, &mut reg)
}

/// Installed mods with their files.
pub fn installed(game: &Path) -> Result<Vec<(String, Vec<String>)>> {
    Ok(load(game)?.mods.iter().map(|m| (m.clone(), mod_files(game, m).into_iter().map(|(a, rel, _)| format!("{a}/{rel}")).collect())).collect())
}

/// A package folder: one volume per archive (`<archive>.vol`) and an empty one (`<archive>.empty`), plus INSTALL.bat
/// and ROLLBACK.bat. `installed.txt` notes `<Data folder>|<archive>|<number>` per volume installed.
pub fn package(out: &Path, title: &str, files: &[(String, String, Vec<u8>)]) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut per: BTreeMap<&str, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for (a, rel, bytes) in files { per.entry(a.as_str()).or_default().push((rel.clone(), bytes.clone())); }
    let now = crate::volume::filetime_now();
    for (a, list) in &per {
        std::fs::write(out.join(format!("{a}.vol")), crate::volume::write(a, list, now))?;
        std::fs::write(out.join(format!("{a}.empty")), crate::volume::write(a, &[], now))?;
    }
    let title = title.replace('%', "%%").replace(['\r', '\n', '&', '|', '<', '>', '^', '!'], " ");
    // Batch text: paths stay in quotes ("Program Files (x86)" would end a bracketed block); cmd wants CR LF.
    let busy = r#"tasklist /FI "IMAGENAME eq Gothic3.exe" | "%SystemRoot%\System32\find.exe" /I "Gothic3.exe" >nul && (echo Close Gothic 3 first. & pause & exit /b 1)"#;
    let mut install = r#"@echo off
setlocal EnableDelayedExpansion
rem TITLE - Gothic 3 mod. Usage: INSTALL.bat [Gothic 3 folder]
set "GAME=%~1"
if "%GAME%"=="" set "GAME=C:\Program Files (x86)\Steam\steamapps\common\Gothic 3"
if not exist "%GAME%\Data" (echo Gothic 3 not found at "%GAME%" - run INSTALL.bat "game folder" & pause & exit /b 1)
BUSY
if exist "%~dp0installed.txt" (echo Already installed - run ROLLBACK.bat first. & pause & exit /b 1)
"#.to_string();
    for a in per.keys() {
        // The first free number: the engine stops at a gap.
        install += &r#"set "N="
for /L %%i in (0,1,99) do if not defined N (set "I=%%i" & (if %%i LSS 10 set "I=0%%i") & if not exist "%GAME%\Data\ARCHIVE.p!I!" set "N=!I!")
if not defined N (echo No free volume number for ARCHIVE. & pause & exit /b 1)
copy /Y "%~dp0ARCHIVE.vol" "%GAME%\Data\ARCHIVE.p!N!" >nul || (echo Copy failed. & pause & exit /b 1)
>>"%~dp0installed.txt" echo %GAME%\Data^|ARCHIVE^|!N!
"#.replace("ARCHIVE", a);
    }
    install += "echo TITLE installed.\npause\n";
    // A volume above ours (another mod installed since) would stop mounting if ours left a gap: then ours becomes empty.
    let rollback = r#"@echo off
setlocal EnableDelayedExpansion
rem TITLE - remove what INSTALL.bat added.
set "HERE=%~dp0"
if not exist "%HERE%installed.txt" (echo Nothing to remove. & pause & exit /b 0)
BUSY
for /F "usebackq tokens=1-3 delims=|" %%a in ("%HERE%installed.txt") do call :remove "%%a" "%%b" "%%c"
del "%HERE%installed.txt"
echo TITLE removed.
pause
exit /b 0

:remove
set /a K=1%~3-100+1
set "UP="
for /L %%i in (%K%,1,99) do (set "J=%%i" & (if %%i LSS 10 set "J=0%%i") & if exist "%~1\%~2.p!J!" set "UP=1")
if defined UP (copy /Y "%HERE%%~2.empty" "%~1\%~2.p%~3" >nul) else (del "%~1\%~2.p%~3" 2>nul)
exit /b 0
"#;
    let bat = |s: &str| s.replace("BUSY", busy).replace("TITLE", &title).replace('\n', "\r\n");
    std::fs::write(out.join("INSTALL.bat"), bat(&install))?;
    std::fs::write(out.join("ROLLBACK.bat"), bat(rollback))?;
    std::fs::write(out.join("README.txt"), format!("{title}\r\n\r\nGothic 3 mod made with Gothic 3 ImpExp.\r\nInstall: run INSTALL.bat (or INSTALL.bat \"<Gothic 3 folder>\"). Remove: ROLLBACK.bat.\r\nEach .vol becomes the first free patch volume (.pNN) of its archive in Gothic 3\\Data; the game's archives are not changed.\r\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_game() -> PathBuf {
        // Spaces and brackets, as in "Program Files (x86)".
        let g = std::env::temp_dir().join(format!("g3 mods (x86) {}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
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
        let reg = load(&g).unwrap();
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
        let out = run(&pkg.join("INSTALL.bat"), Some(&g));
        assert!(g.join("Data").join("_compiledImage.p02").exists(), "{out}");
        run(&pkg.join("ROLLBACK.bat"), None);
        assert!(!g.join("Data").join("_compiledImage.p02").exists());
        assert!(g.join("Data").join("_compiledImage.p01").exists());
        std::fs::remove_dir_all(&g).ok();
    }

    fn numbers_of(g: &Path, a: &str) -> Vec<u32> { numbers(&g.join("Data"), a).unwrap() }
    fn files_in(g: &Path, name: &str) -> usize { crate::g3::read_volume(&g.join("Data").join(name)).unwrap().len() }
    fn run(bat: &Path, arg: Option<&Path>) -> String {
        // As a double click or a console runs it: cmd /S /C ""bat" "arg"" (quoted paths with spaces and brackets).
        use std::os::windows::process::CommandExt;
        let mut line = format!("\"{}\"", bat.display());
        if let Some(a) = arg { line += &format!(" \"{}\"", a.display()); }
        let mut c = std::process::Command::new("cmd");
        c.raw_arg(format!("/S /C \"{line}\""));
        let o = c.stdin(std::process::Stdio::null()).output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    }

    /// The engine stops at the first missing volume number: whatever comes and goes, ours and a package's, the
    /// numbers never get a gap.
    #[test]
    fn volume_numbers_never_get_a_gap() {
        let g = fake_game();
        let a = |s: &str| ("_compiledMesh".to_string(), format!("x/{s}.xcmsh"), s.as_bytes().to_vec());
        install(&g, "A", &[a("A")]).unwrap();
        assert_eq!(numbers_of(&g, "_compiledMesh"), [0]);
        // A package goes on top of ours.
        let pkg = g.join("pkg");
        package(&pkg, "P", &[a("P")]).unwrap();
        let out = run(&pkg.join("INSTALL.bat"), Some(&g));
        assert_eq!(numbers_of(&g, "_compiledMesh"), [0, 1], "{out}");
        // Ours emptied below it stays, empty.
        uninstall(&g, "A").unwrap();
        assert_eq!(numbers_of(&g, "_compiledMesh"), [0, 1]);
        assert_eq!(files_in(&g, "_compiledMesh.p00"), 0);
        // A new mod of ours takes its old number again.
        install(&g, "B", &[a("B")]).unwrap();
        assert_eq!(load(&g).unwrap().volumes["_compiledMesh"], "_compiledMesh.p00");
        assert_eq!(files_in(&g, "_compiledMesh.p00"), 1);
        // The package leaves; ours is the last and goes when emptied.
        let out = run(&pkg.join("ROLLBACK.bat"), None);
        assert_eq!(numbers_of(&g, "_compiledMesh"), [0], "{out}");
        uninstall(&g, "B").unwrap();
        assert!(numbers_of(&g, "_compiledMesh").is_empty());
        assert!(load(&g).unwrap().volumes.is_empty());
        std::fs::remove_dir_all(&g).ok();
    }

    /// A package removed from under a later one leaves an empty volume, so the later one still mounts.
    #[test]
    fn a_package_removed_from_below_leaves_an_empty_volume() {
        let g = fake_game();
        let (p1, p2) = (g.join("one"), g.join("two"));
        package(&p1, "One", &[("_compiledImage".to_string(), "x/1.ximg".to_string(), b"1".to_vec())]).unwrap();
        package(&p2, "Two", &[("_compiledImage".to_string(), "x/2.ximg".to_string(), b"2".to_vec())]).unwrap();
        run(&p1.join("INSTALL.bat"), Some(&g));
        let out = run(&p2.join("INSTALL.bat"), Some(&g));
        assert_eq!(numbers_of(&g, "_compiledImage"), [0, 1, 2, 3], "{out}");
        let out = run(&p1.join("ROLLBACK.bat"), None);
        assert_eq!(numbers_of(&g, "_compiledImage"), [0, 1, 2, 3], "{out}");
        assert_eq!(files_in(&g, "_compiledImage.p02"), 0);
        assert_eq!(files_in(&g, "_compiledImage.p03"), 1);
        run(&p2.join("ROLLBACK.bat"), None);
        assert_eq!(numbers_of(&g, "_compiledImage"), [0, 1, 2]);
        std::fs::remove_dir_all(&g).ok();
    }

    /// The installer finds the game in the default Steam folder form ("Program Files (x86)") — brackets in the path.
    #[test]
    fn the_installer_takes_a_path_with_brackets() {
        let g = fake_game();
        assert!(g.to_string_lossy().contains("(x86)"));
        let pkg = g.join("pkg (1)");
        package(&pkg, "Bracket (test) mod", &[("Lightmaps".to_string(), "x/L.xlmp".to_string(), b"l".to_vec())]).unwrap();
        let out = run(&pkg.join("INSTALL.bat"), Some(&g));
        assert_eq!(numbers_of(&g, "Lightmaps"), [0], "{out}");
        std::fs::remove_dir_all(&g).ok();
    }

    #[test]
    fn a_damaged_registry_stops_instead_of_forgetting_our_volumes() {
        let g = fake_game();
        std::fs::create_dir_all(root(&g)).unwrap();
        std::fs::write(reg_path(&g), "{ not json").unwrap();
        assert!(install(&g, "A", &[("_compiledMesh".to_string(), "x/A.xcmsh".to_string(), b"a".to_vec())]).is_err());
        assert!(crate::g3::G3Ctx::open_original(&g).is_err());
        std::fs::remove_dir_all(&g).ok();
    }

    #[test]
    fn a_mod_name_is_never_a_path() {
        let g = fake_game();
        std::fs::create_dir_all(root(&g).join("mods").join("Keep")).unwrap();
        for bad in [".", "..", "a/b", r"a\b","", " ", "x:"] { assert!(install(&g, bad, &[]).is_err(), "{bad:?}"); assert!(uninstall(&g, bad).is_err()); }
        assert!(root(&g).join("mods").join("Keep").exists());
        install(&g, "Млин 2", &[("_compiledMesh".to_string(), "x/A.xcmsh".to_string(), b"a".to_vec())]).unwrap();
        std::fs::remove_dir_all(&g).ok();
    }
}
