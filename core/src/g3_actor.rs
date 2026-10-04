//! Gothic 3 actors (`_compiledAnimation/*.xact`): creatures, NPC bodies and heads.
//!
//! A `GENOMFLE` resource whose payload is an old EMotionFX actor (`FXA `, v1.1; Risen 1 has EMotionFX 3.6
//! `XAC `). Layout (checked on every actor of the install):
//!
//! ```text
//! u32 FXA bytes · "FXA " u8 hi lo (at 78 in body actors) · sections to the end:
//!   u32 id · u32 size (body = size bytes after the version) · u32 version · body
//!   0  node:     f32 pos[3] · quat[4] (x y z w) · 40 bytes (scale, scale rotation) · u32 len name · u32 len parent
//!   3  mesh:     u32 node · u32 vertices · u32 corners · u32 indices · u32 parts · u32 uv channels · u32
//!                part: u8 material, 3 · u32 indices · u32 corners ·
//!                      corner: u32 vertex · f32 pos[3] · f32 normal[3] · f32 uv[2] (· 8 bytes per extra channel)
//!                      u32 index[3 * triangles] (into the part's corners)
//!   4  skin:     u32 node · per vertex: u8 n · n × (u16 node, u16, f32 weight)
//!   6  material: 68 bytes · u32 len name           7  texture map: u8 type (1 diffuse, 3 normal, 9 specular) · 27 · u32 len name
//! ```
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone)]
pub struct G3Node { pub name: String, pub parent: Option<usize>, pub pos: [f32; 3], pub rot: [f32; 4] }

#[derive(Debug, Clone, Default)]
pub struct G3Material { pub name: String, pub diffuse: Option<String>, pub normal: Option<String>, pub specular: Option<String> }

/// One skinned mesh: corners (position, normal, uv, source vertex) and triangles with a material slot.
#[derive(Debug, Clone, Default)]
pub struct G3SkinMesh {
    pub node: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// The original (welded) vertex of each corner: the skin weights are per original vertex.
    pub vertex: Vec<u32>,
    pub triangles: Vec<[u32; 3]>,
    pub tri_material: Vec<u32>,
    /// Per original vertex: (node, weight).
    pub weights: Vec<Vec<(u16, f32)>>,
    /// Set when the mesh came from another actor (a head put on a body): that actor's name.
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct G3Actor { pub nodes: Vec<G3Node>, pub meshes: Vec<G3SkinMesh>, pub materials: Vec<G3Material> }

struct R<'a> { d: &'a [u8], p: usize }
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> { let s = self.d.get(self.p..self.p + n).with_context(|| format!("actor runs short at 0x{:x}", self.p))?; self.p += n; Ok(s) }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn f32(&mut self) -> Result<f32> { Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    fn v3(&mut self) -> Result<[f32; 3]> { Ok([self.f32()?, self.f32()?, self.f32()?]) }
    fn s(&mut self) -> Result<String> { let n = self.u32()? as usize; if n > 4096 { bail!("name length {n}"); } Ok(String::from_utf8_lossy(self.take(n)?).into_owned()) }
}

