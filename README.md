# Gothic 3 ImpExp — Blender add-on for Gothic 3

**English** · [Українська](README.uk.md)

Import **Gothic 3** (Piranha Bytes, 2006, Genome engine) models, characters, animations and collision into
**Blender 5.1+**, straight from the game's archives.

> **Status: 0.1, import only.** Writing back into Gothic 3 (models, characters, animations, mod volumes) is the
> next stage.

## What it does

**File → Import** (search by name within a category):

| Entry | Game files | Comes in as |
|---|---|---|
| Gothic 3 Model | `.xcmsh` + `.xshmat` + `.ximg` | mesh with textures (positions welded, uv/normal seams kept), alpha test/blend and specular set up, and its collision as a wireframe `<name>_COL` child. Categories: items, buildings, decor and furniture, plants, locations, landscape, water, technical |
| Gothic 3 Character | `.xact` | armature (bones as sticks) and skinned mesh, no animation; a human body with the head of your choice (the Nameless Hero's by default). Categories: humans — body or head, animals and monsters, items and objects |
| Gothic 3 Animations | `.xmot` | animations onto the selected Gothic 3 skeleton: one clip or a whole category — idle, movement, attacks, defence, dialogue and gestures, sitting and lying, interaction, death |
| Gothic 3 Collision | `.xnvmsh` | only the collision, as a wireframe mesh |

The **Gothic 3** tab in the 3D view sidebar has all of these and, with a Gothic 3 armature selected, an
**animation rig**: bone colours and collections, shapes to grab, and IK for arms and legs (a target and a pole per
limb). The rig only adds bones that deform nothing; IK starts off, and switching a limb on snaps it to the current
pose.

## Install

1. Blender 5.1 or newer.
2. *Edit → Preferences → Get Extensions → ⌄ → Install from Disk…* → `gothic3_impexp-0.1.0.zip`.
3. In the add-on's preferences check **Gothic 3 game folder** (the one with `Gothic3.exe`; the Steam default is
   filled in).

The zip carries `gothic3-core.exe`, the native part (Windows x64). Nothing else is needed. The add-on only reads
the game; it never writes into the game folder.

## Notes

- Gothic 3 works in centimetres; the add-on imports at 0.01 (metres).
- Patch volumes (`.p00`, `.p01`, … of the Community Patch) are read over the base archives, as the game does.
- A motion track is relative to the nearest ancestor that has a track too; the helper bones in between
  (`*_ROOT`, `*_END`) are held at the identity while a clip plays, as in the game.

## Build from source

```sh
cd core && cargo build --release                          # gothic3-core.exe
BLENDER=/path/to/blender.exe tools/build_release.sh       # dist/gothic3_impexp-<version>.zip
```

## Legal

Gothic 3 is © Piranha Bytes / THQ Nordic. This add-on ships no game data; it reads the game you own.

License: GPL-3.0-or-later (see `LICENSE`).
