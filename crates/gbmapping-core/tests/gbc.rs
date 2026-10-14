//! A Game Boy Color ROM, made up: what Pokémon Crystal's map data looks like (map
//! groups, a tilesets table, blocks of 4x4 tiles, compressed pictures, the routines
//! that load the palettes and the roofs), read into the editor's model, edited and
//! written back.

use gbmapping_core::format::block;
use gbmapping_core::gbc::{block_of, quarter};
use gbmapping_core::gfx::color555;
use gbmapping_core::metatiles::GrowError;
use gbmapping_core::pixels::DrawError;
use gbmapping_core::{Edits, Family, Game, OpenError, Profile};

const BANK: usize = 0x4000;
const TOWN: u8 = 1;
const ROUTE: u8 = 2;
const INDOOR: u8 = 3;

struct Synth {
    bytes: Vec<u8>,
    /// Where the first map's blocks, the first tileset's blocks and their collisions are
    blocks: usize,
    tileset_blocks: usize,
    collisions: usize,
}

fn at(bank: usize, address: usize) -> usize {
    bank * BANK + address - BANK
}

/// The colours of the made-up palettes: palette `p`, colour `c`
fn color(p: usize, c: usize) -> u16 {
    ((p * 4 + c + 1) * 211 % 0x7FFF) as u16
}

/// The colours of the tileset that has palettes of its own
fn special(p: usize, c: usize) -> u16 {
    ((p * 4 + c + 1) * 389 % 0x7FFF) as u16
}

/// The roofs' two colours in a map group, by day and by night
fn roof_color(group: usize, night: bool, c: usize) -> u16 {
    (0x4000 + group * 16 + night as usize * 4 + c) as u16
}

/// Data as the games compress it, the plain way: runs of bytes as they are
fn pack(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for run in data.chunks(32) {
        out.push(run.len() as u8 - 1);
        out.extend_from_slice(run);
    }
    out.push(0xFF);
    out
}

/// A tile all of one colour
fn tile(value: usize) -> [u8; 16] {
    let (low, high) = (if value & 1 != 0 { 0xFF } else { 0 }, if value & 2 != 0 { 0xFF } else { 0 });
    std::array::from_fn(|i| if i % 2 == 0 { low } else { high })
}

