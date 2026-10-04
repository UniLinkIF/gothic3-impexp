//! gothic3-core: the native part of the Gothic 3 ImpExp Blender add-on. Copyright (C) 2026 UniLinkIF,
//! GPL-3.0-or-later with additional terms (section 7): see NOTICE. One command per call, one JSON value on
//! stdout (errors on stderr, exit code 1).
//!
//! ```text
//! gothic3-core keys <game> <prefix>                         archive keys starting with prefix (debugging)
//! gothic3-core find <game> mesh|actor|motion                [name, folder] of every resource of that kind
//! gothic3-core mesh <game> <name> <out dir>                 static mesh as OBJ + MTL + PNG
//! gothic3-core collision <game> <name> <out.obj>            the mesh's collision
//! gothic3-core actor <game> <name> <out.glb> <clips> [limit] [full|skeleton] [head]
//!                                                           actor (+ clips: "" none, "@file" names, words; a head actor
//!                                                           put on a body by bone name; skeleton = no textures)
//! gothic3-core clips <game> <actor> [words] [limit]         clips that fit the actor
//! gothic3-core export <game> <spec.json> install <mod> | package <folder> <title>
//!                                                           a model from Blender into the game or a mod package
//! gothic3-core export-motion <game> <spec.json> install <mod> | package <folder> <title>
//!                                                           the animation of a glb on a game clip (replacing it or as a
//!                                                           new clip), with its frame effects| package <folder> <title>
//!                                                           a game clip replaced by the animation in the glb
//! gothic3-core export-actor <game> <spec.json> install <mod> | package <folder> <title>
//!                                                           a skinned mesh on a game actor's skeleton
//! gothic3-core installed <game>                             mods installed from Blender
//! gothic3-core uninstall <game> <mod>                       remove one
//! ```

mod cook;
mod dds;
mod export;
mod g3;
mod g3_actor;
mod g3_motion;
mod g3_res;
mod g3_write;
mod mods;
mod motion_write;
mod nxs;
mod geom;
mod glb;
mod lightmap;
mod staticmesh;
mod textures;
mod volume;
mod xact_write;
mod xcmsh_write;

use anyhow::{bail, Context, Result};
use std::path::Path;

pub fn stem_of(key: &str) -> String { stem(key) }
fn stem(key: &str) -> String { key.rsplit('/').next().unwrap_or(key).rsplit_once('.').map(|x| x.0).unwrap_or(key).to_string() }
fn folder(key: &str) -> String { let p: Vec<&str> = key.split('/').collect(); if p.len() > 2 { p[1..p.len() - 1].join("/") } else { String::new() } }

/// Every resource of a kind as (name, folder).
fn find(g: &g3::G3Ctx, kind: &str) -> Result<Vec<(String, String)>> {
    let (group, ext) = match kind { "mesh" => ("_compiledmesh/", ".xcmsh"), "actor" => ("_compiledanimation/", ".xact"), "motion" => ("_compiledanimation/", ".xmot"), k => bail!("kind {k}: mesh, actor or motion") };
    let mut v: Vec<(String, String)> = g.keys().into_iter().filter(|k| k.starts_with(group) && k.ends_with(ext)).map(|k| { let p = g.path_of(&k).unwrap_or(&k).to_string(); (stem(&p), folder(&format!("x/{p}"))) }).collect();
    v.sort();
    v.dedup();
    Ok(v)
}

/// The actor's clips: motions named after its rig (`G3_Wolf_Body_01` -> `Wolf_…`, `G3_Hero_Body_*` -> `Hero_…`,
/// a bow `It_Weapon_…` -> `It_…`), filtered by `words`.
fn clips_for(g: &g3::G3Ctx, actor: &str, words: &str, limit: usize) -> Result<Vec<String>> {
    let all = find(g, "motion")?;
    let a = actor.to_lowercase();
    let mut parts = a.split('_');
    let first = parts.next().unwrap_or("");
    let rig = if first == "g3" { parts.next().unwrap_or("").to_string() } else { first.to_string() };
    let ws: Vec<String> = words.to_lowercase().split_whitespace().map(String::from).collect();
    Ok(all.into_iter().map(|x| x.0).filter(|n| { let l = n.to_lowercase(); l.starts_with(&format!("{rig}_")) && ws.iter().all(|w| l.contains(w.as_str())) }).take(limit).collect())
}

