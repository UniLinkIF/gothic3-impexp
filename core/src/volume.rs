//! Writing Gothic 3 archive volumes (`<archive>.pNN`), the way the game's own patch volumes are laid out, so the
//! engine mounts them over the base `.pak` ("Mount incremental pack file … for folder …", `%s.p0%i` / `%s.p%i`).
//!
//! ```text
//! header 48 bytes  u32 0 · "G3V0" · u32 0 · u32 0 · u32 1 · u32 0x100067f8 (as the game's volumes)
//!                  · u64 tree offset · u64 tree offset · u64 end of tree        file data from 0x30 (zlib each)
//! node             FILETIME created, accessed, modified · u64 0 · u32 attributes
//!   directory      attributes 0x20010 · u32 length + name + NUL ("" for the root, "dir/" below it)
//!                  · u32 subdirectories + them · u32 files + them
//!   file           attributes 0x20820 · u64 data offset · u64 stored size · u32 size · u32 0 · u32 0
//!                  · u32 compression (2 zlib) · name (path in the archive) · full path (the source file)
//! tail             u32 0 after the tree (the header's end offset points at it)
//! ```

use flate2::{write::ZlibEncoder, Compression};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Default)]
struct Dir { dirs: BTreeMap<String, Dir>, files: Vec<(String, String, u64, u64, u32)> }

fn put_name(o: &mut Vec<u8>, s: &str) {
    if s.is_empty() { o.extend(0u32.to_le_bytes()); return; }
    o.extend((s.len() as u32).to_le_bytes());
    o.extend(s.as_bytes());
    o.push(0);
}

fn node_head(o: &mut Vec<u8>, time: u64, attributes: u32) {
    for _ in 0..3 { o.extend(time.to_le_bytes()); }
    o.extend(0u64.to_le_bytes());
    o.extend(attributes.to_le_bytes());
}

fn write_dir(o: &mut Vec<u8>, name: &str, d: &Dir, time: u64) {
    node_head(o, time, 0x20010);
    put_name(o, name);
    o.extend((d.dirs.len() as u32).to_le_bytes());
    for (sub, child) in &d.dirs { write_dir(o, &format!("{name}{sub}/"), child, time); }
    o.extend((d.files.len() as u32).to_le_bytes());
    for (path, full, offset, stored, size) in &d.files {
        node_head(o, time, 0x20820);
        o.extend(offset.to_le_bytes());
        o.extend(stored.to_le_bytes());
        o.extend(size.to_le_bytes());
        o.extend(0u32.to_le_bytes());
        o.extend(0u32.to_le_bytes());
        o.extend(2u32.to_le_bytes());
        put_name(o, path);
        put_name(o, full);
    }
}

/// A volume of `files` (path inside the archive with `/`, bytes); `archive` names the archive in the full paths.
pub fn write(archive: &str, files: &[(String, Vec<u8>)], time: u64) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend(0u32.to_le_bytes());
    o.extend(b"G3V0");
    for x in [0u32, 0, 1, 0x100067f8] { o.extend(x.to_le_bytes()); }
    o.resize(0x30, 0);
    let mut root = Dir::default();
    for (path, data) in files {
        let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
        z.write_all(data).unwrap();
        let packed = z.finish().unwrap();
        let offset = o.len() as u64;
        o.extend(&packed);
        let parts: Vec<&str> = path.split('/').collect();
        let mut d = &mut root;
        for p in &parts[..parts.len() - 1] { d = d.dirs.entry(p.to_string()).or_default(); }
        d.files.push((path.clone(), format!("{archive}\\{}", path.replace('/', "\\")), offset, packed.len() as u64, data.len() as u32));
    }
    let tree = o.len() as u64;
    write_dir(&mut o, "", &root, time);
    let end = o.len() as u64;
    o.extend(0u32.to_le_bytes());
    o[0x18..0x20].copy_from_slice(&tree.to_le_bytes());
    o[0x20..0x28].copy_from_slice(&tree.to_le_bytes());
    o[0x28..0x30].copy_from_slice(&end.to_le_bytes());
    o
}

/// FILETIME (100 ns since 1601) of now.
pub fn filetime_now() -> u64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() / 100).unwrap_or(0) as u64;
    unix + 116_444_736_000_000_000
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_written_volume_reads_back() {
        let files = vec![("a.txt".to_string(), b"hello".to_vec()), ("sub/deep/b.bin".to_string(), vec![7u8; 5000]), ("sub/c.bin".to_string(), vec![])];
        let v = super::write("_compiledMesh", &files, super::filetime_now());
        let dir = std::env::temp_dir().join(format!("g3vol-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("_compiledMesh.p00");
        std::fs::write(&p, &v).unwrap();
        let entries = crate::g3::read_volume(&p).unwrap();
        let mut paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
        paths.sort();
        assert_eq!(paths, ["a.txt", "sub/c.bin", "sub/deep/b.bin"]);
        for e in &entries {
            let raw = &v[e.offset as usize..(e.offset + e.stored) as usize];
            let mut out = vec![];
            std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(raw), &mut out).unwrap();
            let want = &files.iter().find(|f| f.0 == e.path).unwrap().1;
            assert_eq!(&out, want, "{}", e.path);
            assert_eq!(e.size as usize, want.len());
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
