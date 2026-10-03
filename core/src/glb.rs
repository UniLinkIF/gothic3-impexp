//! A Gothic 3 actor (`.xact`) and its motions (`.xmot`) as binary glTF for Blender's glTF importer.
//!
//! Axes: Gothic 3 is left-handed, Y up, centimetres; glTF is right-handed, Y up, metres. Positions go as
//! (x, y, -z) / 100 and quaternions as (-x, -y, z, w) — the same mirror. Both games' quaternions are plain
//! Hamilton ones (`world = parent * local`), so nothing else changes.
//!
//! Motions: a track's transform is relative to the nearest ancestor that has a track too. The helper bones in
//! between (`*_ROOT`, `*_END`, never in a motion) are folded into it, so while a clip plays they are held at the
//! identity; at rest they keep their bind transforms.

use crate::g3_actor::G3Actor;
use crate::g3_motion::G3Motion;
use anyhow::{ensure, Result};
use serde_json::{json, Value};

const UNIT: f32 = 0.01;

pub fn conv_p(p: [f32; 3], s: f32) -> [f32; 3] { [p[0] * s, p[1] * s, -p[2] * s] }
pub fn conv_q(q: [f32; 4]) -> [f32; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let n = if n > 1e-8 { n } else { 1.0 };
    [-q[0] / n, -q[1] / n, q[2] / n, q[3] / n]
}

type M4 = [f32; 16];
fn trs(t: [f32; 3], q: [f32; 4]) -> M4 {
    let [x, y, z, w] = q;
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [
        1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy), 0.0,
        2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx), 0.0,
        2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy), 0.0,
        t[0], t[1], t[2], 1.0,
    ]
}
fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 { for rr in 0..4 { r[c * 4 + rr] = (0..4).map(|k| a[k * 4 + rr] * b[c * 4 + k]).sum(); } }
    r
}
fn rigid_inverse(m: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..3 { for rr in 0..3 { r[c * 4 + rr] = m[rr * 4 + c]; } }
    for rr in 0..3 { r[12 + rr] = -(0..3).map(|k| r[k * 4 + rr] * m[12 + k]).sum::<f32>(); }
    r[15] = 1.0;
    r
}

struct Bin { data: Vec<u8>, views: Vec<Value>, accessors: Vec<Value> }
impl Bin {
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while self.data.len() % 4 != 0 { self.data.push(0); }
        let mut v = json!({ "buffer": 0, "byteOffset": self.data.len(), "byteLength": bytes.len() });
        if let Some(t) = target { v["target"] = json!(t); }
        self.data.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }
    fn acc(&mut self, bytes: &[u8], target: Option<u32>, comp: u32, count: usize, ty: &str, minmax: Option<(Vec<f32>, Vec<f32>)>) -> usize {
        let v = self.view(bytes, target);
        let mut a = json!({ "bufferView": v, "componentType": comp, "count": count, "type": ty });
        if let Some((lo, hi)) = minmax { a["min"] = json!(lo); a["max"] = json!(hi); }
        self.accessors.push(a);
        self.accessors.len() - 1
    }
    fn f32s<const N: usize>(&mut self, v: &[[f32; N]], target: Option<u32>, ty: &str, minmax: bool) -> usize {
        let bytes: Vec<u8> = v.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let mm = minmax.then(|| {
            let mut lo = vec![f32::MAX; N];
            let mut hi = vec![f32::MIN; N];
            for x in v { for k in 0..N { lo[k] = lo[k].min(x[k]); hi[k] = hi[k].max(x[k]); } }
            (lo, hi)
        });
        self.acc(&bytes, target, 5126, v.len(), ty, mm)
    }
}

/// Node indices, every parent before its children.
fn parent_first(a: &G3Actor) -> Vec<usize> {
    let mut order = vec![];
    let mut seen = vec![false; a.nodes.len()];
    fn visit(a: &G3Actor, i: usize, seen: &mut Vec<bool>, order: &mut Vec<usize>, depth: usize) {
        if seen[i] || depth > 512 { return; }
        if let Some(p) = a.nodes[i].parent { visit(a, p, seen, order, depth + 1); }
        if !seen[i] { seen[i] = true; order.push(i); }
    }
    for i in 0..a.nodes.len() { visit(a, i, &mut seen, &mut order, 0); }
    order
}

