//! Gothic 3 archives: read-only access to a Gothic 3 install (`Data*.pak` and its patch volumes).
//!
//! Gothic 3 archives carry the same `G3V0` product tag as Risen 1/2, but the volume is an older layout that
//! Risen archive readers cannot read:
//! - header: version u32 **0** (Risen: 1), "G3V0", revision, encryption, compression, reserved, then
//!   u64 data offset, u64 root offset (both point at the directory tree), u64 volume size. Data starts at 0x30.
//! - every node: FILETIME created/accessed/modified, u64 (0), u32 attributes (0x10 dir, 0x8000 deleted, 0x800 packed).
//! - directory: name (u32 len + bytes + NUL, path relative to the volume, `dir/`), u32 subdir count + subdirs,
//!   u32 file count + files.
//! - file: u64 data offset, u64 stored size, u32 file size, u32 (0), u32 encryption, u32 compression (2 = zlib),
//!   name (relative path), full path on the developer's disk (`E:\Gothic III Work\bin\Data\...`).
//! - `.p00`, `.p01` are later volumes of the same archive (patch 1.75 data); later volumes win, deleted
//!   entries remove earlier ones.
use anyhow::{anyhow, bail, Context, Result};
use flate2::read::ZlibDecoder;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const DIR_ATTR: u32 = 0x10;
const DELETED_ATTR: u32 = 0x8000;

#[derive(Debug, Clone)]
pub struct G3Entry { pub path: String, pub offset: u64, pub stored: u64, pub size: u32, pub compression: u32, pub attributes: u32 }

impl G3Entry { pub fn is_deleted(&self) -> bool { self.attributes & DELETED_ATTR != 0 } }

fn u32_<R: Read>(r: &mut R) -> std::io::Result<u32> { let mut b = [0u8; 4]; r.read_exact(&mut b)?; Ok(u32::from_le_bytes(b)) }
fn u64_<R: Read>(r: &mut R) -> std::io::Result<u64> { let mut b = [0u8; 8]; r.read_exact(&mut b)?; Ok(u64::from_le_bytes(b)) }
fn name_<R: Read>(r: &mut R) -> Result<String> {
    let n = u32_(r)? as usize;
    if n == 0 { return Ok(String::new()); }
    if n > 4096 { bail!("name length {n}: not a Gothic 3 directory tree"); }
    let mut b = vec![0u8; n + 1];
    r.read_exact(&mut b)?;
    b.pop();
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// Every file entry of one Gothic 3 volume (deleted markers included).
pub fn read_volume(path: &Path) -> Result<Vec<G3Entry>> {
    let mut f = BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?);
    let version = u32_(&mut f)?;
    let mut tag = [0u8; 4];
    f.read_exact(&mut tag)?;
    if &tag != b"G3V0" || version != 0 { bail!("{} is not a Gothic 3 volume (version {version}, tag {:?})", path.display(), tag); }
    f.seek(SeekFrom::Start(0x20))?;
    let root = u64_(&mut f)?;
    f.seek(SeekFrom::Start(root))?;
    let mut out = vec![];
    node(&mut f, &mut out, 0)?;
    Ok(out)
}

