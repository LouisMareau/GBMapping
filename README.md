# GBMapping

A map and tileset editor for Game Boy Advance ROMs, written in Rust. It runs in
the browser (WebAssembly, installable as an app) and on the desktop, from the
same code: open a ROM, paint its maps with its own tilesets, set collision and
elevation, edit the metatiles, save the ROM or just your edits.

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

Not there yet: tile graphics and palettes (shown, not edited), map sizes,
events and connections, metatiles of three layers, the Game Boy Color games.

## Using it

- **Open…** a `.gba`, or drop it anywhere on the window. The maps are listed
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
  updates at once.
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
- Or it's **another ROM** (the source menu of the reference, "Another ROM…"):
  the original game, or an earlier build of yours. Its maps are drawn with
  its own tilesets. That's for maps redrawn with other tiles, a region
  brought into a hack and converted to that hack's tilesets for instance: the
  original on one side, the conversion to polish on the other.
- **Follow** keeps the reference on the map that goes with the one being
  edited: the map of the same name, else the map of the same place and size
  (the names are those the ROM can tell). When that isn't the right one,
  choose the map in the list next to it.
- Picking in another ROM's reference can't give its tiles: they aren't in
  your ROM. Each is replaced by the **closest-looking metatile** of the
  edited map's tilesets, with the collision and elevation the original had.
  It's a first draft to correct, not a conversion.

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

## Edits files

```json
{"format": "GBMapping/1", "game": "BPEE", "title": "POKEMON EMER",
 "maps": {"0.9": {"width": 20, "height": 20, "blocks": [], "border": []}},
 "tilesets": {"0x3DF704": {"metatiles": {"12": [1, 2, 3, 4, 5, 6, 7, 8]}, "attributes": {"12": 3}}}}
```

Maps go by `group.number` (a layout several maps share is told once), tilesets
by ROM address, metatiles by their number in the tileset. A block is
`metatile | collision << 10 | elevation << 12`.

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
| `crates/gbmapping-core` | the ROM: finding the tables, reading maps and tilesets, drawing, editing, undo, edits files. No UI, no I/O. |
| `crates/gbmapping` | the editor, with [egui](https://github.com/emilk/egui); its page, manifest and service worker for the web |
| `tools/` | generators (behavior names, map names, icons) and the browser check |

### Checks

```bash
cargo test -p gbmapping-core
```

runs the core's tests on made-up ROMs. Real ones can't be in the repository;
name yours and they're opened, drawn and edited too:

```bash
GBMAPPING_TEST_ROMS="emerald.gba;my-hack.gba" cargo test -p gbmapping-core --test roms -- --nocapture
```

`cargo run -p gbmapping-core --example inspect -- game.gba 0.9 map.png` lists
a ROM's maps and draws one, without the editor.

```bash
node tools/browser/check.mjs game.gba
```

checks the built site in a headless Chrome, end to end: a ROM in through the
file chooser, tiles painted with the mouse, undo, the saved ROM and the
exported edits compared with what they should be, tiles taken back from the
reference, the edits kept across a reload, the app installable and working
offline. Screenshots land in `target/browser-check/`.

### Publishing

`.github/workflows/site.yml` tests, builds and publishes the site to GitHub
Pages on every push to `main` (the repository's Pages source is "GitHub
Actions").

GBMapping was first written in plain JavaScript; that version is in the
repository's history (commit `64acd01` is the last to hold it). The Rust
editor reads and writes the same edits files, and offers again the edits the
first version kept in the browser.
