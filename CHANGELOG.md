# Changelog

## Unreleased
- New materials take the game's material of their look, read from the Blender nodes: opaque, with a specular map,
  alpha test (with or without specular), see-through blend (Alpha straight from the image, or a game material with
  blend mode 2: glass, water, haze) and glow (an emission image: additive). Each template is the most common make among
  the game's own materials of that look; a normal or specular map the template samples and the Blender material lacks is
  written flat / dull instead of showing the template's own.
- An actor's new materials are skinned ones (`TransformationType` 2, as 192 of the 193 materials the game's actors
  use); before, they were static materials.
- A model's further coordinate sets come into Blender as UV layers and go back out: `G3_UV2`…`G3_UV4` (the second to
  fourth texture sets of Varant's landscape and some buildings, which export used to drop) and `G3_Lightmap` (the
  coordinates of the lightmap pages).
- A building lit by lightmap pages keeps them when it is exported with its shape unchanged: each part that is one of
  the game's elements again goes back in the game's vertex and triangle order, and every placement's lightmap is
  written as the game's, pages and all (byte for byte from Blender). A changed part is lit per vertex, as before.
- A replaced building replaces its far version too (`<name>_LOWPOLY`, which the world's distance layer places by file
  name): with `*_LOWPOLY` objects when the selection has them (or the model has them as children), else with the model
  itself. A new model with `*_LOWPOLY` objects gets `gothic3_impexp/<name>_LOWPOLY.xcmsh` for a world editor to place.
- Your own texture folders (add-on preferences, with subfolders): on import a PNG, TGA or DDS there stands in for the
  game's texture of that name or one the game lacks; on export an image whose file moved is found there by name.
- Every model export writes `<name>.placement.json` for a world editor (into a package, and into the cache): the mesh,
  its box, the far version, and per collision part the `eCCollisionShape` an entity placing it needs — file, part
  index, surface, box and the values the game's own entities give such shapes. For the water mill it names exactly the
  shapes the world's entity for it has.
- A mod volume that names its files by full paths, as the game's do, is still read as a mod when all its files are
  later than the game's (the newest file of the game's volumes, patches included, is from January 2008).
- A changed part of a building lit by lightmap pages gets pages of its own instead of falling back to vertex
  lighting: charts (planar groups of its triangles, with their lightmap coordinates) in the mesh, and in every
  placement's lightmap a page per chart at that placement's texel size, its light taken from the old pages at the same
  place. Unchanged parts now also keep the game's charts in the mesh, which the pages they keep belong to (the last
  release wrote them without).
- Import keeps the game's double-sided faces (a second face on the same three corners), which Blender's OBJ importer
  dropped.

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
