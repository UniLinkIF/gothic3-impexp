//! What a world editor needs to place a model: a sidecar (`<name>.placement.json`) written with each export.
//!
//! A model shows in the game only where an entity of a world layer (`.node`) places it. Its collision is not taken
//! from the model's files by name: the entity holds an `eCCollisionShape_PS` property set with one shape per part,
//! each naming the collision file, the part's index in it, the surface and the part's box. Read from every compiled
//! layer of the game (see the test):
//! ```text
//! eCCollisionShape_PS   u16 1 · u8 1 · u16 1 · u8 1 · u16 class · u8 1 · u16 0 · u16 83 · u16 83 · u32 size
//!                       · u16 30 · u32 5 properties (u16 name · u16 type · u16 30 · u32 size · value):
//!                         Group (enum eECollisionGroup) · Range (enum eEPhysicRangeType) · DisableCollision,
//!                         DisableResponse, IgnoredByTraceRay (bool)
//!                       · u16 63 · u32 shapes · per shape:
//! eCCollisionShape      the same head · u32 15 properties: ShapeType, Group (eEShapeGroup), Material
//!                         (eEShapeMaterial), ShapeAABBAdaptMode, IgnoredByTraceRay, EnableCCD, OverrideEntityAABB,
//!                         DisableCollision, DisableResponse, TriggersOnTouch, TriggersOnUntouch, TriggersOnIntersect,
//!                         SkinWidth (float), IsLazyGenerated (bool), FileVersion (short 74)
//!                       · for a triangle mesh (ShapeType 1) or a convex hull (6), 44 bytes:
//!                         u16 74 · u16 file (string) · u16 part index · u16 1 · f32 box min[3] max[3] · 12 bytes 0
//! ```
//! An enum is `u16 1 · u32 value`. Boxes are in the model's own centimetres; a hull's is empty (FLT_MAX, −FLT_MAX).

use anyhow::{bail, Result};