fn node<R: Read>(f: &mut R, out: &mut Vec<G3Entry>, depth: u32) -> Result<()> {
    if depth > 64 { bail!("directory tree too deep"); }
    let mut times = [0u8; 32];
    f.read_exact(&mut times)?;
    let attributes = u32_(f)?;
    if attributes & DIR_ATTR != 0 {
        let _name = name_(f)?;
        for _ in 0..u32_(f)? { node(f, out, depth + 1)?; }
        for _ in 0..u32_(f)? { node(f, out, depth + 1)?; }
    } else {
        let offset = u64_(f)?;
        let stored = u64_(f)?;
        let size = u32_(f)?;
        let _ = u32_(f)?;
        let _encryption = u32_(f)?;
        let compression = u32_(f)?;
        let path = name_(f)?;
        let _full = name_(f)?;
        out.push(G3Entry { path, offset, stored, size, compression, attributes });
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Located { archive: PathBuf, entry: G3Entry }

pub struct G3Ctx {
    pub root: PathBuf,
    index: HashMap<String, Located>,
    pub per_archive: BTreeMap<String, usize>,
    pub overrides: usize,
    open: RefCell<HashMap<PathBuf, BufReader<File>>>,
}

/// Archives we never need for props (voice, music, sound, video).
fn skipped(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("speech_") || n.starts_with("music") || n.starts_with("sound")
}

impl G3Ctx {
    /// `root` = the Gothic 3 install folder (the one with `Data\`).
    pub fn open(root: &Path) -> Result<Self> {
        let dir = root.join("Data");
        let mut archives = vec![];
        for e in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))?.flatten() {
            let p = e.path();
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            let volume = ext == "pak" || (ext.len() == 3 && ext.starts_with('p') && ext[1..].chars().all(|c| c.is_ascii_digit()));
            if volume && p.is_file() && !skipped(&name) { archives.push(p); }
        }
        let rank = |p: &PathBuf| { let e = p.extension().unwrap().to_string_lossy().to_lowercase(); if e == "pak" { -1 } else { e[1..].parse::<i32>().unwrap_or(0) } };
        archives.sort_by_key(|p| (p.with_extension("").to_string_lossy().to_lowercase(), rank(p)));
        let mut index: HashMap<String, Located> = HashMap::new();
        let mut per_archive = BTreeMap::new();
        let mut overrides = 0;
        for a in archives {
            let files = read_volume(&a)?;
            per_archive.insert(a.file_name().unwrap().to_string_lossy().to_string(), files.len());
            // Volumes of one archive share a namespace; different archives (_compiledMesh, Templates) never collide in practice.
            let group = a.file_stem().unwrap().to_string_lossy().to_lowercase();
            for e in files {
                let k = format!("{group}/{}", e.path.to_lowercase());
                if index.contains_key(&k) { overrides += 1; }
                if e.is_deleted() { index.remove(&k); continue; }
                index.insert(k, Located { archive: a.clone(), entry: e });
            }
        }
        Ok(G3Ctx { root: root.to_path_buf(), index, per_archive, overrides, open: RefCell::new(HashMap::new()) })
    }

    /// Keys are `<archive stem>/<path>` lowercase, e.g. `_compiledmesh/obj_barrel_01.xcmsh`.
    pub fn keys(&self) -> Vec<String> { let mut v: Vec<String> = self.index.keys().cloned().collect(); v.sort(); v }

    /// The entry path with its original case (`g3_objects_myrtana_misc_01/G3_Object_Barrel_01.xcmsh`).
    pub fn path_of(&self, key: &str) -> Option<&str> { self.index.get(&key.to_lowercase()).map(|l| l.entry.path.as_str()) }

    pub fn size_of(&self, key: &str) -> Option<u32> { self.index.get(&key.to_lowercase()).map(|l| l.entry.size) }

    /// Find an entry by bare file name (case-insensitive) inside one archive group, e.g. (`_compiledmesh`, `Obj_Barrel.xcmsh`).
    pub fn find(&self, group: &str, file: &str) -> Option<String> {
        let k = format!("{}/{}", group.to_lowercase(), file.to_lowercase());
        if self.index.contains_key(&k) { return Some(k); }
        let tail = format!("/{}", file.to_lowercase());
        let pre = format!("{}/", group.to_lowercase());
        self.index.keys().find(|x| x.starts_with(&pre) && x.ends_with(&tail)).cloned()
    }

    pub fn read(&self, key: &str) -> Result<Vec<u8>> {
        let l = self.index.get(&key.to_lowercase()).ok_or_else(|| anyhow!("{key} is not in a Gothic 3 archive"))?;
        let mut open = self.open.borrow_mut();
        if !open.contains_key(&l.archive) { open.insert(l.archive.clone(), BufReader::new(File::open(&l.archive)?)); }
        let f = open.get_mut(&l.archive).unwrap();
        f.seek(SeekFrom::Start(l.entry.offset))?;
        let mut raw = vec![0u8; l.entry.stored as usize];
        f.read_exact(&mut raw)?;
        if l.entry.compression == 0 { return Ok(raw); }
        let mut out = Vec::with_capacity(l.entry.size as usize);
        ZlibDecoder::new(&raw[..]).read_to_end(&mut out).with_context(|| format!("inflate {key}"))?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn g3() -> Option<G3Ctx> { std::env::var_os("G3_GAME").map(|r| G3Ctx::open(Path::new(&r)).expect("G3_GAME is a Gothic 3 install")) }

    #[test]
    fn every_gothic3_volume_opens_and_entries_inflate_to_their_size() {
        let Some(g) = g3() else { return };
        assert!(g.per_archive.len() > 20, "{:?}", g.per_archive);
        for group in ["_compiledmesh/", "_compiledmaterial/", "_compiledimage/", "_compiledphysic/", "templates/"] {
            let k = g.keys().into_iter().find(|k| k.starts_with(group)).unwrap_or_else(|| panic!("no {group}"));
            assert_eq!(g.read(&k).unwrap().len() as u32, g.size_of(&k).unwrap(), "{k}");
        }
    }
}