pub fn decode_xact(d: &[u8]) -> Result<G3Actor> {
    // At 78 in body actors; skeleton-only actors have a shorter GENOMFLE head.
    let at = d.windows(4).take(512).position(|w| w == b"FXA ").filter(|&p| p >= 4).context("no FXA actor")?;
    let end = (u32::from_le_bytes(d[at - 4..at].try_into().unwrap()) as usize + at).min(d.len());
    let mut a = G3Actor::default();
    let mut parents: Vec<String> = vec![];
    let mut next = at + 6;
    while next + 12 <= end {
        let mut r = R { d: &d[..end], p: next };
        let id = r.u32()?;
        let size = r.u32()? as usize;
        r.u32()?;
        next += size + 12;
        match id {
            0 => {
                let pos = r.v3()?;
                let rot = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                r.take(40)?;
                let name = r.s()?;
                parents.push(r.s()?);
                a.nodes.push(G3Node { name, parent: None, pos, rot });
            }
            3 => {
                let node = r.u32()? as usize;
                let nv = r.u32()? as usize;
                let _corners = r.u32()?;
                let _indices = r.u32()?;
                let parts = r.u32()?;
                let channels = r.u32()? as usize;
                r.u32()?;
                let mut m = G3SkinMesh { node, weights: vec![vec![]; nv], ..Default::default() };
                for _ in 0..parts {
                    let mat = r.u8()? as u32;
                    r.take(3)?;
                    let ni = r.u32()? as usize;
                    let nc = r.u32()? as usize;
                    let base = m.positions.len() as u32;
                    for _ in 0..nc {
                        let v = r.u32()?;
                        if v as usize >= nv { bail!("corner vertex {v} >= {nv}"); }
                        m.vertex.push(v);
                        m.positions.push(r.v3()?);
                        m.normals.push(r.v3()?);
                        if channels > 0 { m.uvs.push([r.f32()?, r.f32()?]); r.take((channels - 1) * 8)?; }
                    }
                    if ni % 3 != 0 { bail!("{ni} indices"); }
                    for _ in 0..ni / 3 {
                        let t = [r.u32()?, r.u32()?, r.u32()?];
                        if t.iter().any(|&i| i as usize >= nc) { bail!("index past the part's {nc} corners"); }
                        m.triangles.push(t.map(|i| i + base));
                        m.tri_material.push(mat);
                    }
                }
                a.meshes.push(m);
            }
            4 => {
                let node = r.u32()? as usize;
                let Some(m) = a.meshes.iter_mut().rev().find(|m| m.node == node) else { bail!("skin for node {node} before its mesh") };
                for v in 0..m.weights.len() {
                    let n = r.u8()?;
                    for _ in 0..n { let b = r.u16()?; r.u16()?; let w = r.f32()?; m.weights[v].push((b, w)); }
                }
            }
            6 => { r.take(68)?; a.materials.push(G3Material { name: r.s()?, ..Default::default() }); }
            7 => {
                let ty = r.u8()?;
                r.take(27)?;
                let tex = r.s()?;
                if let Some(m) = a.materials.last_mut() {
                    match ty { 1 => m.diffuse = Some(tex), 3 => m.normal = Some(tex), 9 => m.specular = Some(tex), _ => {} }
                }
            }
            _ => {}
        }
    }
    for (i, p) in parents.iter().enumerate() { a.nodes[i].parent = a.nodes.iter().position(|n| !p.is_empty() && &n.name == p); }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gothic3_actor_decodes_and_its_skin_covers_every_vertex() {
        let Some(root) = std::env::var_os("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open(std::path::Path::new(&root)).unwrap();
        let (mut ok, mut fail, mut skinned, mut tris) = (0usize, vec![], 0usize, 0usize);
        for k in g.keys().into_iter().filter(|k| k.ends_with(".xact")) {
            match decode_xact(&g.read(&k).unwrap()) {
                Ok(a) => {
                    ok += 1;
                    for m in &a.meshes {
                        tris += m.triangles.len();
                        if m.weights.iter().any(|w| !w.is_empty()) { skinned += 1; assert!(m.weights.iter().all(|w| w.iter().all(|&(b, _)| (b as usize) < a.nodes.len())), "{k}"); }
                    }
                }
                Err(e) => fail.push(format!("{k}: {e}")),
            }
        }
        eprintln!("actors {ok} ok, {} failed, {skinned} skinned meshes, {tris} triangles; {:?}", fail.len(), fail.iter().take(5).collect::<Vec<_>>());
        assert!(fail.is_empty());
        let wolf = decode_xact(&g.read("_compiledanimation/g3_wolf_body_01.xact").unwrap()).unwrap();
        assert!(wolf.nodes.len() > 20 && !wolf.meshes.is_empty() && wolf.materials.iter().any(|m| m.diffuse.is_some()), "{:?}", wolf.materials);
    }
}