/// Put `head`'s meshes on `body`: head bones map to the body's by name (the rigs share them); a head bone the
/// body lacks maps to the body's head bone. Returns notes on what did not map.
fn attach(body: &mut g3_actor::G3Actor, head: &g3_actor::G3Actor, head_name: &str) -> Vec<String> {
    let find = |n: &str| body.nodes.iter().position(|b| b.name.eq_ignore_ascii_case(n));
    let fallback = body.nodes.iter().position(|b| { let l = b.name.to_lowercase(); l.ends_with("_head") || l.contains("head_head") }).unwrap_or(0);
    let mut missing = std::collections::BTreeSet::new();
    let map: Vec<usize> = head.nodes.iter().map(|n| find(&n.name).unwrap_or_else(|| { missing.insert(n.name.clone()); fallback })).collect();
    let base = body.materials.len() as u32;
    body.materials.extend(head.materials.iter().cloned());
    for m in &head.meshes {
        let mut m = m.clone();
        m.label = Some(head_name.to_string());
        m.node = map.get(m.node).copied().unwrap_or(fallback);
        for w in m.weights.iter_mut().flatten() { w.0 = map.get(w.0 as usize).copied().unwrap_or(fallback) as u16; }
        for t in m.tri_material.iter_mut() { *t += base; }
        body.meshes.push(m);
    }
    if missing.is_empty() { vec![] } else { vec![format!("head bones not in the body (put on its head bone): {}", missing.into_iter().collect::<Vec<_>>().join(", "))] }
}