/// One property object: (class, properties (name, type, value), the class's own data, where it ends).
pub(crate) fn object(d: &[u8], t: &[String], p: usize) -> Option<(String, Vec<(String, String, Vec<u8>)>, Vec<u8>, usize)> {
    let u16_ = |q: usize| d.get(q..q + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_ = |q: usize| d.get(q..q + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    if d.get(p..p + 6)? != [1, 0, 1, 1, 0, 1] { return None; }
    let class = t.get(u16_(p + 6)? as usize)?.clone();
    if d.get(p + 8..p + 13)? != [1, 0, 0, 0x53, 0] || u16_(p + 13)? != 0x53 { return None; }
    let start = p + 19;
    let end = start + u32_(p + 15)?;
    if u16_(start)? != 30 { return None; }
    let mut q = start + 6;
    let mut props = vec![];
    for _ in 0..u32_(start + 2)? {
        let (name, ty) = (t.get(u16_(q)? as usize)?.clone(), t.get(u16_(q + 2)? as usize)?.clone());
        if u16_(q + 4)? != 30 { return None; }
        let n = u32_(q + 6)?;
        props.push((name, ty, d.get(q + 10..q + 10 + n)?.to_vec()));
        q += 10 + n;
    }
    Some((class, props, d.get(q..end)?.to_vec(), end))
}

/// A shape of an entity's collision.
#[derive(Debug, Clone)]
pub struct Shape { pub shape_type: u32, pub group: u32, pub material: u32, pub skin_width: f32, pub file: Option<String>, pub part: u16, pub bounds: Option<([f32; 3], [f32; 3])>, pub props: Vec<(String, String, Vec<u8>)>, pub data: Vec<u8> }

/// An entity's `eCCollisionShape_PS`: its own properties and its shapes.
#[derive(Debug, Clone)]
pub struct Collision { pub props: Vec<(String, String, Vec<u8>)>, pub shapes: Vec<Shape> }

fn enum_of(props: &[(String, String, Vec<u8>)], name: &str) -> Option<u32> { props.iter().find(|p| p.0 == name).filter(|p| p.2.len() == 6).map(|p| u32::from_le_bytes(p.2[2..6].try_into().unwrap())) }

/// Every `eCCollisionShape_PS` of a compiled (GENOMFLE) world layer.
pub fn collisions(d: &[u8]) -> Result<Vec<Collision>> {
    if !d.starts_with(b"GENOMFLE") { bail!("not a compiled world layer"); }
    let t = crate::g3_res::genomfle_strings(d)?;
    let Some(ci) = t.iter().position(|x| x == "eCCollisionShape_PS") else { return Ok(vec![]) };
    let end = (u32::from_le_bytes(d[10..14].try_into().unwrap()) as usize).min(d.len());
    let head = [1u8, 0, 1, 1, 0, 1, (ci & 255) as u8, (ci >> 8) as u8];
    let mut out = vec![];
    let mut p = 14;
    while p + 8 < end {
        if d[p..p + 8] != head { p += 1; continue; }
        let Some((_, props, data, oend)) = object(d, &t, p) else { bail!("eCCollisionShape_PS at 0x{p:x} does not read") };
        if data.len() < 6 || u16::from_le_bytes([data[0], data[1]]) != 63 { bail!("eCCollisionShape_PS at 0x{p:x}: version") }
        let n = u32::from_le_bytes(data[2..6].try_into().unwrap()) as usize;
        let mut q = oend - data.len() + 6;
        let mut shapes = vec![];
        for _ in 0..n {
            let Some((class, sp, sd, send)) = object(d, &t, q) else { bail!("shape at 0x{q:x} does not read") };
            if class != "eCCollisionShape" { bail!("shape at 0x{q:x} is {class}"); }
            let f = |i: usize| f32::from_le_bytes(sd[8 + i * 4..12 + i * 4].try_into().unwrap());
            let file_shape = sd.len() == 44;
            let skin = sp.iter().find(|x| x.0 == "SkinWidth").map(|x| f32::from_le_bytes(x.2[..4].try_into().unwrap())).unwrap_or(0.0);
            shapes.push(Shape {
                shape_type: enum_of(&sp, "ShapeType").unwrap_or(0), group: enum_of(&sp, "Group").unwrap_or(0), material: enum_of(&sp, "Material").unwrap_or(0), skin_width: skin,
                file: file_shape.then(|| t.get(u16::from_le_bytes([sd[2], sd[3]]) as usize).cloned()).flatten(),
                part: if file_shape { u16::from_le_bytes([sd[4], sd[5]]) } else { 0 },
                bounds: file_shape.then(|| ([f(0), f(1), f(2)], [f(3), f(4), f(5)])),
                props: sp, data: sd,
            });
            q = send;
        }
        if q != oend { bail!("eCCollisionShape_PS at 0x{p:x}: shapes end at 0x{q:x}, the set at 0x{oend:x}"); }
        out.push(Collision { props, shapes });
        p = oend;
    }
    Ok(out)
}

/// Shape types (`eECollisionShapeType`) a model's files give: a triangle mesh (`*_COL`, `<name>.xnvmsh`) and a convex
/// hull (`*_CV`). The game also places boxes (3), capsules (4) and spheres (5) of its own, without a file.
pub const TRIANGLE_MESH: u32 = 1;
pub const CONVEX_HULL: u32 = 6;

/// A part of the model's collision as the sidecar names it.
pub struct Part { pub shape_type: u32, pub file: String, pub part: usize, pub material: u8, pub bounds: ([f32; 3], [f32; 3]) }

/// The sidecar: the mesh, its box, the far version, and per collision part the shape an entity needs, with the values
/// the game's own entities give such shapes.
pub fn sidecar(mesh_path: &str, bounds: ([f32; 3], [f32; 3]), far: Option<&str>, parts: &[Part]) -> serde_json::Value {
    let file = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let bx = |b: &([f32; 3], [f32; 3])| serde_json::json!({ "min": b.0, "max": b.1 });
    let shapes: Vec<serde_json::Value> = parts.iter().map(|p| {
        let mesh = p.shape_type == TRIANGLE_MESH;
        serde_json::json!({
            "ShapeType": p.shape_type, "kind": if mesh { "triangle mesh" } else { "convex hull" },
            "file": file(&p.file), "path": p.file, "part": p.part,
            "Material": p.material, "surface": crate::g3_write::SHAPE_MATERIALS.get(p.material as usize).copied().unwrap_or("none"),
            "Group": 1, "ShapeAABBAdaptMode": 0, "SkinWidth": if mesh { 0.0001 } else { -1.0 }, "FileVersion": 74,
            "IgnoredByTraceRay": false, "EnableCCD": false, "OverrideEntityAABB": false, "DisableCollision": false, "DisableResponse": false,
            "TriggersOnTouch": false, "TriggersOnUntouch": false, "TriggersOnIntersect": false, "IsLazyGenerated": false,
            "box": if mesh { bx(&p.bounds) } else { bx(&crate::g3_write::EMPTY_BOX) },
        })
    }).collect();
    serde_json::json!({
        "format": "Gothic 3 ImpExp placement 1",
        "mesh": { "ResourceFileName": file(mesh_path), "path": format!("_compiledMesh/{mesh_path}"), "box_cm": bx(&bounds) },
        "far_version": far.map(|f| serde_json::json!({ "ResourceFileName": file(f), "path": format!("_compiledMesh/{f}"), "layer": "the distance layer (g3_world_lowpoly_01_levelmesh_01_spat)" })),
        "eCCollisionShape_PS": { "Group": 1, "Range": 1, "DisableCollision": false, "DisableResponse": false, "IgnoredByTraceRay": false, "shapes": shapes },
        "notes": [
            "Coordinates: centimetres, the game's axes (left-handed, Y up), the model's own origin.",
            "One eCCollisionShape per part, in this order: the world names a part by its index in the file and gives it the surface here; a part left out has no collision.",
            "A new placement has no lightmap of its own: the mesh's vertex colours light it."
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    /// Every collision property set of every compiled world layer reads to its end. The shapes with a file — triangle
    /// meshes and convex hulls — name the part by its index in the file; a mesh part's box is that part's box in the
    /// file and its surface the file's (almost always), a hull's box is empty. The entity and shape values the sidecar
    /// writes are the ones the game's entities with such shapes have.
    #[test]
    fn the_worlds_collision_shapes_read_as_the_sidecar_writes_them() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (mut sets, mut mesh, mut hull) = (0, 0, 0);
        let (mut box_same, mut box_other, mut mat_same, mut mat_other, mut hull_empty) = (0, 0, 0, 0, 0);
        let mut entity: BTreeMap<String, usize> = BTreeMap::new();
        let mut shape_vals: BTreeMap<String, usize> = BTreeMap::new();
        let mut parts: BTreeMap<usize, usize> = BTreeMap::new();
        let mut files: HashMap<String, Option<crate::g3_res::Xnv>> = HashMap::new();
        for k in g.keys().into_iter().filter(|k| k.starts_with("projects_compiled/") && k.ends_with(".node")) {
            let d = g.read(&k).unwrap();
            if !d.starts_with(b"GENOMFLE") { continue; }
            for c in collisions(&d).unwrap_or_else(|e| panic!("{k}: {e:#}")) {
                sets += 1;
                let names: Vec<&str> = c.props.iter().map(|p| p.0.as_str()).collect();
                assert_eq!(names, ["Group", "Range", "DisableCollision", "DisableResponse", "IgnoredByTraceRay"], "{k}");
                let with_file: Vec<&Shape> = c.shapes.iter().filter(|s| s.shape_type == TRIANGLE_MESH || s.shape_type == CONVEX_HULL).collect();
                if with_file.is_empty() { continue; }
                *parts.entry(with_file.len()).or_default() += 1;
                *entity.entry(c.props.iter().map(|p| format!("{}={:?}", p.0, p.2)).collect::<Vec<_>>().join(" ")).or_default() += 1;
                for s in with_file {
                    assert_eq!(s.data.len(), 44, "{k}: a shape with a file has 44 bytes of its own");
                    assert_eq!((&s.data[..2], &s.data[6..8], &s.data[32..]), (&[74u8, 0][..], &[1u8, 0][..], &[0u8; 12][..]), "{k}");
                    let file = s.file.clone().unwrap();
                    assert!(file.to_lowercase().ends_with(".xnvmsh"), "{k}: {file}");
                    let vals: Vec<String> = s.props.iter().filter(|p| !["ShapeType", "Material"].contains(&p.0.as_str())).map(|p| format!("{}={:?}", p.0, p.2)).collect();
                    *shape_vals.entry(format!("type {} {}", s.shape_type, vals.join(" "))).or_default() += 1;
                    let x = files.entry(file.to_lowercase()).or_insert_with(|| {
                        let fk = g.find("_compiledphysic", &file).or_else(|| g.find("_compiledmesh", &file))?;
                        crate::g3_res::parse_xnvmsh(&g.read(&fk).ok()?).ok()
                    });
                    let Some(x) = x else { continue };
                    let b = s.bounds.unwrap();
                    if s.shape_type == CONVEX_HULL {
                        hull += 1;
                        if b == crate::g3_write::EMPTY_BOX { hull_empty += 1; }
                        continue;
                    }
                    mesh += 1;
                    let Some((_, fb)) = x.streams.get(s.part as usize) else { panic!("{k}: {file} has no part {}", s.part) };
                    if (0..3).all(|i| (fb.0[i] - b.0[i]).abs() < 0.5 && (fb.1[i] - b.1[i]).abs() < 0.5) { box_same += 1 } else { box_other += 1 }
                    match x.shapes.get(s.part as usize) { Some(m) if m[0] as u32 == s.material => mat_same += 1, Some(_) => mat_other += 1, None => {} }
                }
            }
        }
        let most = |m: &BTreeMap<String, usize>, pre: &str| m.iter().filter(|(k, _)| k.starts_with(pre)).max_by_key(|x| x.1).map(|x| x.0.clone()).unwrap_or_default();
        eprintln!("{sets} sets; {mesh} mesh parts (box as the file's {box_same}, not {box_other}; surface as the file's {mat_same}, not {mat_other}); {hull} hulls ({hull_empty} empty boxes); parts per entity {parts:?}");
        eprintln!("entity: {:?}\nmesh: {}\nhull: {}", most(&entity, ""), most(&shape_vals, "type 1 "), most(&shape_vals, "type 6 "));
        assert!(sets > 100_000 && mesh > 10_000 && hull > 40_000, "{sets} sets, {mesh} mesh parts, {hull} hulls");
        assert!(box_same * 100 >= (box_same + box_other) * 99 && mat_same * 100 >= (mat_same + mat_other) * 98 && hull_empty * 100 >= hull * 99);
        assert!(parts.keys().any(|&n| n >= 5), "{parts:?}");
        // What the sidecar writes: the most common values of entities and shapes with a file.
        assert_eq!(most(&entity, ""), "Group=[1, 0, 1, 0, 0, 0] Range=[1, 0, 1, 0, 0, 0] DisableCollision=[0] DisableResponse=[0] IgnoredByTraceRay=[0]");
        let common = "IgnoredByTraceRay=[0] EnableCCD=[0] OverrideEntityAABB=[0] DisableCollision=[0] DisableResponse=[0] TriggersOnTouch=[0] TriggersOnUntouch=[0] TriggersOnIntersect=[0]";
        assert_eq!(most(&shape_vals, "type 1 "), format!("type 1 Group=[1, 0, 1, 0, 0, 0] ShapeAABBAdaptMode=[1, 0, 0, 0, 0, 0] {common} SkinWidth=[23, 183, 209, 56] IsLazyGenerated=[0] FileVersion=[74, 0]"));
        assert_eq!(most(&shape_vals, "type 6 "), format!("type 6 Group=[1, 0, 1, 0, 0, 0] ShapeAABBAdaptMode=[1, 0, 0, 0, 0, 0] {common} SkinWidth=[0, 0, 128, 191] IsLazyGenerated=[0] FileVersion=[74, 0]"));
        assert_eq!(f32::from_le_bytes([23, 183, 209, 56]), 0.0001);
    }
}
