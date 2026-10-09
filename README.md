# GBMapping

A map and tileset editor for Game Boy Advance (and, later, Game Boy Color)
ROMs, in the browser: drop a ROM in, paint its maps with metatiles, set
collision and elevation, edit the metatiles themselves (tiles, flips,
palettes, behavior), save the ROM or just your edits. Nothing leaves your
browser.

**Try it:** https://louismareau.github.io/GBMapping/ (or open
`index.html` from a local web server: `python -m http.server`).

## Games

| Game | Code | Maps | Tilesets | Notes |
| --- | --- | --- | --- | --- |
| Pokémon Emerald (USA/Europe) | BPEE | yes | metatiles, attributes | tile graphics are LZ77-compressed in the ROM: not editable yet |
| Hacks built from pokeemerald | BPEE | yes | yes | drop a profile .json with the ROM (see below) |

More games are a profile each (`src/games/`): a profile knows where the ROM
keeps its map table, how a map header, a layout and a tileset are laid out,
and how to decode graphics and text. FireRed/LeafGreen and Ruby/Sapphire
(same family, other offsets and header layouts) and the Game Boy Color games
(Crystal's 4×4 blocks, banked pointers) are the next ones.

## Using it

- Drop a `.gba` on the page. The game is told by the ROM header; its maps
  are listed by group with the names of their region map sections.
- **Metatiles** tab: click one (or drag a rectangle to stamp several), paint
  on the map; right-click the map to pick what's there. The border (the four
  tiles around the map) is painted the same way.
- **Collision** and **Elevation** tools set a tile impassable or not, or its
  elevation; the checkboxes overlay them (keys: `g` grid, `c` collision,
  `e` elevation, Ctrl+Z/Y undo/redo).
- **Tileset** tab: the selected metatile's two layers; click a quadrant,
  then a tile below (or type its number), set flips, palette, behavior and
  layer type. The map and the picker update at once.
- **Save ROM** downloads the ROM with the edits. **Export edits** downloads a
  small `.json` with only what changed (maps by group.number, tilesets by
  ROM address); **Import edits** applies such a file to a ROM. The same
  edits are kept in the browser per ROM and offered again when that ROM is
  dropped.

## Profiles

A hack moves things around. A profile `.json` dropped with the ROM says
where they are and what the maps are called:

```json
{
  "game": "pokeemerald",
  "name": "My hack",
  "gMapGroups": 7006500,
  "gRegionMapEntries": 8585828,
  "mapsec16": true,
  "groups": [{"name": "gMapGroup_TownsAndRoutes", "maps": [{"name": "PetalburgCity"}, "..."]}]
}
```

Addresses are ROM offsets (decimal or hex strings aren't both accepted:
decimal). `mapsec16` is for headers whose region map section is 16 bits.
Without a profile the editor uses the vanilla game's addresses and looks for
the region map names by scanning for "LITTLEROOT TOWN", which works for most
hacks too.

## Development

Plain ES modules, no build step. `node test/parse.mjs <rom> [profile.json]
[map id] [out.ppm]` lists a ROM's maps and renders one, without a browser.