fn synth() -> Synth {
    let mut bytes = vec![0u8; 8 * BANK];
    let mut put = |o: usize, data: &[u8]| bytes[o..o + data.len()].copy_from_slice(data);
    let word = |address: usize| [address as u8, (address >> 8) as u8];
    put(0x104, &[0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B]);
    put(0x134, b"SYNTH_GBC\0\0TEST");

    // -- bank 1: two groups of maps, then the maps' attributes ---------------------------
    // tileset, environment, place, time of day, attributes' address
    let maps = [(1u8, TOWN, 1u8, 0u8, 0x4020), (2, INDOOR, 1, 1, 0x402C), (1, ROUTE, 2, 2, 0x4038)];
    put(at(1, 0x4000), &[word(0x4004), word(0x4016)].concat());
    for (i, &(tileset, environment, place, time, attributes)) in maps.iter().enumerate() {
        let a = word(attributes);
        put(at(1, 0x4004) + i * 9, &[1, tileset, environment, a[0], a[1], place, 7, time, 0]);
    }
    // border block, height, width, the blocks' bank and address, scripts, events, connections
    let blocks = at(2, 0x4100);
    for (attributes, border, height, width, map_blocks) in
        [(0x4020, 1u8, 2u8, 2u8, 0x4100), (0x402C, 0, 1, 1, 0x4200), (0x4038, 2, 2, 2, 0x4100)]
    {
        let b = word(map_blocks);
        put(at(1, attributes), &[border, height, width, 2, b[0], b[1], 1, 0x00, 0x60, 0x00, 0x61, 0]);
    }
    put(blocks, &[1, 2, 3, 0]);
    put(at(2, 0x4200), &[0]);

    // -- bank 4: the tilesets' pictures, their blocks, the blocks' collisions ------------
    // the first: 0x62 tiles, each of one colour (its number's last two bits), 4 blocks
    let picture: Vec<u8> = (0..0x62).flat_map(|t| tile(t % 4)).collect();
    put(at(4, 0x4000), &pack(&picture));
    let tileset_blocks = at(4, 0x5000);
    let first: Vec<u8> = (1..=16).collect();
    put(tileset_blocks, &[[1u8; 16].as_slice(), &first, &[0x81; 16], &[2; 16]].concat());
    let collisions = tileset_blocks + 4 * 16;
    put(collisions, &[0, 0, 0, 0, 0x07, 0x00, 0x18, 0x29, 0x12, 0x12, 0x12, 0x12, 0x71, 0x72, 0x7A, 0x7B]);
    // the second: 4 tiles, one block of its tile 3
    put(at(4, 0x6000), &pack(&(0..4).flat_map(tile).collect::<Vec<u8>>()));
    put(at(4, 0x6800), &[3; 16]);
    put(at(4, 0x6810), &[0x90, 0x91, 0x93, 0x94]);

    // -- bank 3: the tilesets table, and which palette and tile bank each tile is of ------
    for t in 0..8 {
        let (picture, blocks, collisions, palettes) =
            if t == 2 { (0x6000, 0x6800, 0x6810, 0x4200) } else { (0x4000, 0x5000, 0x5040, 0x4100) };
        let entry =
            [&[4u8][..], &word(picture), &[4], &word(blocks), &[4], &word(collisions), &word(0x4800), &[0, 0], &word(palettes)].concat();
        put(at(3, 0x4400) + t * 15, &entry);
    }
    // the first tileset's tiles: palette = the tile's number, of 8; from 0x80 on, the second tile bank and palette 5
    let palettes: Vec<u8> =
        (0..0x80u8).map(|pair| if pair < 0x40 { (((pair * 2 + 1) % 8) << 4) | ((pair * 2) % 8) } else { 0xDD }).collect();
    put(at(3, 0x4100), &palettes);
    put(at(3, 0x4200), &[0x22; 0x80]);

    // -- bank 5: the palettes and the routine that loads a map's -------------------------
    // "call the special palettes' routine, then by environment and time of day..."
    let special_call = [0x3E, 6, 0x21, 0x00, 0x40, 0xCF, 0x38, 0x00, 0xFA, 0x00, 0xD0];
    let environments = [&[0xE6u8, 0x07, 0x5F, 0x16, 0x00, 0x21][..], &word(0x4200), &[0x19, 0x19, 0x2A, 0x66, 0x6F]].concat();
    let list = [&[0x6Fu8, 0x26, 0x00, 0x29, 0x29, 0x29, 0x11][..], &word(0x4400), &[0x19, 0x5D, 0x54, 0xE1, 0x0E, 0x08]].concat();
    let roofs = [
        &[0x6Fu8, 0x26, 0x00, 0x29, 0x29, 0x29, 0x11][..],
        &word(0x4800),
        &[0x19, 0xFA, 0x00, 0xD0, 0xE6, 0x03, 0xFE, 0x02, 0x38, 0x04, 0x23, 0x23, 0x23, 0x23],
    ]
    .concat();
    put(at(5, 0x4000), &[&special_call[..], &environments, &[0; 8], &list, &[0; 8], &roofs].concat());
    // every environment has the same palettes (8 per time of day), but indoors: always the same 8
    for environment in 0..8 {
        put(at(5, 0x4200) + environment * 2, &word(if environment == INDOOR as usize { 0x4240 } else { 0x4220 }));
    }
    put(at(5, 0x4220), &(0..32).collect::<Vec<u8>>());
    put(at(5, 0x4240), &(0..32).map(|i| 32 + i % 8).collect::<Vec<u8>>());
    for p in 0..40 {
        for c in 0..4 {
            put(at(5, 0x4400) + (p * 4 + c) * 2, &color(p, c).to_le_bytes());
        }
    }
    for group in 0..3 {
        for (i, night) in [false, false, true, true].into_iter().enumerate() {
            put(at(5, 0x4800) + group * 8 + i * 2, &roof_color(group, night, i % 2).to_le_bytes());
        }
    }
    // the places' names: "ld de, names + 2", and four bytes a place
    put(at(5, 0x5000), &[&[0xE5u8, 0xD5, 0xC5, 0x6B, 0x26, 0x00, 0x29, 0x29, 0x11][..], &word(0x5102), &[0x19, 0x2A, 0x66, 0x6F]].concat());
    for (i, name) in [0x5200, 0x5210, 0x5220, 0x5230].into_iter().enumerate() {
        put(at(5, 0x5100) + i * 4, &[0, 0, word(name)[0], word(name)[1]]);
    }
    put(at(5, 0x5200), &[0x92, 0x8F, 0x84, 0x82, 0x88, 0x80, 0x8B, 0x50]); // SPECIAL
    put(at(5, 0x5210), &[0x8D, 0x84, 0x96, 0x7F, 0x81, 0x80, 0x91, 0x8A, 0x1F, 0x93, 0x8E, 0x96, 0x8D, 0x50]); // NEW BARK (break) TOWN
    put(at(5, 0x5220), &[0x91, 0x8E, 0x94, 0x93, 0x84, 0x7F, 0xF8, 0xFF, 0x50]); // ROUTE 29
    put(at(5, 0x5230), &[0x4E, 0x50]); // no name: the table is over

    // -- bank 6: the tilesets with palettes of their own (the second one) -----------------
    // "ld a, [tileset] / cp 2 / jr z, there / jr nothing / there: call loader / scf / ret"
    put(at(6, 0x4000), &[0xFA, 0x00, 0xD0, 0xFE, 2, 0x28, 0x02, 0x18, 0x05, 0xCD, 0x10, 0x40, 0x37, 0xC9, 0xA7, 0xC9]);
    put(at(6, 0x4010), &[0x3E, 0x05, 0x11, 0x00, 0xD0, 0x21, 0x00, 0x41, 0x01, 0x40, 0x00, 0xCD, 0x00, 0x30, 0xC9]);
    for i in 0..32 {
        put(at(6, 0x4100) + i * 2, &special(i / 4, i % 4).to_le_bytes());
    }

    // -- bank 7: the roofs; bank 0: the tilesets that take one (the first) ------------------
    let loader =
        [&[0xFAu8, 0x00, 0xD0, 0x5F, 0x16, 0x00, 0x21][..], &word(0x4100), &[0x19, 0x7E, 0xFE, 0xFF, 0xC8, 0x21], &word(0x4200)].concat();
    put(
        at(7, 0x4000),
        &[&loader[..], &[0x01, 0x90, 0x00, 0xCD, 0x00, 0x30, 0x11, 0xA0, 0x90, 0x01, 0x90, 0x00, 0xCD, 0x00, 0x30, 0xC9]].concat(),
    );
    put(at(7, 0x4100), &[0xFF, 0x00, 0xFF]);
    put(at(7, 0x4200), &tile(3).repeat(9));
    put(0x200, &[0xFE, 1, 0x28, 0x02, 0x18, 0x06, 0x3E, 7, 0x21, 0x00, 0x40, 0xCF]);

    // the cartridge's checksum
    let sum = bytes.iter().enumerate().filter(|(i, _)| !(0x14E..0x150).contains(i)).fold(0u16, |sum, (_, &b)| sum.wrapping_add(b as u16));
    bytes[0x14E..0x150].copy_from_slice(&sum.to_be_bytes());
    Synth { bytes, blocks, tileset_blocks, collisions }
}