fn run(args: &[String]) -> Result<serde_json::Value> {
    let a = |i: usize| args.get(i).map(String::as_str);
    let open = |p: &str| g3::G3Ctx::open(Path::new(p));
    Ok(match (a(1), a(2)) {
        (Some("keys"), Some(game)) => {
            let g = open(game)?;
            let pre = a(3).unwrap_or("").to_lowercase();
            serde_json::to_value(g.keys().into_iter().filter(|k| k.starts_with(&pre)).take(a(4).and_then(|x| x.parse().ok()).unwrap_or(200)).collect::<Vec<_>>())?
        }
        (Some("find"), Some(game)) => serde_json::to_value(find(&open(game)?, a(3).unwrap_or("mesh"))?)?,
        (Some("mesh"), Some(game)) => {
            let g = open(game)?;
            serde_json::to_value(staticmesh::export_obj(&g, a(3).context("mesh name")?, Path::new(a(4).context("out dir")?))?)?
        }
        (Some("collision"), Some(game)) => staticmesh::collision_obj(&open(game)?, a(3).context("mesh name")?, Path::new(a(4).context("out.obj")?))?,
        (Some("clips"), Some(game)) => serde_json::to_value(clips_for(&open(game)?, a(3).context("actor")?, a(4).unwrap_or(""), a(5).and_then(|x| x.parse().ok()).unwrap_or(500))?)?,
        (Some("actor"), Some(game)) => {
            let g = open(game)?;
            let name = a(3).context("actor name")?.trim_end_matches(".xact");
            let out = Path::new(a(4).context("out.glb")?);
            let query = a(5).unwrap_or("");
            let limit = a(6).and_then(|x| x.parse().ok()).unwrap_or(40);
            let key = g.find("_compiledanimation", &format!("{name}.xact")).with_context(|| format!("{name}: no such actor in Gothic 3"))?;
            let mut actor = g3_actor::decode_xact(&g.read(&key)?).with_context(|| format!("decode {key}"))?;
            let with_textures = a(7) != Some("skeleton");
            let mut head_note = None;
            if let Some(h) = a(8).filter(|h| !h.is_empty() && *h != "-") {
                let hk = g.find("_compiledanimation", &format!("{}.xact", h.trim_end_matches(".xact"))).with_context(|| format!("{h}: no such head in Gothic 3"))?;
                let head = g3_actor::decode_xact(&g.read(&hk)?).with_context(|| format!("decode {hk}"))?;
                head_note = Some(attach(&mut actor, &head, &stem(g.path_of(&hk).unwrap_or(&hk))));
            }
            let names: Vec<String> = if query.is_empty() { vec![] } else if let Some(f) = query.strip_prefix('@') {
                std::fs::read_to_string(f).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(limit).collect()
            } else { clips_for(&g, name, query, limit)? };
            let mut motions = vec![];
            let mut warnings = vec![];
            for n in names {
                match g.find("_compiledanimation", &format!("{n}.xmot")).context("not in the game").and_then(|k| g3_motion::decode_xmot(&g.read(&k)?)) {
                    Ok(m) => motions.push((n, m)),
                    Err(e) => warnings.push(format!("clip {n} skipped: {e:#}")),
                }
            }
            let cache = out.parent().map(|p| p.join("textures")).unwrap_or_default();
            let (bytes, mut s) = glb::build(&stem(g.path_of(&key).unwrap_or(&key)), &actor, &motions, with_textures, &mut |t: &str, normal: bool| -> Result<Option<Vec<u8>>> {
                Ok(match textures::png(&g, t, &cache, normal)? { Some(p) => Some(std::fs::read(p)?), None => None })
            })?;
            if let Some(p) = out.parent() { std::fs::create_dir_all(p)?; }
            std::fs::write(out, bytes)?;
            s.warnings.splice(0..0, warnings);
            if let Some(n) = head_note { s.warnings.extend(n); }
            let mut v = serde_json::to_value(s)?;
            v["glb"] = serde_json::json!(out.to_string_lossy());
            v
        }
        (Some("export"), Some(game)) => {
            let g = g3::G3Ctx::open_original(Path::new(game))?;
            export::run(&g, Path::new(game), a(3).context("spec.json")?, a(4).unwrap_or("install"), a(5).context("mod name or package folder")?, a(6).unwrap_or(""))?
        }
        (Some("export-motion"), Some(game)) => {
            let g = open(game)?;
            let spec: motion_write::MotionSpec = serde_json::from_str(&std::fs::read_to_string(a(3).context("spec.json")?)?).context("motion spec")?;
            let (report, file) = motion_write::build(&g, &spec)?;
            let mut out = serde_json::json!({ "report": report });
            match a(4).unwrap_or("install") {
                "install" => { out["volumes"] = serde_json::json!(mods::install(Path::new(game), a(5).context("mod name")?, &[file])?); }
                "package" => { mods::package(Path::new(a(5).context("package folder")?), a(6).unwrap_or(""), &[file])?; }
                m => bail!("mode {m}: install or package"),
            }
            out
        }
        (Some("export-actor"), Some(game)) => {
            let g = open(game)?;
            let spec: xact_write::ActorSpec = serde_json::from_str(&std::fs::read_to_string(a(3).context("spec.json")?)?).context("actor spec")?;
            let (report, files) = xact_write::build(&g, &spec)?;
            let mut out = serde_json::json!({ "report": report });
            match a(4).unwrap_or("install") {
                "install" => { out["volumes"] = serde_json::json!(mods::install(Path::new(game), a(5).context("mod name")?, &files)?); }
                "package" => { mods::package(Path::new(a(5).context("package folder")?), a(6).unwrap_or(""), &files)?; }
                m => bail!("mode {m}: install or package"),
            }
            out
        }
        (Some("installed"), Some(game)) => serde_json::to_value(mods::installed(Path::new(game)))?,
        (Some("uninstall"), Some(game)) => { mods::uninstall(Path::new(game), a(3).context("mod name")?)?; serde_json::json!(true) }
        _ => bail!("usage: gothic3-core keys|find|mesh|collision|clips|actor|export|installed|uninstall <game folder> …"),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(v) => println!("{v}"),
        Err(e) => { eprintln!("gothic3-core: {e:#}"); std::process::exit(1); }
    }
}