/// Keys with strictly rising times (a repeated time keeps its last value).
fn rising<T: Copy>(keys: &[(f32, T)]) -> Vec<(f32, T)> {
    let mut out: Vec<(f32, T)> = vec![];
    for &k in keys {
        match out.last_mut() { Some(l) if k.0 <= l.0 => { if k.0 == l.0 { *l = k; } } _ => out.push(k) }
    }
    out
}

/// Texture bytes (PNG) for an actor's texture name; `normal` asks for a normal map.
pub type TexFn<'a> = dyn FnMut(&str, bool) -> Result<Option<Vec<u8>>> + 'a;

#[derive(serde::Serialize)]
pub struct Summary { pub actor: String, pub joints: usize, pub vertices: usize, pub triangles: usize, pub clips: Vec<(String, f32)>, pub warnings: Vec<String> }

pub fn build(name: &str, a: &G3Actor, motions: &[(String, G3Motion)], tex: &mut TexFn) -> Result<(Vec<u8>, Summary)> {
    ensure!(!a.nodes.is_empty(), "the actor has no nodes");
    let order = parent_first(a);
    let mut new_of = vec![0usize; a.nodes.len()];
    for (n, &o) in order.iter().enumerate() { new_of[o] = n; }
    let parent = |n: usize| a.nodes[order[n]].parent.map(|p| new_of[p]);

    let mut globals: Vec<M4> = Vec::with_capacity(order.len());
    for n in 0..order.len() {
        let g3 = &a.nodes[order[n]];
        let local = trs(conv_p(g3.pos, UNIT), conv_q(g3.rot));
        globals.push(match parent(n) { Some(p) => mul(&globals[p], &local), None => local });
    }

    let mut bin = Bin { data: vec![], views: vec![], accessors: vec![] };
    let (mut images, mut textures, mut materials, mut meshes) = (vec![], vec![], vec![], vec![]);
    let mut tex_index: std::collections::HashMap<(String, bool), Option<usize>> = Default::default();
    let mut warnings = vec![];
    let mut mat_index: Vec<Option<usize>> = vec![None; a.materials.len().max(1)];
    let (mut verts, mut tris) = (0, 0);

    for m in &a.meshes {
        if m.triangles.is_empty() || m.positions.is_empty() { continue; }
        let nc = m.positions.len();
        verts += nc;
        let pos: Vec<[f32; 3]> = m.positions.iter().map(|p| conv_p(*p, UNIT)).collect();
        let nrm: Vec<[f32; 3]> = m.normals.iter().map(|n| { let c = conv_p(*n, 1.0); let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt(); if l > 1e-6 { c.map(|x| x / l) } else { [0.0, 1.0, 0.0] } }).collect();
        let uv: Vec<[f32; 2]> = if m.uvs.len() == nc { m.uvs.clone() } else { vec![[0.0, 0.0]; nc] };
        let (mut js, mut ws) = (Vec::with_capacity(nc * 8), Vec::with_capacity(nc));
        for c in 0..nc {
            let mut w: Vec<(u16, f32)> = m.weights.get(m.vertex[c] as usize).cloned().unwrap_or_default().into_iter().filter(|x| x.1 > 0.0 && (x.0 as usize) < a.nodes.len()).collect();
            w.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap());
            w.truncate(4);
            if w.is_empty() { w.push((m.node.min(a.nodes.len() - 1) as u16, 1.0)); }
            let s: f32 = w.iter().map(|x| x.1).sum();
            let mut j = [0u16; 4];
            let mut x = [0f32; 4];
            for (k, (b, v)) in w.iter().enumerate() { j[k] = new_of[*b as usize] as u16; x[k] = v / s; }
            for i in j { js.extend_from_slice(&i.to_le_bytes()); }
            ws.push(x);
        }
        let a_pos = bin.f32s(&pos, Some(34962), "VEC3", true);
        let a_nrm = bin.f32s(&nrm, Some(34962), "VEC3", false);
        let a_uv = bin.f32s(&uv, Some(34962), "VEC2", false);
        let a_j = bin.acc(&js, Some(34962), 5123, nc, "VEC4", None);
        let a_w = bin.f32s(&ws, Some(34962), "VEC4", false);
        // glTF wants counter-clockwise fronts in its (mirrored) space.
        let turn = crate::staticmesh::needs_turn(&m.positions, &m.normals, m.triangles.iter().copied());
        let mut prims = vec![];
        let slots: std::collections::BTreeSet<u32> = m.tri_material.iter().copied().collect();
        for slot in slots {
            let idx: Vec<u8> = m.triangles.iter().zip(&m.tri_material).filter(|(_, s)| **s == slot)
                .flat_map(|(t, _)| (if turn { [t[0], t[2], t[1]] } else { *t }).into_iter().flat_map(|i| i.to_le_bytes())).collect();
            let count = idx.len() / 4;
            tris += count / 3;
            let a_i = bin.acc(&idx, Some(34963), 5125, count, "SCALAR", None);
            let s = slot as usize;
            if s >= mat_index.len() { mat_index.resize(s + 1, None); }
            let mi = match mat_index[s] {
                Some(i) => i,
                None => {
                    let src = a.materials.get(s).cloned().unwrap_or_default();
                    let mut mat = json!({ "name": if src.name.is_empty() { format!("{name}_{s}") } else { src.name.clone() }, "pbrMetallicRoughness": { "metallicFactor": 0.0, "roughnessFactor": 1.0 } });
                    for (t, normal) in [(&src.diffuse, false), (&src.normal, true)] {
                        let Some(t) = t else { continue };
                        let key = (t.clone(), normal);
                        let ti = match tex_index.get(&key) {
                            Some(x) => *x,
                            None => {
                                let x = match tex(t, normal) {
                                    Ok(Some(png)) => { let bv = bin.view(&png, None); images.push(json!({ "name": t, "bufferView": bv, "mimeType": "image/png" })); textures.push(json!({ "source": images.len() - 1, "sampler": 0 })); Some(textures.len() - 1) }
                                    Ok(None) => { warnings.push(format!("texture {t} not in the game")); None }
                                    Err(e) => { warnings.push(format!("texture {t}: {e:#}")); None }
                                };
                                tex_index.insert(key, x);
                                x
                            }
                        };
                        if let Some(ti) = ti { if normal { mat["normalTexture"] = json!({ "index": ti }); } else { mat["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": ti }); } }
                    }
                    materials.push(mat);
                    mat_index[s] = Some(materials.len() - 1);
                    materials.len() - 1
                }
            };
            prims.push(json!({ "attributes": { "POSITION": a_pos, "NORMAL": a_nrm, "TEXCOORD_0": a_uv, "JOINTS_0": a_j, "WEIGHTS_0": a_w }, "indices": a_i, "material": mi }));
        }
        let mesh_name = a.nodes.get(m.node).map(|n| n.name.clone()).unwrap_or_else(|| format!("{name}_Mesh"));
        meshes.push(json!({ "name": mesh_name, "primitives": prims }));
    }

    let mut gnodes: Vec<Value> = (0..order.len()).map(|n| { let g3 = &a.nodes[order[n]]; json!({ "name": g3.name, "translation": conv_p(g3.pos, UNIT), "rotation": conv_q(g3.rot) }) }).collect();
    for n in 0..order.len() {
        if let Some(p) = parent(n) {
            let mut c: Vec<usize> = serde_json::from_value(gnodes[p].get("children").cloned().unwrap_or(json!([])))?;
            c.push(n);
            gnodes[p]["children"] = json!(c);
        }
    }
    let mut root_children: Vec<usize> = (0..order.len()).filter(|&n| parent(n).is_none()).collect();
    let mut skins = vec![];
    if !meshes.is_empty() {
        let ibm: Vec<[f32; 16]> = globals.iter().map(rigid_inverse).collect();
        let a_ibm = bin.f32s(&ibm, None, "MAT4", false);
        skins.push(json!({ "name": format!("{name}_Skin"), "joints": (0..order.len()).collect::<Vec<_>>(), "inverseBindMatrices": a_ibm }));
        for (i, m) in meshes.iter().enumerate() {
            root_children.push(gnodes.len());
            gnodes.push(json!({ "name": m["name"], "mesh": i, "skin": 0 }));
        }
    }
    let root = gnodes.len();
    gnodes.push(json!({ "name": name, "children": root_children }));

    // Motions.
    let names: std::collections::HashMap<String, usize> = (0..order.len()).map(|n| (a.nodes[order[n]].name.to_lowercase(), n)).collect();
    let mut gl_anims = vec![];
    let mut clips = vec![];
    for (clip, motion) in motions {
        let tracked: std::collections::HashMap<usize, &crate::g3_motion::G3Track> = motion.tracks.iter().filter_map(|t| names.get(&t.bone.to_lowercase()).map(|&n| (n, t))).collect();
        if tracked.is_empty() { warnings.push(format!("clip {clip}: no track names a node of this actor")); continue; }
        // Helpers between a tracked node and its nearest tracked ancestor.
        let mut held = std::collections::BTreeSet::new();
        for &n in tracked.keys() {
            let mut p = parent(n);
            while let Some(q) = p { if tracked.contains_key(&q) { break; } held.insert(q); p = parent(q); }
        }
        let dur = motion.duration.max(1.0 / 30.0);
        let (mut samplers, mut channels) = (vec![], vec![]);
        let mut put = |bin: &mut Bin, samplers: &mut Vec<Value>, channels: &mut Vec<Value>, node: usize, path: &str, times: Vec<[f32; 1]>, vals: Value| {
            let a_t = bin.f32s(&times, None, "SCALAR", true);
            let a_v = match vals { Value::Array(ref rows) if rows.first().map(|r| r.as_array().map(|x| x.len()) == Some(4)).unwrap_or(false) => {
                let v: Vec<[f32; 4]> = rows.iter().map(|r| { let r = r.as_array().unwrap(); [0, 1, 2, 3].map(|k| r[k].as_f64().unwrap() as f32) }).collect(); bin.f32s(&v, None, "VEC4", false) }
                Value::Array(rows) => { let v: Vec<[f32; 3]> = rows.iter().map(|r| { let r = r.as_array().unwrap(); [0, 1, 2].map(|k| r[k].as_f64().unwrap() as f32) }).collect(); bin.f32s(&v, None, "VEC3", false) }
                _ => unreachable!() };
            samplers.push(json!({ "input": a_t, "output": a_v, "interpolation": "LINEAR" }));
            channels.push(json!({ "sampler": samplers.len() - 1, "target": { "node": node, "path": path } }));
        };
        for &h in &held {
            put(&mut bin, &mut samplers, &mut channels, h, "translation", vec![[0.0], [dur]], json!([[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]));
            put(&mut bin, &mut samplers, &mut channels, h, "rotation", vec![[0.0], [dur]], json!([[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]]));
        }
        for (&n, t) in &tracked {
            let rot = rising(&t.rot);
            let (times, vals): (Vec<[f32; 1]>, Vec<[f32; 4]>) = if rot.is_empty() { (vec![[0.0], [dur]], vec![conv_q(t.pose_rot); 2]) } else {
                let mut prev = conv_q(rot[0].1);
                rot.iter().map(|k| { let mut q = conv_q(k.1); if q.iter().zip(&prev).map(|(a, b)| a * b).sum::<f32>() < 0.0 { q = q.map(|x| -x); } prev = q; ([k.0], q) }).unzip()
            };
            put(&mut bin, &mut samplers, &mut channels, n, "rotation", times, json!(vals));
            let pos = rising(&t.pos);
            let (times, vals): (Vec<[f32; 1]>, Vec<[f32; 3]>) = if pos.is_empty() { (vec![[0.0], [dur]], vec![conv_p(t.pose_pos, UNIT); 2]) } else { pos.iter().map(|k| ([k.0], conv_p(k.1, UNIT))).unzip() };
            put(&mut bin, &mut samplers, &mut channels, n, "translation", times, json!(vals));
            let sc = rising(&t.scale);
            if !sc.is_empty() {
                let (times, vals): (Vec<[f32; 1]>, Vec<[f32; 3]>) = sc.iter().map(|k| ([k.0], k.1)).unzip();
                put(&mut bin, &mut samplers, &mut channels, n, "scale", times, json!(vals));
            }
        }
        clips.push((clip.clone(), motion.duration));
        gl_anims.push(json!({ "name": clip, "samplers": samplers, "channels": channels }));
    }

    while bin.data.len() % 4 != 0 { bin.data.push(0); }
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": "gothic3-core" },
        "scene": 0, "scenes": [{ "name": name, "nodes": [root] }],
        "nodes": gnodes, "buffers": [{ "byteLength": bin.data.len() }],
        "bufferViews": bin.views, "accessors": bin.accessors,
    });
    if !meshes.is_empty() { doc["meshes"] = json!(meshes); doc["skins"] = json!(skins); doc["materials"] = json!(materials); }
    if !gl_anims.is_empty() { doc["animations"] = json!(gl_anims); }
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
        doc["samplers"] = json!([{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }]);
    }
    let mut js = serde_json::to_vec(&doc)?;
    while js.len() % 4 != 0 { js.push(b' '); }
    let total = 12 + 8 + js.len() + 8 + bin.data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E4942u32.to_le_bytes());
    out.extend_from_slice(&bin.data);
    Ok((out, Summary { actor: name.into(), joints: order.len(), vertices: verts, triangles: tris, clips, warnings }))
}