fn open(s: &Synth) -> Game {
    Game::open(s.bytes.clone(), "synth.gbc", Profile::default()).unwrap_or_else(|e| panic!("{e}"))
}

/// The four metatiles of a block, as a map has them: left to right, top to bottom
fn quarters(block: usize) -> [u16; 4] {
    std::array::from_fn(|q| quarter(block, q % 2, q / 2))
}

fn rgb(color: u16) -> [u8; 4] {
    let [r, g, b] = color555(color);
    [r, g, b, 255]
}

#[test]
fn a_game_boy_color_rom_is_read_into_the_editors_model() {
    let s = synth();
    let game = open(&s);
    assert_eq!((game.family, game.format.block_size(), game.extension()), (Family::Crystal, 2, "gbc"));
    assert!(game.animations().is_empty(), "a Game Boy Color game's animations aren't looked for");
    assert_eq!((game.rom.header().title.as_str(), game.rom.header().code.as_str()), ("SYNTH_GBC", "TEST"));
    assert_eq!(game.maps.iter().map(|m| m.id.to_string()).collect::<Vec<_>>(), ["0.0", "0.1", "1.0"]);
    // no names but the places': the maps go by them
    assert_eq!(game.maps.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["NEW BARK TOWN", "NEW BARK TOWN", "ROUTE 29"]);
    assert_eq!(game.source(), s.bytes.as_slice());
    assert!(!game.modified());

    // a map is twice as wide and as high as it has blocks: each block is four metatiles
    let map = game.load_map(0).unwrap();
    assert_eq!((map.width, map.height), (4, 4));
    let rows: Vec<&[u16]> = map.blocks.chunks(4).collect();
    let (one, two, three, none) = (quarters(1), quarters(2), quarters(3), quarters(0));
    assert_eq!(rows[0], [one[0], one[1], two[0], two[1]]);
    assert_eq!(rows[1], [one[2], one[3], two[2], two[3]]);
    assert_eq!(rows[2], [three[0], three[1], none[0], none[1]]);
    assert_eq!((map.border.as_slice(), map.border_width, map.border_height), (&one[..], 2, 2), "the border block, whole");
    assert_eq!((game.load_map(1).unwrap().width, game.load_map(1).unwrap().blocks), (2, none.to_vec()));
    // two maps of the same blocks in the ROM have the same ones here
    assert_eq!((game.maps[0].layout.blocks, game.load_map(2).unwrap().blocks), (game.maps[2].layout.blocks, map.blocks.clone()));

    // a metatile: a quarter of a block (its 2x2 tiles, each with the palette that goes
    // with the tile), under a layer of nothing; its behavior is the quarter's collision
    let pair = map.pair;
    assert_eq!(game.metatile_count(pair), 16, "4 blocks: 16 metatiles, and no second tileset's");
    let entries = game.entries(pair, one[0]);
    assert_eq!(entries, [1 | 1 << 12, 2 | 2 << 12, 5 | 5 << 12, 6 | 6 << 12, 0x100, 0x100, 0x100, 0x100]);
    assert_eq!(game.entries(pair, one[3])[..4], [11 | 3 << 12, 12 | 4 << 12, 15 | 7 << 12, 16]);
    assert_eq!(game.entries(pair, two[0])[0], 0x81 | 5 << 12);
    assert_eq!(one.map(|id| game.behavior(pair, id)), [0x07, 0x00, 0x18, 0x29]);
    assert_eq!(game.format.behavior_name(0x18), Some("TALL_GRASS"));
    assert_eq!(
        (game.tile_palette(pair.primary, 0x81), game.tile_palette(pair.primary, 6), game.tile_palette(pair.secondary, 6)),
        (Some(5), Some(6), None)
    );
    for id in 0..16 {
        let (block, qx, qy) = block_of(id);
        assert_eq!((quarter(block, qx, qy), game.locate(pair, id).is_some()), (id, true));
    }
    assert!(game.locate(pair, 16).is_none());
}

