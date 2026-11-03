# Changelog

## Unreleased
- The actor export lays an actor out as the game's: the default material first and the others numbered from 1, the
  material map after the FXA and the `.xshmat` table matching the new mesh (before, the map and table stayed the
  template's while the materials moved by one). An actor that carries its levels of detail gets the new mesh in each,
  and replacing one also replaces its `_lod1`/`_lod2` beside it.
- **Export → Gothic 3 New Creature → Mod**: an armature of your own with its mesh and actions becomes a creature:
  `G3_<Word>_Body_01.xact` (`_Skeleton` without a mesh) and clips `<Word>_<words of the game>.xmot`, as the engine ties
  clips to actors. Bones are checked against what the game's code expects (`<Word>_Head_Head_1`, `<Word>_Spine_Spine_1`,
  `<Word>_Left_…`/`<Word>_Right_…`, the `Slot_…` bones); actions not named from the game's words are left out. Bones can
  carry rotation limits and physics shapes (`g3_limit`, `g3_shape`). The creature needs a template to stand in the world.
- Actors and clips are written from their parts: every one of the game's 379 actors and 5 752 clips, read into its
  fields and written again, is the same file to the byte; a clip written from its tracks alone keeps keys only where
  the motion bends, as the game's (within 0.01 cm and 0.06° of the game's own clips).
- **Import → Gothic 3 Terrain**: landscape cells by name or around the 3D cursor, as one mesh welded across the
  cells' borders, with all a cell carries: the game's materials, its uv map, its colours as corner colour attributes
  (`g3_diffuse`, whose alpha blends the ground layers, and `g3_light`, the baked brightness), Varant's three further uv
  sets, and the cell of every face (`g3_cell`). The cells come as the game ships them, without any mod.
- **Export → Gothic 3 Terrain → Mod**: only the cells that changed are written (mesh, collision from their own
  triangles, lightmap); a cell that comes back as it went out is left to the game. Painted colours go along as they are.
- Characters with an **animated head** (`G3_Head_…_Animated_…`, 28 heads) bring its face bones (lips, jaw, brows: 22)
  onto the body's skeleton, where they move the face; before, they were all put on the head bone.
- **Import → Gothic 3 Lip-sync**: a voice line of the game (search by name, per language) on the selected character's
  animated head, as an action on its face bones only, with the line's voice in the Video Sequencer. Gothic 3's faces
  move by bones: each animated head has a clip of 15 poses (`<head>_LipSync`), each line a timeline of phones
  (`.xlip`, in the speech archive), and a phone shows its pose.
- The unknown u32 at 32 of an actor is the size of the source it was compiled from: the FXA's length in every actor
  compiled since August 2006 (the 66 that differ are all older, and they are exactly the actors that carry their
  `_lod1`/`_lod2` inside the same file). The actor export writes that, as before; now a test
  holds it.
- Lip-sync follows the game's own rule (Engine.dll builds a line's motion from key number `phone` of every key array
  of the head's clip with at least 14 keys; shorter ones stay still), so a few short position tracks no longer move.
- **Export → Gothic 3 Lip-sync → Mod**: a face action back to a line's `.xlip` (`Speech_<Language>`): at every key the
  nearest of the head's poses. The writer gives every one of the game's 24 241 `.xlip` back to the byte (the German
  patch's three in their plain form too).
- Motion export **from the game's words**: a new clip's name is picked from lists of the words Gothic 3 makes clip names
  of (stance, left and right hand, pose, action, phase, N/O/I, direction, variant; read from the tables Game.dll and
  Script.dll fill at start-up), the actor and the clip's own part after `%` from the template. The free name field is
  gone: a name the game never composes cannot be typed. Every clip the game ships is named from those tables, but for
  about a hundred whose words no code of the game holds (the composer never makes them).
- Motion export **as a new variant** of the clip's action: the next free variant number (the wolf's ambient loops
  01–05 → 06), named the way the game composes clip names. The report says what the game makes of a new name: a
  variant, words the game never composes, or a name it does not look up.
- Known now, not built: a clip belongs to the actor its name's first word names (Engine.dll
  `eCWrapper_emfx2Motion::CleanUpMotions`: `G3_<word>_Skeleton.FXA`, else `G3_<word>_Body_01.FXA`; a test holds it on
  the game's clips). A new creature still needs a template the game places it by, and our actor and clip writers keep
  the template's skeleton: what is missing is in STATUS.
- A painted cell repaints the **distant land's picture** over it (the 1024² DXT1 atlas of its distant material): each
  texel moves by what a colour model says the paint changed there (per ground material, colour from layer blend and
  baked brightness, learned from the game's own picture over the shipped cells around), and only the 4×4 blocks
  holding such texels are encoded again; every other byte stays the game's. Asked for a cell it never saw, the model
  is within 2.6 of 255 of the game's picture on average (a flat colour: 3.6).
- The terrain export brings the **distant land** along: the 300 m tiles the game shows far away
  (`G3_World_Landscape_Lowpoly_*`) over moved ground take the edit's height difference, their triangles there split
  to 8 m (shared across neighbours, so no cracks; uvs, colours and the second uv set from the triangle they lie in).
  The rest of a tile does not move.
- **Trees** in the World Layer and Terrain imports: every SpeedTree of the static cells around (all 57 380 of the
  game's trees are placed there) as a stand-in to look at — a bark cylinder and three crossed cards of the game's leaf
  atlas (alpha test 64), as tall as the tree's recipe (`.spt`, size in metres), placed and scaled like the game's. In a
  collection of their own, marked `g3_view_only`; the model export leaves them out. A `.spt` is only a recipe, so the
  real tree is not built.
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
