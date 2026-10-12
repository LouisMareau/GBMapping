# GBMapping

A map and tileset editor for Game Boy Advance and Game Boy Color ROMs, written
in Rust. It runs in the browser (WebAssembly, installable as an app) and on
the desktop, from the same code: open a ROM, paint its maps with its own
tilesets, set collision and elevation, edit the metatiles, save the ROM or
just your edits.

The ROM never leaves your computer: it's read and edited in the page.

**Try it:** https://louismareau.github.io/GBMapping/

## Games

The maps are found in the ROM itself, by the shape of its data: no table of
addresses per game, so an original cartridge dump, a build of a decompilation
and a hack all open the same way.

| Game | Codes | State |
| --- | --- | --- |
| Pokémon Emerald | BPE* | checked on the English and French games, and on a build of pokeemerald with 1182 maps |
| Pokémon Ruby, Sapphire | AXV*, AXP* | same data layout as Emerald; not yet tried on a ROM |
| Pokémon FireRed, LeafGreen | BPR*, BPG* | their own layout (640 primary metatiles, 32-bit attributes, sized borders), written from pokefirered and checked on made-up ROMs only; not yet tried on a real one |
| Pokémon Crystal (Game Boy Color) | BYT* | [its blocks as four tiles each](#game-boy-color-games); checked on the French game. Gold and Silver are laid out much the same and not yet tried. A hack that stores its maps another way (Polished Crystal compresses them) isn't read. |

Not there yet: map sizes, events and connections, metatiles of three layers,
the first Game Boy's games.

## Using it

- **Open…** a `.gba` or a `.gbc`, or drop it anywhere on the window. The maps are listed
  by group; type in the search box to find one by name, place or number.
- **Paint** (`P`): pick a metatile in the **Metatiles** tab, or drag there to
  take a rectangle of them, then paint. Right-click the map to pick what's
  there; right-drag to pick a whole piece of it (a house, a pond) with its
  collision, and stamp it elsewhere.
- **Usual collision** (on by default): a metatile taken in the picker is
  painted with the collision and elevation it usually has on that game's
  maps, so a tree blocks and water sits at water level without a second pass.
- **Fill** (`F`) replaces an area of one metatile. **Collision** (`K`): drag
  to make tiles impassable, or passable again. **Elevation** (`L`): drag to
  set it.
- **Grid** (`G`), **Collision** (`C`) and **Elevation** (`E`) overlays; zoom
  with `+` and `-`, or Ctrl + wheel; the middle button drags the map.
- The **border** (the tiles the game repeats around the map) is next to the
  picker: click one of its tiles to set it from the selection.
- **Tileset** tab: the selected metatile's two layers. Click a quarter, then a
  tile below; set flips, palette, behavior (by name) and layer type. The map
  updates at once. The tiles can be those of [any
  tileset](#metatiles-from-any-tileset).
- **Undo** / **Redo** (Ctrl+Z, Ctrl+Y) cover everything, map after map.
- **Save ROM** gives the ROM with your edits. **Export edits** gives a small
  `.json` with only what changed; **Import edits…** (or dropping that file)
  applies it to a ROM, another build of the same game for instance.
- Your edits are also kept by the browser per ROM, and offered again the next
  time that ROM is opened.

### Two views: the map, and what to work from

**Reference** (`R`) opens a second, read-only view next to the map. Both
scroll and zoom together, tile facing tile, and the tile under the pointer is
shown in both.

- By default the reference is **this ROM as it was opened**: every map before
  your edits. Right-click it (or right-drag) to take what was there, with its
  collision, and paint it back where it's needed.
- When the ROM keeps **its maps' originals**, those come first: a hack that
  redraws another game's maps with its own tilesets can keep each map as it
  was, on the tilesets it came with, and say where in its profile (see
  [Profiles](#profiles)). The reference then shows, for each map, that
  original with those tilesets; a map without one is shown as it was opened.
  The source menu switches between the two.
- Or it's **another ROM** (the source menu of the reference, "Another ROM…"):
  the original game, or an earlier build of yours. Its maps are drawn with
  its own tilesets. That's for maps redrawn with other tiles, a region
  brought into a hack and converted to that hack's tilesets for instance: the
  original on one side, the conversion to polish on the other. It can be a
  game of the other console: Crystal's `.gbc` next to the Advance hack that
  has Johto in it. A Game Boy Color block being four tiles, its maps lie tile
  for tile next to the ones made of them.
- **Follow** keeps the reference on the map that goes with the one being
  edited: the map of the same name (or of that name after a region's, a hack
  calling `JohtoNewBarkTown` the `NewBarkTown` it brought), else the map of
  the same place and size (the names are those the ROM can tell). When that
  isn't the right one, choose the map in the list next to it.
- Picking in an original, or in another ROM's reference, can't give its
  tiles: the map being edited isn't on those tilesets. Each is replaced by
  the **closest-looking metatile** of the edited map's tilesets, with the
  collision and elevation the original had. It's a first draft to correct,
  not a conversion.

### Metatiles from any tileset

A metatile is eight tiles, each drawn in one of the map's palettes. The
**Tileset** tab lists the tiles under the metatile:

- **Usual colours** (on by default): a tile has no colours of its own, so each
  is shown in the palette the metatiles use it with the most, and a click
  takes the tile with that palette. Off, every tile is shown in the selected
  quarter's palette instead, to try a tile in other colours: the list then
  changes colours with the quarter picked.
- **All tilesets** lists, under the map's two tilesets, every other tileset
  the ROM's maps use (those that go with the same primary tileset first),
  each with a map that uses it. When the ROM keeps [its maps'
  originals](#the-maps-originals), the tilesets those came with are there
  too: this map's original's right under its own two, the other originals'
  last. A map can only draw its own two tilesets' tiles, so a click on a tile
  of another one **copies it into the map's secondary tileset**, with its
  colours:
  - a palette of the map that already has those colours is used;
  - else they're written to a palette of the secondary tileset that no
    metatile uses;
  - else the tile is put in the closest colours a palette has, and the status
    bar says so.

  A tile the map's tilesets already have isn't copied twice. The copy is one
  step to undo, and goes in the saved ROM and in the edits file. When the
  tileset's tiles no longer fit where they were, they're written to free space
  at the end of the ROM.
- **New** adds an empty metatile at the end of the map's secondary tileset,
  and **Duplicate** adds the copy of the selected one there (its tiles, its
  behavior): a metatile to build without changing any map, since nothing in
  the game uses it yet. A copy is how to rework a metatile for one place and
  leave the maps the original is on as they are, and how a primary tileset's
  metatile gets tiles of the secondary one. A metatile is 16×16 pixels, two
  layers of 2×2 tiles: the game sets that, and bigger things are several
  metatiles (drag in the picker, or right-drag on a map, to take them
  together). What can grow is how many a tileset has: up to 512 for a
  secondary tileset in Emerald (the ids `0x200` to `0x3FF`), 384 in FireRed.
  The first new one moves the tileset's metatiles to free space at the end of
  the ROM, where they then grow.
- **Next free** goes to the next metatile of the secondary tileset that no map
  shows, for when the tileset has all it can. A script may still put such a
  metatile on a map as the game runs (a door that opens, a tree that's cut):
  a new one is the safe choice. The line over the buttons tells how many
  times the selected metatile is on the maps.

The limits: a secondary tileset holds a fixed number of tiles, the last 16 of
which the games keep for the doors they open. One with no room left gives the
tiles no metatile draws, and the status bar says when there's none. The tiles
must be compressed in the ROM, as the games' are, to be added to. A metatile
of the primary tileset (below `0x200` in Emerald), which every map shares,
only takes the primary tileset's tiles: the tab says so when one is selected,
and dims the tiles it can't take. The secondary tileset's metatiles take any.

### Drawing on a metatile

The **Draw** tab shows the selected metatile's bottom or top layer, large: 16×16
pixels to draw on.

- **Pencil** draws in the selected colour, **Eraser** leaves nothing (the layer
  under shows through, or what's behind the map), **Fill** recolours the
  pixels that look like the one clicked and touch it. The right button takes
  the colour under it. A stroke is one step to undo.
- The **colours** are those of the game: each quarter of a layer (a tile) is
  drawn in one of the tilesets' palettes, 15 colours and nothing. The palette
  under the picture is that of the quarter the pointer is on; the number next
  to it gives the quarter another palette. The colour button changes the
  selected colour itself, for everything drawn in that palette.
- **Only this metatile** (on by default): a metatile is made of tiles that
  other metatiles use too, the same one often several times in itself. A
  quarter that shares its tile gets a copy of it when it's first drawn on, new
  in the map's secondary tileset, so nothing else changes. Off, the tiles
  themselves are drawn on, and every metatile that has them shows it: that's
  how to retouch a tile everywhere. A primary tileset's metatile can't have
  tiles of its own: **Duplicate** it and draw on the copy.
- **Show both** shows the other layer too: what's drawn on the bottom layer
  may be under the top one.

Start from **New** for an empty metatile, or from **Duplicate** for a copy of
one that's close.

### A map's tilesets, and where its metatiles are

The **Metatiles** tab shows what a map is drawn with: the metatiles of its
primary tileset, then those of its secondary one. The two are named over the
picker, by where they are in the ROM and the maps that have them.

- The **list** gives the map another of the ROM's tilesets.
- **Duplicate** gives it a copy of the one it has, its own: its tiles, its
  palettes, its metatiles, its animation. The map looks the same, and what's
  then changed in the copy (a metatile rebuilt, a tile drawn on, a colour)
  changes no other map. That's how to rework a tileset for one place when
  thirty maps share it.
- **New** (for the secondary tileset) gives it an empty one, with the palettes
  of the one it had: what the map drew with the old one shows nothing until
  it's redrawn. The old one's tiles are still at hand under **All tilesets**.

A map has its tilesets from its layout: maps drawn from the same layout get
them together, and the status bar says how many. A new tileset is written to
free space at the end of the ROM; one step to undo.

**Move**, over the picker, makes metatiles change places: drag one onto
another, of the same tileset. Every map drawn with that tileset is renumbered
to match (the maps' originals too), so no map looks or plays differently:
it's for putting a tileset in order, the new metatiles next to what they go
with. What names a metatile by its number elsewhere isn't known to the
editor, and isn't changed: a script that places one, the game's code for the
doors it opens. Metatiles made here are named by nothing yet; think twice
before moving the game's own.

### Game Boy Color games

Crystal draws its maps with **blocks** of 4×4 tiles, 32 pixels a side, where
the Advance games have metatiles of 2×2. The editor shows a block as the four
metatiles it is (its quarters), so a map is twice as wide and as high as it
has blocks, and everything above works the same, with these differences:

- **Blocks are painted whole.** The picker shows them four to a row; a click
  or a drag takes whole blocks, on the map too, and Fill replaces an area of
  one block. **Move** makes two blocks change places, the maps following.
- Maps have no collision or elevation of their own: a block's **collision**
  is in its tileset, one per quarter. The Tileset tab names them (`FLOOR`,
  `WALL`, `TALL_GRASS`, `DOOR`…) and sets them.
- The **Tileset** tab edits a block a quarter at a time: click one of its
  four tiles, then a tile of the tileset. A tile comes with its palette (the
  game keeps one per tile, not per block), and there are no flips and no
  second layer.
- A map has one tileset, drawn in **its own colours**: its environment's
  (outdoors, indoors, cave), by day (by night for the maps the game always
  shows so), with its town's roofs. One tileset of the ROM is listed once per
  set of colours its maps give it.
- The ROM draws many houses, gates and Pokémon Centers from the **same
  blocks**: painting one paints them all, and the editor says how many maps
  share them.
- **Save ROM** gives the `.gbc` with the blocks, tiles and collisions that
  changed written back, and its checksum set right. Nothing else in it is
  touched: saved without an edit, it's the file that was opened, byte for
  byte.

Tile pictures and palettes aren't edited, a tileset keeps the blocks it has,
and a map the tileset it has.

### As an app

In Chrome or Edge, the address bar offers to **install** GBMapping: it then
has its own window and icon, works offline, and can be chosen to open `.gba`
files. A desktop program is built from the same code (see below): it takes
files on its command line, `gbmapping game.gba profile.json`.

## Profiles

A ROM knows its places' names ("PETALBURG CITY" for the city, its gym and its
houses alike), not the names its maps have in a decompilation. For the original
games those names are built in (`PetalburgCity_Gym`). For your own project, a
profile `.json` dropped with the ROM gives them:

```json
{
  "game": "pokeemerald",
  "name": "My hack",
  "groups": [{"name": "gMapGroup_TownsAndRoutes", "maps": ["PetalburgCity", "SlateportCity"]}]
}
```

Optional fields, for the ROM the search gets wrong: `gMapGroups` and
`gRegionMapEntries` (ROM offsets, decimal), `mapsec16` (map headers whose
region map section is 16 bits wide: the headers usually tell it themselves),
`regionNames`.

### The maps' originals

A hack that brings another game's maps and redraws them with its own
tilesets can keep, for each, the map as it was: a layout no map uses, on the
tilesets the map came with. The profile tells the editor where, and the
[reference view](#two-views-the-map-and-what-to-work-from) shows it next to
the map:

```json
{
  "gMapLayouts": 8069344,
  "groups": [{"name": "gMapGroup_Kanto", "maps": [
    {"name": "PalletTown", "layout": 602, "original": 878},
    {"name": "Route1", "layout": 617}]}]
}
```

`gMapLayouts` is the ROM offset of the layouts table; `original` is the id of
the layout that keeps the map as it was (the table's entry `id - 1`, as the
games count their layouts). `layout`, the map's own, is there for the reader:
the ROM tells it. An original is only looked at: nothing is ever written to
it, and it isn't part of the edits files.

## Edits files

```json
{"format": "GBMapping/1", "game": "BPEE", "title": "POKEMON EMER",
 "maps": {"0.9": {"width": 20, "height": 20, "blocks": [], "border": []}},
 "tilesets": {"0x3DF704": {"metatiles": {"12": [1, 2, 3, 4, 5, 6, 7, 8]}, "attributes": {"12": 3},
                           "tiles": {"498": "0011…"}, "palettes": {"12": [0, 32767]}}}}
```

Maps go by `group.number` (a layout several maps share is told once), tilesets
by ROM address, metatiles by their number in the tileset. A block is
`metatile | collision << 10 | elevation << 12`; a metatile is eight
`tile | hflip << 10 | vflip << 11 | palette << 12`.

A Game Boy Color game's edits are told in the editor's terms: a map twice as
wide and as high as it has blocks, each of its tiles the quarter of a block,
`((block / 4) * 2 + y) * 8 + (block % 4) * 2 + x` for the quarter at `x`, `y`
(0 or 1). Its tilesets go by where the editor put them, which is the same
each time that ROM is opened: such a file is for that ROM.

A metatile numbered past the tileset's last one is a new one: applying the
file gives the tileset the metatiles up to it. What rebuilds a game from its
sources has to add them to the tileset's metatiles and attributes the same
way.

A map given another tileset says which: `"primary"` or `"secondary"`, the
tileset's address. A tileset made in the editor has `"new": {"secondary":
true, "copy_of": "0x3DFBB4"}` and is then told whole (every metatile, every
tile that isn't empty, its 16 palettes), `copy_of` being the tileset it was
made as a copy of, when it was. Applied, it's made anew wherever that ROM
has room, and the maps that had it are given it there. `"originals"` holds,
by map, the maps' originals that were renumbered because metatiles changed
places.

`tiles` and `palettes` are only there when tiles were drawn on or brought from
another tileset, or a colour changed: a tile's 32 bytes (4 bits a pixel, as
the ROM has them once unpacked) in hexadecimal, by its number in the
tileset, and a palette's 16 colours (15 bits each) by its number. What reads
an edits file to rebuild a game from its sources has to write those too, or
the metatiles point at tiles that aren't there.

## Building

Rust (stable) with the `wasm32-unknown-unknown` target, and
[Trunk](https://trunkrs.dev) for the web build.

```bash
trunk serve
```

serves the editor on http://127.0.0.1:8080 and rebuilds on every change;
`trunk build --release` writes the site to `dist/`, ready for any static host.

```bash
cargo run --release -p gbmapping
```

runs the desktop program.

| | |
| --- | --- |
| `crates/gbmapping-core` | the ROM: finding the tables, reading maps and tilesets, drawing, editing, undo, edits files; `gbc` reads a Game Boy Color ROM into the same model and writes the edits back. No UI, no I/O. |
| `crates/gbmapping` | the editor, with [egui](https://github.com/emilk/egui); its page, manifest and service worker for the web |
| `tools/` | generators (behavior names, map names, icons) and the browser check |

### Checks

```bash
cargo test -p gbmapping-core
```

runs the core's tests on made-up ROMs. Real ones can't be in the repository;
name yours and they're opened, drawn and edited too. A ROM's profile goes
after it, behind a `|`: every original it tells is then read and checked
(its own tilesets, every metatile it uses, the map's size). A `.gbc` has its
own check: read, drawn in colours, saved as it was opened, then with a block
painted:

```bash
GBMAPPING_TEST_ROMS="emerald.gba;my-hack.gba|my-hack.json;crystal.gbc" cargo test -p gbmapping-core --test roms -- --nocapture
```

`cargo run -p gbmapping-core --example inspect -- game.gba 0.9 map.png` lists
a ROM's maps and draws one, without the editor (`original` before the
picture's name draws the map's original).

```bash
node tools/browser/check.mjs game.gba [profile.json] [reference.gbc [map]]
```

checks the built site in a headless Chrome, end to end: a ROM in through the
file chooser, tiles painted with the mouse, undo, the saved ROM and the
exported edits compared with what they should be, tiles taken back from the
reference, a map's original shown on its own tilesets when the profile tells
one, a tileset copied for a map, two metatiles made to change places, a
metatile duplicated and drawn on, a tile of another tileset put in a
metatile, each undone, the edits kept across a reload, the app installable
and working offline. Screenshots land in
`target/browser-check/`. A second ROM is opened as the reference of the first,
with the first's map `map` on screen: Crystal next to the hack that has its
maps, `JohtoNewBarkTown` for instance. A `.gbc` given first is checked on its
own: a block picked, painted, undone, and the saved ROM compared.

### Publishing

`.github/workflows/site.yml` tests, builds and publishes the site to GitHub
Pages on every push to `main` (the repository's Pages source is "GitHub
Actions").

GBMapping was first written in plain JavaScript; that version is in the
repository's history (commit `64acd01` is the last to hold it). The Rust
editor reads and writes the same edits files, and offers again the edits the
first version kept in the browser.