#[test]
fn a_map_is_drawn_in_its_own_colours() {
    let s = synth();
    let game = open(&s);
    let (town, house, route) = (game.load_map(0).unwrap(), game.load_map(1).unwrap(), game.load_map(2).unwrap());
    // the same tileset in the ROM, but a town's by day and a route's by night; the house's is another
    assert_eq!(game.pairs().len(), 3);
    assert_ne!(town.pair, route.pair);

    let (day, night) = (game.render_map(&town), game.render_map(&route));
    // tile 1, colour 1, in palette 1 of the time of day's eight
    assert_eq!((day.pixel(0, 0), night.pixel(0, 0)), (rgb(color(8 + 1, 1)), rgb(color(16 + 1, 1))));
    // tile 6 is in the roofs' palette, whose colours 1 and 2 are the map group's
    assert_eq!((day.pixel(8, 8), night.pixel(8, 8)), (rgb(roof_color(1, false, 1)), rgb(roof_color(2, true, 1))));
    // tile 10 is a roof: the town's group has one of its own (colour 3), the route's hasn't (the tileset's: colour 2)
    assert_eq!((day.pixel(8, 16), night.pixel(8, 16)), (rgb(color(8 + 2, 3)), rgb(color(16 + 2, 2))));
    // tile 0x81 is the second of the console's second tile bank: the picture's tile 0x61, colour 1, palette 5
    assert_eq!(day.pixel(32, 0), rgb(color(8 + 5, 1)));
    // the house's tileset comes with its own palettes: tile 3, colour 3, palette 2
    assert_eq!(game.render_map(&house).pixel(0, 0), rgb(special(2, 3)));
    assert!(day.rgba.chunks(4).all(|p| p[3] == 255));

    // the tiles, each in its palette
    let sheet = game.tile_sheet(town.pair, false, &game.tile_usage(), None, 16);
    assert_eq!((sheet.palettes[6], sheet.palettes[0x81], sheet.image.pixel(8, 0)), (6, 5, rgb(color(8 + 1, 1))));
}

