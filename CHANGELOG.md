# Changelog

## Unreleased
- **Import → Gothic 3 Terrain**: landscape cells by name or around the 3D cursor, as one mesh welded across the
  cells' borders, with all a cell carries: the game's materials, its uv map, its colours as corner colour attributes
  (`g3_diffuse`, whose alpha blends the ground layers, and `g3_light`, the baked brightness), Varant's three further uv
  sets, and the cell of every face (`g3_cell`). The cells come as the game ships them, without any mod.
- **Export → Gothic 3 Terrain → Mod**: only the cells that changed are written (mesh, collision from their own
  triangles, lightmap); a cell that comes back as it went out is left to the game. Painted colours go along as they are.
- Motion export **as a new variant** of the clip's action: the next free variant number (the wolf's ambient loops
  01–05 → 06), named the way the game composes clip names. The report says what the game makes of a new name: a
  variant, words the game never composes, or a name it does not look up.
- Model export: a replaced mesh with further uv sets (Varant's landscape cells) keeps them; before, they were lost.

## 0.9.10 — 2026-11-02
- An export reads the game as it ships without any mod's volume: ours, a package's put in by `INSTALL.bat`, a mod
  launcher's. A volume is a mod's when its files name their place in the archive, not the developers' disks.
- Motion and actor exports take the game's clip and actor as their template, not a version a mod put in.

## 0.9.9 — 2026-11-02
- A new actor's name becomes a file name the way a model's does (Cyrillic transliterated, never a path), and the base
  actor's name with Blender's `.001` still replaces it.
- The world layer's name search no longer fails when the game folder is not set yet.
- `bl_info` carries the release's version again (0.9.8 still said 0.9.7).

## 0.9.8 — 2026-10-28
- **Import → Gothic 3 World Layer**: what the game places in one world layer (`.node`), and within a radius around it
  the town's houses and rocks (the static cells, LoD sets shown at full detail) and the ground, as context to build
  against. Each mesh is loaded once and linked at each placement; read only.

## 0.9.7 — 2026-10-27
- Export to **one mod file** (`.g3mod`, the default): a ZIP with `mod.json` and the files under their archives, put into
  the game's `Mods` folder for a mod launcher to put in and take out. The author signing it is set in the preferences.

## 0.9.6 — 2026-10-27
- Collision is found where the game keeps it for about a thousand more models: `_compiledPhysic/<name>.xnvmsh`,
  without `_COL`. Open meshes (PhysX mass −1) and older plain files read; every collision file a model uses (4161) reads.
- Items' convex bodies (`*_CV`): imported as `<name>_CV`, exported from `*_CV` objects or the model's own shape, cooked
  by the game's PhysX as hulls.
- New `.ximg` images have the game's head layout and mip chain.
- Mod volumes never leave a gap in their numbers (the game stops at the first missing one); `INSTALL.bat` works in
  `Program Files (x86)`; a damaged registry stops instead of forgetting volumes; mod names are never paths.
- Add-on: only names ending in `_COL` / `_CV` are collision; JPG/TGA, packed and painted textures export; several frame
  effects on one frame are kept; an actor's skin is taken in its armature's space; dialogs no longer reuse the last
  target; Cyrillic names are transliterated.

## 0.9.5 — 2026-10-23
- A collision that replaces the game's keeps the game's parts in the game's order (the world refers to them by number,
  with a surface of its own): the water mill's floors hold again.

## 0.9.4 — 2026-10-06
- Collision cooked by the game's own `NxCooking.dll` through the 32-bit helper `g3cook.exe`; one part per mesh element in
  mesh order; triangles turned the way the game's are. Checked in the game.

## 0.9.3 — 2026-10-05
- The collision tree written the way Gothic 3's PhysX 2.5 reads it.

## 0.9.2 — 2026-10-04
- Models and collision the game takes: the collision file's tail, surfaces by material, version-5 mesh elements with
  vertex colours and a sphere tree, vertex lightmaps for every placement, `_lod1` replaced along.

## 0.9.1 — 2026-10-04
- Frame effects of clips, new clips from a game clip as template, textures whose names differ, licence terms.

## 0.9.0 beta — 2026-10-04
- Export: models (install as a patch volume or a mod package), motions, actors.

## 0.1 — 2026-10-04
- Import: models with textures and collision, characters, animations.
