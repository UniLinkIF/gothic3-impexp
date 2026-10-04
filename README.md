# Gothic 3 ImpExp — Blender add-on for Gothic 3

**English** · [Українська](README.uk.md)

Import and export **Gothic 3** (Piranha Bytes, 2006, Genome engine) models, characters, animations and collision in
**Blender 5.1+**, straight from and back into the game's archives.

> **Status: 0.9 beta.** Everything the add-on writes is checked against the game's own files and read back through
> the game's archive lookup, but it has not yet been played through in the game. Keep your game backed up and report
> what you find.

## What it does

**File → Import** (search by name within a category):

| Entry | Game files | Comes in as |
|---|---|---|
| Gothic 3 Model | `.xcmsh` + `.xshmat` + `.ximg` | mesh with textures (positions welded, uv/normal seams kept), alpha test/blend and specular set up, and its collision as a wireframe `<name>_COL` child. Categories: items, buildings, decor and furniture, plants, locations, landscape, water, technical |
| Gothic 3 Character | `.xact` | armature (bones as sticks) and skinned mesh, no animation; a human body with the head of your choice (the Nameless Hero's by default). Categories: humans — body or head, animals and monsters, items and objects |
| Gothic 3 Animations | `.xmot` | animations onto the selected Gothic 3 skeleton: one clip or a whole category — idle, movement, attacks, defence, dialogue and gestures, sitting and lying, interaction, death |
| Gothic 3 Collision | `.xnvmsh` | only the collision, as a wireframe mesh; one `G3_Shape_<surface>` material per surface (stone, wood, earth…). A landscape cell's collision is its own triangles |

**File → Export** (install into the game, or build a mod package with `INSTALL.bat` / `ROLLBACK.bat`):

| Entry | Writes |
|---|---|
| Gothic 3 Model → Mod | `.xcmsh` replacing the game's mesh of the same name or a new one, new materials (`.xshmat` from game templates: opaque, specular, alpha test) and images (`.ximg`), collision (`.xnvmsh`, from the `*_COL` objects or the model itself; for the landscape always from the model) with surfaces by material. Replacing a game model takes its vertex colours and lighting (the lightmaps of all its placements in the world) from the nearest old vertices, and replaces its `_lod1` too |
| Gothic 3 Motion → Mod | a `.xmot` replacing one of the game's clips (by default the one the action came from) or a new clip beside it; IK and other constraints go in as their result; frame effects (footsteps, sounds) from the action's list, edited in the Gothic 3 tab |
| Gothic 3 Actor → Mod | a `.xact` — your mesh on a game skeleton (armour, bodies, heads, creatures with one mesh), weights from vertex groups |

Mods go into the game as a patch volume per archive (`Data\_compiledMesh.pNN` and so on, after the game's own), the way
the game's patches do; the archives themselves are never changed. The **Gothic 3** tab lists the mods installed from
Blender with a button to remove each.

The **Gothic 3** tab in the 3D view sidebar has all of these and, with a Gothic 3 armature selected, an
**animation rig**: bone colours and collections, shapes to grab, and IK for arms and legs (a target and a pole per
limb). The rig only adds bones that deform nothing; IK starts off, and switching a limb on snaps it to the current
pose.

## Install

1. Blender 5.1 or newer.
2. *Edit → Preferences → Get Extensions → ⌄ → Install from Disk…* → `gothic3_impexp-0.9.2.zip`.
3. In the add-on's preferences check **Gothic 3 game folder** (the one with `Gothic3.exe`; the Steam default is
   filled in).

The zip carries `gothic3-core.exe`, the native part (Windows x64). Nothing else is needed — no PhysX SDK, no tools.
Installed mods live in `Gothic 3\gothic3_impexp\` (their files and a list) plus one patch volume per archive in `Data`.

## Quick start

- **Replace a prop:** Import → Gothic 3 Model → `G3_Object_Barrel_02`. Edit it, keep the origin. Export → Gothic 3
  Model → Mod, same name, *Install*. Start the game.
- **New armour:** Import → Gothic 3 Character → `G3_Hero_Body_Player`. Change the body mesh (keep it skinned), select
  only it, Export → Gothic 3 Actor, same name. The head is its own actor: select it alone to export it.
- **Change an animation:** Import → Gothic 3 Character → the creature, select the armature, Import → Gothic 3
  Animations. Edit the action (the animation rig helps), Export → Gothic 3 Motion: the clip it came from is preselected.

## Limits (0.9)

- Meshes are written as the game's (version-5 elements with vertex colours and a sphere tree) but without a second uv
  set: a replaced model's lighting is written per vertex (as three in four of the game's lightmaps are), even where the
  game had lightmap pages.
- The far low-poly versions of the world (`*_lowpoly`, `G3_World_Landscape_Lowpoly_*`) are not replaced.
- Collision streams follow the game's layout; the per-edge contact flags are computed by Risen's rule, not Gothic 3's.
- A new motion takes a game clip as its template (its bones and structure); actors only replace (or sit beside) a
  one-mesh actor of the game.
- A new model or actor shows up in the game only once something places or references it.

## Notes

- **Collision surface** (footstep sound): material panel → Gothic 3 → *Collision surface*. Auto takes it from the name: the
  game's landscape materials have their own (grass and forest floor = clay, gravel = debris, paths = earth, rock = stone,
  sand = sand), others by words in the name (wood, metal, stone…), the rest get the export dialog's default.
- Importing several models that share a game material gives one shared material (no `.001`); the meshes stay separate.
- Gothic 3 works in centimetres; the add-on imports at 0.01 (metres).
- Patch volumes (`.p00`, `.p01`, … of the Community Patch) are read over the base archives, as the game does.
- A motion track is relative to the nearest ancestor that has a track too; the helper bones in between
  (`*_ROOT`, `*_END`) are held at the identity while a clip plays, as in the game.

## Build from source

```sh
cd core && cargo build --release                          # gothic3-core.exe
BLENDER=/path/to/blender.exe tools/build_release.sh       # dist/gothic3_impexp-<version>.zip
```

## License

Copyright © 2026 UniLinkIF. Gothic 3 ImpExp is free software under the **GNU GPL 3.0 or later** (`LICENSE`) **with
additional terms** under its section 7 (`NOTICE`), which go with every copy and every modified version:

- **Attribution:** keep `NOTICE`, the copyright line and the attribution "Gothic 3 ImpExp by UniLinkIF" with the link
  to this repository (in the add-on's preferences and in the documentation).
- **Origin:** a modified version must say it is modified and by whom, carry a different name and version, and must
  not be presented as the original or as made or endorsed by UniLinkIF.
- **Names:** no rights to the names "Gothic 3 ImpExp" or "UniLinkIF" for forks, products or publicity.

Gothic 3 is © Piranha Bytes / THQ Nordic. This add-on ships no game data and is not affiliated with them; it reads the
game you own.