#[test]
fn edits_go_back_into_the_rom() {
    let s = synth();
    let mut game = open(&s);
    assert!(game.saved() == s.bytes, "nothing changed: the ROM as it was, byte for byte");

    // a whole block painted: its byte in the ROM, and the cartridge's checksum
    let mut map = game.load_map(0).unwrap();
    for (q, id) in quarters(3).into_iter().enumerate() {
        map.blocks[(2 + q / 2) * 4 + 2 + q % 2] = id;
    }
    game.store_map(&map);
    assert!(game.commit() && game.modified());
    let saved = game.saved();
    let differing: Vec<usize> = (0..saved.len()).filter(|&i| saved[i] != s.bytes[i]).collect();
    assert_eq!(
        (saved[s.blocks + 3], differing.iter().filter(|&&i| !(0x14E..0x150).contains(&i)).collect::<Vec<_>>()),
        (3, vec![&(s.blocks + 3)])
    );
    let sum = saved.iter().enumerate().filter(|(i, _)| !(0x14E..0x150).contains(i)).fold(0u16, |sum, (_, &b)| sum.wrapping_add(b as u16));
    assert_eq!(saved[0x14E..0x150], sum.to_be_bytes());
    // the other map of those blocks shows it too
    assert_eq!(game.load_map(2).unwrap().blocks, map.blocks);
    assert_eq!(game.edited_layouts().len(), 2);
    assert_eq!((game.layout_users(game.maps[0].layout.offset), game.layout_users(game.maps[1].layout.offset)), (vec![0, 2], vec![1]));

    // a quarter alone makes no block: the ROM keeps the block it had
    map.blocks[0] = quarters(3)[0];
    game.store_map(&map);
    game.commit();
    assert_eq!(game.saved()[s.blocks], 1);
    assert!(game.undo().is_some());

    // the border, a block's tile and a quarter's collision
    map = game.load_map(0).unwrap();
    map.border = quarters(2).to_vec();
    game.store_map(&map);
    let id = quarters(3)[3];
    let mut entries = game.entries(map.pair, id);
    entries[3] = 7 | 7 << 12;
    game.set_entries(map.pair, id, &entries);
    game.set_attribute(map.pair, id, 0x33);
    game.commit();
    let saved = game.saved();
    assert_eq!((saved[BANK + 0x20], saved[s.tileset_blocks + 3 * 16 + 15], saved[s.collisions + 3 * 4 + 3]), (2, 7, 0x33));
    // the route's tileset is the same one in the ROM: the tile changed there too
    assert_eq!(game.entries(game.maps[2].layout.pair(), id)[3], 7 | 7 << 12);

    // the edits, through a file, give the same ROM from another copy of it
    let edits = game.collect_edits();
    let mut copy = open(&s);
    let applied = copy.apply_edits(&Edits::from_json(&edits.to_json()).unwrap());
    assert!(applied.skipped.is_empty(), "{:?}", applied.skipped);
    copy.commit();
    assert!(copy.saved() == saved);

    while game.undo().is_some() {}
    assert!(!game.modified() && game.saved() == s.bytes, "all undone: the ROM as it was");
}

