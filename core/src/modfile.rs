//! A mod as one file (`.g3mod`): an ordinary ZIP a player can look into, which a mod launcher puts into the game as
//! one patch volume per archive and takes out again.
//!
//! ```text
//! mod.json                                  {"format": 1, "game": "gothic3", "name", "author", "version",
//!                                            "description", "files": ["<Archive>/<path>", …]}
//! files/_compiledMesh/<path>.xcmsh          every file under its archive's name and its path inside the archive
//! files/_compiledPhysic/<path>.xnvmsh
//! ```
//!
//! ZIP as written here: deflate, UTF-8 names (flag 0x0800), a fixed date so the same mod packs to the same bytes,
//! no zip64 (a mod stays far below 4 GB).
//! ```text
//! local header  PK\3\4 · ver 20 · flags · method 8 · time · date · crc · sizes · name length · extra 0 · name · data
//! central dir   PK\1\2 · ver 20 · ver 20 · the same fields · comment/disk/attributes 0 · local header offset · name
//! end           PK\5\6 · disk 0 · 0 · entries · entries · central dir size · offset · comment 0
//! ```

use anyhow::{bail, Context, Result};
use flate2::{write::DeflateEncoder, Compression, Crc};
use std::io::Write;
use std::path::Path;

pub const EXTENSION: &str = "g3mod";

#[derive(serde::Serialize)]
struct Manifest<'a> { format: u32, game: &'a str, name: &'a str, author: &'a str, version: &'a str, description: &'a str, files: Vec<String> }

/// One ZIP of `entries` (name with forward slashes, bytes), in the order given.
pub fn zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    const DOS_DATE: u16 = (1 << 5) | 1; // 1980-01-01
    let (mut out, mut central) = (vec![], vec![]);
    for (name, bytes) in entries {
        let mut crc = Crc::new();
        crc.update(bytes);
        let mut enc = DeflateEncoder::new(vec![], Compression::default());
        enc.write_all(bytes)?;
        let packed = enc.finish()?;
        if bytes.len() > u32::MAX as usize || out.len() > u32::MAX as usize { bail!("{name}: too big for a mod file"); }
        let head = |sig: u32| -> Vec<u8> {
            let mut h = sig.to_le_bytes().to_vec();
            if sig == 0x0201_4b50 { h.extend(20u16.to_le_bytes()); }
            for v in [20u16, 0x0800, 8, 0, DOS_DATE] { h.extend(v.to_le_bytes()); }
            for v in [crc.sum(), packed.len() as u32, bytes.len() as u32] { h.extend(v.to_le_bytes()); }
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h
        };
        let mut c = head(0x0201_4b50);
        c.extend([0u8; 10]); // comment length, disk, internal attributes (2 bytes each), external attributes (4)
        c.extend((out.len() as u32).to_le_bytes());
        c.extend(name.as_bytes());
        central.extend(c);
        out.extend(head(0x0403_4b50));
        out.extend(name.as_bytes());
        out.extend(packed);
    }
    let (at, size) = (out.len() as u32, central.len() as u32);
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, entries.len() as u16, entries.len() as u16] { out.extend(v.to_le_bytes()); }
    for v in [size, at] { out.extend(v.to_le_bytes()); }
    out.extend(0u16.to_le_bytes());
    Ok(out)
}

/// Write `files` (archive, path inside it, bytes) as the mod file `path`. A later file of the same path wins.
pub fn write(path: &Path, name: &str, author: &str, files: &[(String, String, Vec<u8>)]) -> Result<()> {
    if name.trim().is_empty() { bail!("a mod needs a name"); }
    let mut entries: Vec<(String, Vec<u8>)> = vec![];
    for (a, rel, bytes) in files {
        let key = format!("{a}/{}", rel.replace('\\', "/"));
        match entries.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&key)) { Some(e) => e.1 = bytes.clone(), None => entries.push((key, bytes.clone())) }
    }
    let m = Manifest { format: 1, game: "gothic3", name: name.trim(), author: author.trim(), version: "", description: "", files: entries.iter().map(|(k, _)| k.clone()).collect() };
    let mut all = vec![("mod.json".to_string(), serde_json::to_vec_pretty(&m)?)];
    all.extend(entries.into_iter().map(|(k, b)| (format!("files/{k}"), b)));
    if let Some(p) = path.parent() { std::fs::create_dir_all(p).with_context(|| format!("create {}", p.display()))?; }
    let tmp = path.with_extension("g3mod_tmp");
    std::fs::write(&tmp, zip(&all)?).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

/// A file name for mod `name`: what Windows allows, the rest as it is.
pub fn file_name(name: &str) -> String {
    let clean: String = name.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim_matches(['.', ' ']);
    format!("{}.{EXTENSION}", if clean.is_empty() { "mod" } else { clean })
}

#[cfg(test)]
mod tests {
    /// The mod file is a ZIP Windows and any unzip read: the entries come back with their bytes, the manifest lists
    /// the files under their archives.
    #[test]
    fn a_mod_file_is_an_ordinary_zip() {
        let dir = std::env::temp_dir().join(format!("g3ie_modfile_{}", std::process::id()));
        let p = dir.join(super::file_name("Млин: новий?"));
        assert!(p.file_name().unwrap().to_string_lossy() == "Млин_ новий_.g3mod");
        let files = vec![("_compiledMesh".to_string(), "gothic3_impexp/A.xcmsh".to_string(), vec![7u8; 3000]), ("_compiledPhysic".to_string(), "A_COL.xnvmsh".to_string(), b"col".to_vec())];
        super::write(&p, "Млин", "UniLinkIF", &files).unwrap();
        // Read it back with PowerShell's own ZIP (the one Explorer uses).
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &format!("Add-Type -A System.IO.Compression.FileSystem; $z=[IO.Compression.ZipFile]::OpenRead('{}'); $z.Entries | % {{ \"$($_.FullName)|$($_.Length)\" }}; $r=New-Object IO.StreamReader($z.GetEntry('mod.json').Open()); $r.ReadToEnd(); $z.Dispose()", p.display())]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("files/_compiledMesh/gothic3_impexp/A.xcmsh|3000"), "{text}");
        assert!(text.contains("files/_compiledPhysic/A_COL.xnvmsh|3"), "{text}");
        assert!(text.contains("\"game\": \"gothic3\"") && text.contains("_compiledMesh/gothic3_impexp/A.xcmsh"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