#[test]
fn maps_go_by_their_names_from_one_game_to_another() {
    let s = synth();
    let named = |maps: [&str; 3]| {
        let json =
            format!(r#"{{"groups": [{{"name": "A", "maps": ["{}", "{}"]}}, {{"name": "B", "maps": ["{}"]}}]}}"#, maps[0], maps[1], maps[2]);
        Game::open(s.bytes.clone(), "synth.gbc", Profile::from_json(&json).unwrap()).unwrap()
    };
    let original = named(["NewBarkTown", "ElmsLab", "Route29"]);
    // a hack that brought those maps and named them after their region
    let hack = named(["JohtoRoute29", "JohtoNewBarkTown", "JohtoElmsLab"]);
    assert_eq!((0..3).map(|i| original.counterpart(&hack, i)).collect::<Vec<_>>(), [Some(2), Some(0), Some(1)]);
    assert_eq!((0..3).map(|i| hack.counterpart(&original, i)).collect::<Vec<_>>(), [Some(1), Some(2), Some(0)]);
    // the same name comes first; a short name that ends another isn't enough, and the
    // map is then the one of the same place and size
    let other = named(["Route29", "Lab", "JohtoRoute29"]);
    assert_eq!((original.counterpart(&other, 0), other.counterpart(&original, 1)), (Some(2), Some(1)));
}

#[test]
fn not_these_games() {
    let mut bytes = vec![0u8; 4 * BANK];
    bytes[0x104..0x10C].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B]);
    bytes[0x134..0x13B].copy_from_slice(b"TETRIS\0");
    let err = Game::open(bytes, "tetris.gb", Profile::default()).err().unwrap();
    assert_eq!(err, OpenError::NoGameBoyMaps { header: "TETRIS".to_string() });
    assert!(err.to_string().contains("Pokémon Crystal"));
    // the maps' cells are metatiles alone: no collision or elevation of the map's own
    let mut game = open(&synth());
    assert!(game.load_map(0).unwrap().blocks.iter().all(|&b| b == block::metatile(b)));
    // and a tileset keeps the blocks it has
    let pair = game.maps[0].layout.pair();
    assert_eq!(game.add_metatile(pair, None), Err(GrowError::Blocks));
    // nor are its tiles drawn on, or its colours changed: they aren't written back
    assert_eq!(game.draw_on_metatile(pair, 0, 0, &[(0, 0, 1)], true), Err(DrawError::Fixed));
    assert!(!game.set_color(pair, 0, 1, 0x7FFF));
    // nor are whole tilesets made, or given to a map
    assert_eq!(game.duplicate_tileset(pair.primary, true), Err(GrowError::Blocks));
    assert!(!game.set_layout_tileset(game.maps[0].layout.offset, true, game.maps[1].layout.primary) && !game.commit());
    assert!(!game.commit());
}

#[test]
fn blocks_change_places_and_the_maps_follow() {
    let s = synth();
    let mut game = open(&s);
    let pair = game.maps[0].layout.pair();
    let pictures: Vec<_> = (0..3).map(|i| game.render_map(&game.load_map(i).unwrap())).collect();
    // a block is its four quarters: they change places one by one
    let renumbered: usize = (0..4).map(|q| game.swap_metatiles(pair, quarter(1, q % 2, q / 2), quarter(3, q % 2, q / 2)).unwrap()).sum();
    assert!(game.commit());
    // the first map's blocks (which are the third's too) and its border: the others' aren't of that tileset, or have neither block
    assert_eq!(renumbered, 8 + 4);
    assert!((0..3).all(|i| game.render_map(&game.load_map(i).unwrap()) == pictures[i]), "no map looks different, whatever its colours");

    // in the ROM: the two blocks' tiles and collisions, the maps' blocks, the border
    let saved = game.saved();
    assert_eq!((&saved[s.blocks..s.blocks + 4], saved[BANK + 0x20], saved[BANK + 0x38]), (&[3u8, 2, 1, 0][..], 3, 2));
    let tiles = |block: usize| &saved[s.tileset_blocks + block * 16..s.tileset_blocks + block * 16 + 16];
    assert_eq!((tiles(1), tiles(3)), (&[2u8; 16][..], &(1..=16).collect::<Vec<u8>>()[..]));
    assert_eq!(saved[s.collisions + 4..s.collisions + 16], [0x71, 0x72, 0x7A, 0x7B, 0x12, 0x12, 0x12, 0x12, 0x07, 0x00, 0x18, 0x29]);
    // and that ROM opens with maps that look the same
    let again = Game::open(saved, "moved.gbc", Profile::default()).unwrap();
    assert!((0..3).all(|i| again.render_map(&again.load_map(i).unwrap()) == pictures[i]));

    assert!(game.undo().is_some() && game.saved() == s.bytes);
}
