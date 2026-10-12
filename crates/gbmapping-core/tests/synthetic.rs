//! A tiny made-up ROM per family, laid out like the games lay theirs: everything the
//! core does, checked without a copyrighted byte.

use gbmapping_core::format::block;
use gbmapping_core::text;
use gbmapping_core::tiles::{BorrowError, Borrowed, Colors};
use gbmapping_core::{closest_metatile, Edits, Family, Game, Image, MapId, OpenError, Profile};

const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;

struct Builder {
    bytes: Vec<u8>,
    kanto: bool,
    /// Map headers with a 16-bit region map section, as some hacks have
    wide: bool,
}

impl Builder {
    fn new(code: &str, kanto: bool) -> Self {
        let mut bytes = vec![0u8; 0x200];
        bytes[0xA0..0xA8].copy_from_slice(b"SYNTHROM");
        bytes[0xAC..0xAC + code.len()].copy_from_slice(code.as_bytes());
        bytes[0xB2] = 0x96;
        Self { bytes, kanto, wide: false }
    }

    /// Where the next thing goes (word-aligned)
    fn here(&mut self) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        self.bytes.len()
    }

    fn put(&mut self, data: &[u8]) -> usize {
        let at = self.here();
        self.bytes.extend_from_slice(data);
        at
    }

    fn words(&mut self, words: &[u32]) -> usize {
        let data: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        self.put(&data)
    }

    fn halves(&mut self, halves: &[u16]) -> usize {
        let data: Vec<u8> = halves.iter().flat_map(|h| h.to_le_bytes()).collect();
        self.put(&data)
    }

    fn palettes(&mut self, colors: &[(usize, usize, u16)]) -> usize {
        let mut pals = [0u16; 256];
        for &(palette, index, color) in colors {
            pals[palette * 16 + index] = color;
        }
        self.halves(&pals)
    }

    /// Metatiles with their attributes right after, as the games have them
    fn metatiles(&mut self, metatiles: &[[u16; 8]], attributes: &[u32]) -> (usize, usize) {
        let at = self.halves(&metatiles.concat());
        let attrs =
            if self.kanto { self.words(attributes) } else { self.halves(&attributes.iter().map(|&a| a as u16).collect::<Vec<_>>()) };
        (at, attrs)
    }

    fn tileset(&mut self, compressed: bool, secondary: bool, tiles: usize, palettes: usize, metatiles: usize, attributes: usize) -> usize {
        let p = |o: usize| 0x0800_0000 + o as u32;
        let callback = 0;
        let (a, b) = if self.kanto { (callback, p(attributes)) } else { (p(attributes), callback) };
        self.words(&[compressed as u32 | (secondary as u32) << 8, p(tiles), p(palettes), p(metatiles), a, b])
    }

    #[allow(clippy::too_many_arguments)]
    fn layout(
        &mut self,
        width: u32,
        height: u32,
        border: &[u16],
        border_size: (u8, u8),
        blocks: &[u16],
        primary: usize,
        secondary: usize,
    ) -> usize {
        let p = |o: usize| 0x0800_0000 + o as u32;
        let border = self.halves(border);
        let blocks = self.halves(blocks);
        let at = self.words(&[width, height, p(border), p(blocks), p(primary), p(secondary)]);
        if self.kanto {
            self.put(&[border_size.0, border_size.1, 0, 0]);
        }
        at
    }

    fn header(&mut self, layout: usize, mapsec: u8) -> usize {
        let p = |o: usize| 0x0800_0000 + o as u32;
        let scripts = self.put(&[0, 0, 0, 0]);
        if self.wide {
            // the section on two bytes, then cave, weather, and the map's type a byte later
            return self.words(&[p(layout), 0, p(scripts), 0, 0x0001_012C, mapsec as u32, 8]);
        }
        self.words(&[p(layout), 0, p(scripts), 0, 0x0001_012C, mapsec as u32 | 2 << 24, 0])
    }
}

/// How much unused space the made-up ROMs end with
const FREE: usize = 0x2000;

struct Synth {
    bytes: Vec<u8>,
    groups_table: usize,
    /// The layouts table: the first map's layout, the two others', nothing, and the first map as it was before
    layouts_table: usize,
    primary: usize,
    secondary: usize,
    first_secondary: u16,
}

/// 3 maps in 2 groups: 0.0 (3x2), then 0.1 and 1.0 drawn from one layout (2x2)
fn synth(code: &str, kanto: bool) -> Synth {
    synth_with(code, kanto, false)
}

fn synth_with(code: &str, kanto: bool, wide: bool) -> Synth {
    let mut b = Builder::new(code, kanto);
    b.wide = wide;
    let p = |o: usize| 0x0800_0000 + o as u32;
    let (primary_tiles, primary_pals) = if kanto { (640u16, 7usize) } else { (512, 6) };

    // primary: tile 0 blank, tile 1 all color 1, tile 2 all color 2 (not compressed)
    let mut tiles = vec![0u8; 96];
    tiles[32..64].fill(0x11);
    tiles[64..96].fill(0x22);
    let primary_tiles_at = b.put(&tiles);
    // secondary: tile 0 all color 1, LZ77 (literals only)
    let mut lz = vec![0x10, 32, 0, 0];
    for _ in 0..4 {
        lz.push(0);
        lz.extend_from_slice(&[0x11; 8]);
    }
    let secondary_tiles_at = b.put(&lz);
    let primary_pals_at = b.palettes(&[(0, 1, RED), (0, 2, GREEN), (primary_pals, 1, GREEN)]);
    let secondary_pals_at = b.palettes(&[(0, 1, BLUE), (primary_pals, 1, BLUE)]);

    let solid =
        |tile: u16, palette: u16| [tile | palette << 12, tile | palette << 12, tile | palette << 12, tile | palette << 12, 0, 0, 0, 0];
    let (pm, pa) = b.metatiles(&[[0; 8], solid(1, 0), solid(2, 0)], &[0, 1, 2]);
    let (sm, sa) = b.metatiles(&[solid(primary_tiles, primary_pals as u16), [0; 8]], &[3, 0]);
    let primary = b.tileset(false, false, primary_tiles_at, primary_pals_at, pm, pa);
    let secondary = b.tileset(true, true, secondary_tiles_at, secondary_pals_at, sm, sa);

    let first_secondary = primary_tiles; // metatile ids split where tile ids do
    let big = [1, 2, first_secondary, block::pack(1, 1, 3), block::pack(2, 0, 3), first_secondary + 5];
    let border: Vec<u16> = if kanto { vec![1; 6] } else { vec![1; 4] };
    let layout_a = b.layout(3, 2, &border, (3, 2), &big, primary, secondary);
    let layout_b = b.layout(
        2,
        2,
        &border,
        (3, 2),
        &[block::pack(1, 0, 3), block::pack(1, 0, 3), block::pack(1, 1, 3), block::pack(2, 0, 1)],
        primary,
        secondary,
    );
    // the first map as it was before, on a pair of its own (the secondary tileset twice): a layout no map uses
    let before = [first_secondary, first_secondary, 1, 0, 0, first_secondary + 1];
    let layout_before = b.layout(3, 2, &border, (3, 2), &before, secondary, secondary);
    let layouts_table = b.words(&[p(layout_a), p(layout_b), 0, p(layout_before), 0x1234_5678]);
    let section = if kanto { 0x58 } else { 0 };
    let headers = [b.header(layout_a, section + 2), b.header(layout_b, section + 3), b.header(layout_b, section + 3)];
    let group0 = b.words(&[p(headers[0]), p(headers[1])]);
    let group1 = b.words(&[p(headers[2])]);
    let groups_table = b.words(&[p(group0), p(group1), 0]);

    // 40 places
    let names: Vec<usize> = (0..40)
        .map(|i| {
            let mut name = text::encode(&format!("PLACE {i}"));
            name.push(text::TERMINATOR);
            b.put(&name)
        })
        .collect();
    b.words(&[0]);
    for (i, &name) in names.iter().enumerate() {
        if kanto {
            b.words(&[p(name)]);
        } else {
            b.words(&[i as u32 | 1 << 16 | 1 << 24, p(name)]);
        }
    }
    b.words(&[0, 0]);
    // the unused end of a cartridge
    b.bytes.extend_from_slice(&[0xFF; FREE]);
    Synth { bytes: b.bytes, groups_table, layouts_table, primary, secondary, first_secondary }
}

fn open(s: &Synth) -> Game {
    Game::open(s.bytes.clone(), "synth.gba", Profile::default()).expect("the synthetic ROM opens")
}

fn both() -> [(Synth, Family); 2] {
    [(synth("BPEE", false), Family::Emerald), (synth("BPRE", true), Family::FireRedLeafGreen)]
}

#[test]
fn opens_and_lists_the_maps() {
    for (s, family) in both() {
        let game = open(&s);
        assert_eq!(game.family, family);
        assert_eq!(game.groups.table, s.groups_table);
        let ids: Vec<String> = game.maps.iter().map(|m| m.id.to_string()).collect();
        assert_eq!(ids, ["0.0", "0.1", "1.0"]);
        let names: Vec<&str> = game.maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["PLACE 2", "PLACE 3", "PLACE 3"], "{family:?}");
        assert_eq!(game.maps[0].group_name, "Group 0");
        assert_eq!((game.maps[0].layout.width, game.maps[0].layout.height), (3, 2));
        assert_eq!(game.layout_users(game.maps[1].layout.offset), [1, 2]);
        assert_eq!(game.pairs().len(), 1);
        assert_eq!(game.tilesets().len(), 2);
        let map = game.load_map(0).unwrap();
        let border = if family == Family::FireRedLeafGreen { (3, 2) } else { (2, 2) };
        assert_eq!((map.border_width, map.border_height), border);
        assert_eq!(map.border.len(), border.0 * border.1);
        assert_eq!(game.metatile_count(map.pair), s.first_secondary as usize + 2);
        assert!(!game.modified());
    }
}

#[test]
fn the_family_is_told_by_the_structures_when_the_code_is_unknown() {
    for (kanto, family) in [(false, Family::Emerald), (true, Family::FireRedLeafGreen)] {
        let game = open(&synth("ZZZZ", kanto));
        assert_eq!(game.family, family);
        assert_eq!(game.maps.len(), 3);
    }
    // a misleading code doesn't hide the maps either
    assert_eq!(open(&synth("BPEE", true)).family, Family::FireRedLeafGreen);
    assert_eq!(open(&synth("AXVE", false)).family, Family::RubySapphire);
}

#[test]
fn not_a_game() {
    assert_eq!(Game::open(Vec::new(), "x", Profile::default()).err(), Some(OpenError::Empty));
    let err = Game::open(vec![0; 0x1000], "x", Profile::default()).err().unwrap();
    assert_eq!(err, OpenError::NoMaps { header: String::new(), gba: false });
    assert!(err.to_string().contains("isn't a Game Boy Advance ROM"));
}

#[test]
fn draws_the_metatiles() {
    for (s, family) in both() {
        let game = open(&s);
        let map = game.load_map(0).unwrap();
        let image = game.render_map(&map);
        assert_eq!((image.width, image.height), (48, 32));
        assert_eq!(image.pixel(0, 0), [255, 0, 0, 255], "{family:?}: the primary's tile 1");
        assert_eq!(image.pixel(16, 0), [0, 255, 0, 255], "the primary's tile 2");
        assert_eq!(image.pixel(32, 0), [0, 0, 255, 255], "the secondary's tile and palette");
        assert_eq!(image.pixel(47, 31), [255, 0, 255, 255], "a metatile id that holds nothing");
        let sheet = game.render_sheet(map.pair, 16);
        assert_eq!(sheet.width, 256);
        assert_eq!(sheet.pixel(16, 0), [255, 0, 0, 255]);
        assert_eq!(sheet.pixel(0, 0), [0, 0, 0, 255], "the backdrop");
        let (x, y) = ((s.first_secondary as usize % 16) * 16, (s.first_secondary as usize / 16) * 16);
        assert_eq!(sheet.pixel(x, y), [0, 0, 255, 255]);
        let layer = game.render_layer(map.pair, &game.entries(map.pair, 1), 1);
        assert_eq!(layer.pixel(0, 0), [0, 0, 0, 0], "an empty top layer is see-through");
        assert_eq!(game.render_layer(map.pair, &game.entries(map.pair, 1), 0).pixel(8, 8), [255, 0, 0, 255]);
        assert_eq!(game.gfx(map.pair).tile_count(), s.first_secondary as usize + 1);
    }
}

#[test]
fn attributes_and_behaviors() {
    for (s, family) in both() {
        let mut game = open(&s);
        let pair = game.maps[0].layout.pair();
        assert_eq!(game.behavior(pair, 2), 2);
        assert_eq!(game.attribute(pair, s.first_secondary), 3);
        assert_eq!(game.attribute(pair, s.first_secondary + 9), 0, "nothing there");
        let layer = game.format.layer_type;
        let attr = layer.set(game.format.behavior.set(0, 0x21), 1);
        game.set_attribute(pair, 1, attr);
        assert!(game.commit());
        assert_eq!(game.attribute(pair, 1), attr, "{family:?}");
        assert_eq!(game.behavior(pair, 1), 0x21);
        assert_eq!(game.attribute(pair, 2), 2, "the neighbour is untouched");
        assert_eq!(game.format.behavior_name(2), Some("TALL_GRASS"));
    }
}

#[test]
fn edits_are_undone_and_redone() {
    for (s, _) in both() {
        let mut game = open(&s);
        let mut map = game.load_map(1).unwrap();
        map.blocks[0] = block::with_metatile(map.blocks[0], 2);
        map.border[0] = 2;
        game.store_map(&map);
        assert!(game.commit());
        assert!(!game.commit(), "nothing more to commit");
        assert!(game.modified());
        assert_eq!(game.edited_layouts().into_iter().collect::<Vec<_>>(), [map.layout.offset]);
        assert_eq!(game.load_map(2).unwrap().blocks, map.blocks, "the map sharing the layout changed too");

        assert_eq!(game.undo().and_then(|at| game.map_at(at)), Some(1), "undo tells where it acted");
        assert!(!game.modified());
        assert!(game.edited_layouts().is_empty());
        assert!(game.undo().is_none());
        assert!(game.redo().is_some());
        assert_eq!(game.load_map(1).unwrap(), map);
        assert!(!game.can_redo());

        // a new edit after an undo forgets what was undone
        game.undo();
        map.blocks[1] = 0;
        game.store_map(&map);
        game.commit();
        assert!(!game.can_redo());
    }
}

#[test]
fn edits_files_carry_the_changes_to_another_copy() {
    for (s, family) in both() {
        let mut game = open(&s);
        assert!(game.collect_edits().is_empty());
        let mut map = game.load_map(0).unwrap();
        map.blocks[5] = block::pack(2, 1, 4);
        game.store_map(&map);
        let pair = map.pair;
        let mut entries = game.entries(pair, s.first_secondary);
        entries[4] = 1 | 0x400;
        game.set_entries(pair, s.first_secondary, &entries);
        game.set_attribute(pair, 2, 7);
        game.commit();

        let edits = game.collect_edits();
        assert_eq!(edits.maps.keys().collect::<Vec<_>>(), ["0.0"]);
        let keys: Vec<String> = edits.tilesets.keys().cloned().collect();
        assert_eq!(keys, [format!("0x{:06X}", s.primary), format!("0x{:06X}", s.secondary)], "{family:?}");
        assert_eq!(edits.tilesets[&keys[0]].attributes["2"], 7);
        assert!(edits.tilesets[&keys[0]].metatiles.is_empty());
        assert_eq!(edits.tilesets[&keys[1]].metatiles["0"], entries);

        let json = edits.to_json();
        assert!(Edits::looks_like(&json));
        assert!(!Edits::looks_like(r#"{"game": "pokeemerald"}"#));
        let mut copy = open(&s);
        let applied = copy.apply_edits(&Edits::from_json(&json).unwrap());
        assert_eq!((applied.maps, applied.tilesets, applied.skipped.len()), (1, 2, 0));
        assert!(copy.commit());
        assert!(copy.rom.bytes() == game.rom.bytes(), "the copy is the edited ROM, byte for byte");
        assert!(copy.undo().is_some(), "an import is one step");
        assert!(!copy.modified());
    }
}

#[test]
fn edits_that_do_not_fit_are_reported() {
    let s = synth("BPEE", false);
    let mut game = open(&s);
    let json = r#"{"format": "GBMapping/1", "maps": {"0.0": {"width": 9, "height": 9, "blocks": [], "border": []}, "7.7": {"width": 1, "height": 1, "blocks": [0]}, "x": {"width": 1, "height": 1, "blocks": [0]}},
        "tilesets": {"0x000010": {"metatiles": {}, "attributes": {}}}}"#;
    let applied = game.apply_edits(&Edits::from_json(json).unwrap());
    assert_eq!((applied.maps, applied.tilesets), (0, 0));
    assert_eq!(applied.skipped.len(), 4);
    assert!(applied.skipped[0].contains("9x9 in the file, 3x2 in this ROM"), "{:?}", applied.skipped);
    assert!(!game.commit());
    assert!(Edits::from_json(r#"{"format": "Other/1"}"#).is_err());
    assert!(Edits::from_json(r#"{"game": "pokeemerald"}"#).is_err());
}

#[test]
fn a_shared_layout_is_told_once() {
    let s = synth("BPEE", false);
    let mut game = open(&s);
    let mut map = game.load_map(2).unwrap();
    map.blocks[3] = 1;
    game.store_map(&map);
    game.commit();
    assert_eq!(game.collect_edits().maps.keys().collect::<Vec<_>>(), ["0.1"]);
}

#[test]
fn profiles_name_the_maps() {
    let s = synth("BPEE", false);
    let profile = Profile::from_json(&format!(
        r#"{{"game": "pokeemerald", "name": "Synth hack", "gMapGroups": {}, "groups": [{{"name": "gMapGroup_Towns", "maps": ["TownA"]}}, {{"name": "", "maps": [{{"name": "Cave"}}]}}]}}"#,
        s.groups_table
    ))
    .unwrap();
    let game = Game::open(s.bytes.clone(), "synth.gba", profile).unwrap();
    assert_eq!(game.name(), "Synth hack");
    // the profile lists one map in the first group: that's the group's size
    let maps: Vec<(String, &str, &str)> = game.maps.iter().map(|m| (m.id.to_string(), m.name.as_str(), m.group_name.as_str())).collect();
    assert_eq!(maps, [("0.0".to_string(), "TownA", "Towns"), ("1.0".to_string(), "Cave", "Group 1")]);
    assert_eq!(game.maps[0].section.as_deref(), Some("PLACE 2"));
    assert_eq!(game.map_index(MapId { group: 1, num: 0 }), Some(1));
    assert_eq!("1.0".parse::<MapId>(), Ok(MapId { group: 1, num: 0 }));
    assert!("1".parse::<MapId>().is_err());

    // an address that isn't the table's is no obstacle: the table is found
    let wrong = Profile::from_json(r#"{"gMapGroups": 512}"#).unwrap();
    assert_eq!(Game::open(s.bytes.clone(), "synth.gba", wrong).unwrap().maps.len(), 3);
}

#[test]
fn the_usual_collision_and_elevation_of_a_metatile() {
    let s = synth("BPEE", false);
    let game = open(&s);
    let usage = game.block_usage(game.maps[0].layout.pair());
    // metatile 1: on the maps as (0, 0), (1, 3), (0, 3), (0, 3), (1, 3); the shared layout counts once
    let one = &usage[&1];
    assert_eq!(one.counts.iter().sum::<u32>(), 5);
    assert_eq!(one.choose(block::pack(9, 0, 3)), block::pack(0, 0, 3), "at this elevation it's passable as often as not: passable");
    assert_eq!(one.choose(block::pack(9, 1, 7)), block::pack(0, 0, 3), "never seen that high: its usual place");
    assert_eq!(one.choose(block::pack(9, 1, 0)), block::pack(0, 0, 0), "seen at 0 often enough: stays there");
    // metatile 2: (0, 0), (0, 3), (0, 1)
    assert_eq!(usage[&2].choose(block::pack(9, 1, 1)), block::pack(0, 0, 1));
    // a metatile no map uses keeps what the tile had
    assert_eq!(gbmapping_core::game::BlockUsage::default().choose(block::pack(9, 1, 4)), block::pack(0, 1, 4));
}

#[test]
fn headers_with_a_wide_section_are_told_by_themselves() {
    let plain = open(&synth("BPEE", false));
    assert!(!plain.mapsec16);
    assert_eq!(plain.maps[0].header.map_type, 2);

    let wide = open(&synth_with("BPEE", false, true));
    assert!(wide.mapsec16, "no profile said so: the headers did");
    assert_eq!((wide.maps[0].header.mapsec, wide.maps[0].header.map_type), (2, 8));
    let names: Vec<&str> = wide.maps.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["PLACE 2", "PLACE 3", "PLACE 3"]);
}

#[test]
fn a_map_is_found_again_in_another_game() {
    let s = synth("BPEE", false);
    let plain = open(&s);
    // nothing names the maps: the place and the size tell them, and their rank among those alike
    for index in 0..3 {
        assert_eq!(plain.counterpart(&plain, index), Some(index));
    }
    // the other game names its maps, this one doesn't: still the place and the size
    let named = |names: [&str; 3]| {
        let json = format!(
            r#"{{"groups": [{{"name": "A", "maps": ["{}", "{}"]}}, {{"name": "B", "maps": ["{}"]}}]}}"#,
            names[0], names[1], names[2]
        );
        Game::open(s.bytes.clone(), "synth.gba", Profile::from_json(&json).unwrap()).unwrap()
    };
    let ours = named(["TownA", "HouseB", "HouseC"]);
    assert_eq!(plain.counterpart(&ours, 2), Some(2));
    // both name them: the name wins over the rank (their second map is our third)
    let theirs = named(["Elsewhere", "housec", "Other"]);
    assert_eq!(ours.counterpart(&theirs, 1), Some(2));
    assert_eq!(ours.counterpart(&theirs, 0), Some(0), "an unknown name: the place and the size again");
    // a place this game doesn't have
    let elsewhere =
        Game::open(s.bytes.clone(), "synth.gba", Profile::from_json(r#"{"regionNames": {"2": "ELSEWHERE"}}"#).unwrap()).unwrap();
    assert_eq!(elsewhere.maps[0].section.as_deref(), Some("ELSEWHERE"));
    assert_eq!(plain.counterpart(&elsewhere, 0), None);
    assert_eq!(plain.counterpart(&elsewhere, 1), None, "a map of no known place has no counterpart");
    assert_eq!(plain.counterpart(&plain, 9), None);
}

#[test]
fn the_closest_looking_metatile() {
    for (s, family) in both() {
        let game = open(&s);
        let pair = game.maps[0].layout.pair();
        let (columns, count) = (8, game.metatile_count(pair));
        let sheet = game.render_sheet(pair, columns);
        let picture = |id: usize| sheet.crop((id % columns) * 16, (id / columns) * 16, 16, 16);
        // a metatile's own picture
        for id in [1usize, 2, s.first_secondary as usize] {
            assert_eq!(closest_metatile(&sheet, columns, count, &picture(id)), Some(id as u16), "{family:?}");
        }
        // pictures that are no metatile's: the nearest color
        let flat = |color: [u8; 4]| Image::filled(16, 16, color);
        assert_eq!(closest_metatile(&sheet, columns, count, &flat([240, 30, 20, 255])), Some(1), "reddish: the red one");
        assert_eq!(closest_metatile(&sheet, columns, count, &flat([10, 20, 230, 255])), Some(s.first_secondary), "bluish: the blue one");
        assert_eq!(closest_metatile(&sheet, columns, count, &flat([8, 8, 8, 255])), Some(0), "dark: the first of the blank ones");
        // ids that hold nothing are never the answer, and a sheet of nothing has none
        assert_eq!(closest_metatile(&Image::new(128, 32), columns, 16, &flat([0, 0, 0, 255])), None);
        // a crop past the picture's edge is see-through there
        let corner = sheet.crop(sheet.width - 8, 0, 16, 16);
        assert_eq!(corner.pixel(8, 0)[3], 0);
        assert_eq!(corner.pixel(7, 0), sheet.pixel(sheet.width - 1, 0));
    }
}

#[test]
fn a_map_comes_with_its_original_when_the_profile_tells_where() {
    for (s, family) in both() {
        // nothing told: no original
        assert!(open(&s).maps.iter().all(|m| m.original.is_none()));
        assert!(open(&s).load_original(0).is_none());

        let profile = |maps: &str| {
            let json = format!(
                r#"{{"gMapLayouts": {}, "groups": [{{"name": "A", "maps": [{maps}]}}, {{"name": "B", "maps": ["C"]}}]}}"#,
                s.layouts_table
            );
            Game::open(s.bytes.clone(), "synth.gba", Profile::from_json(&json).unwrap()).unwrap()
        };
        let game = profile(r#"{"name": "A", "layout": 1, "original": 4}, {"name": "B", "layout": 2}"#);
        assert_eq!(game.layout_by_id(1).map(|l| l.offset), Some(game.maps[0].layout.offset), "{family:?}");
        let (map, original) = (game.load_map(0).unwrap(), game.load_original(0).unwrap());
        assert_eq!((original.index, original.id, &original.name), (0, map.id, &map.name));
        assert_eq!((original.width, original.height), (3, 2));
        assert_eq!(original.blocks, [s.first_secondary, s.first_secondary, 1, 0, 0, s.first_secondary + 1]);
        assert_ne!(original.blocks, map.blocks);
        // on its own tilesets: the secondary one in the primary's place too
        assert_eq!((original.pair.primary, original.pair.secondary), (s.secondary, s.secondary));
        assert_ne!(original.pair, map.pair);
        assert_eq!(game.render_map(&original).pixel(0, 0), [0, 0, 255, 255], "drawn with them: blue where the map is red");
        assert_eq!(game.render_map(&map).pixel(0, 0), [255, 0, 0, 255]);
        // the other maps have none, and the game's own pairs are the maps' alone
        assert!(game.maps[1].original.is_none() && game.load_original(1).is_none() && game.load_original(9).is_none());
        assert_eq!(game.pairs().len(), 1);
        // the original's pair is told apart; its tilesets here are ones the maps are on too
        assert_eq!(game.original_pairs().into_iter().collect::<Vec<_>>(), [original.pair]);
        assert!(game.original_tilesets().is_empty());
        assert_eq!(game.pair_with(s.secondary), Some((map.pair, true)), "a tileset goes by a pair of the maps first");

        // ids that are no layout: nothing there, past the table, not a pointer, the map's own layout
        for id in [0, 3, 5, 6, 400, 1] {
            let game = profile(&format!(r#"{{"name": "A", "original": {id}}}, "B""#));
            assert!(game.maps[0].original.is_none(), "{family:?}: id {id}");
        }
        // and without the table's address, ids mean nothing
        let json = r#"{"groups": [{"name": "A", "maps": [{"name": "A", "original": 4}, "B"]}]}"#;
        assert!(Game::open(s.bytes.clone(), "synth.gba", Profile::from_json(json).unwrap()).unwrap().maps[0].original.is_none());
    }
}

#[test]
fn a_tile_is_shown_in_the_palette_it_is_used_with() {
    for (s, family) in both() {
        let game = open(&s);
        let pair = game.maps[0].layout.pair();
        let own = game.format.primary_palettes as u8;
        let usage = game.tile_usage();
        assert_eq!((usage.usual(s.primary, 1), usage.usual(s.primary, 2), usage.uses(s.primary, 1)), (Some(0), Some(0), 4), "{family:?}");
        assert_eq!(usage.usual(s.secondary, 0), Some(own), "the secondary tileset's tile, in its own palette");
        assert_eq!((usage.usual(s.secondary, 3), usage.uses(s.secondary, 3)), (None, 0));
        assert_eq!(usage.shown(s.secondary, 3), own, "a tile no metatile uses is shown like its neighbour");
        assert_eq!(usage.shown(1, 3), 0, "and one of an unknown tileset in the first palette");

        let sheet = game.tile_sheet(pair, true, &usage, None, 16);
        assert_eq!((sheet.count, sheet.palettes.as_slice()), (1, &[own][..]));
        assert_eq!(sheet.image.pixel(0, 0), [0, 0, 255, 255], "blue, as the metatile that uses it draws it");
        assert_eq!(game.tile_sheet(pair, true, &usage, Some(0), 16).image.pixel(0, 0), [255, 0, 0, 255], "red in the palette asked for");
        let first = game.tile_sheet(pair, false, &usage, None, 16);
        assert_eq!(first.count, game.tileset_tile_count(pair, false));
        assert!(first.count > 2);
        assert_eq!(first.image.pixel(0, 0)[3], 0, "what a tile leaves see-through stays so");
        assert_eq!((first.image.pixel(8, 0), first.image.pixel(16, 0)), ([255, 0, 0, 255], [0, 255, 0, 255]));

        assert_eq!(
            (game.pair_with(s.secondary), game.pair_with(s.primary), game.pair_with(1)),
            (Some((pair, true)), Some((pair, false)), None)
        );
        // on the maps: the first map's tiles and border, and the shared layout's once
        let uses = game.metatile_uses();
        assert_eq!((uses[&s.primary][2], uses[&s.secondary][0], uses[&s.secondary][5]), (3, 1, 1));
    }
}

/// A tile in colours 0, 1 and 2 with no pattern to it: nothing a compressor can shorten
fn shape() -> [u8; 64] {
    let mut state = 2024u32;
    std::array::from_fn(|_| {
        state = state.wrapping_mul(1103515245).wrapping_add(12345);
        ((state >> 16) % 3) as u8
    })
}

#[test]
fn a_tile_of_another_tileset_is_made_usable_in_a_pair() {
    for (s, family) in both() {
        let mut game = open(&s);
        let pair = game.maps[0].layout.pair();
        let (own, first) = (game.format.primary_palettes as u8, game.format.primary_tiles as u16);
        let colors = |list: &[(usize, u16)]| {
            let mut colors = [0u16; 16];
            list.iter().for_each(|&(i, color)| colors[i] = color);
            colors
        };

        // the pair has the colours and the very tile: nothing to copy
        let red = game.import_tile(pair, &[1; 64], &colors(&[(1, RED)]));
        assert_eq!(red, Ok(Borrowed { tile: 1, palette: 0, colors: Colors::Found, existing: true }), "{family:?}");
        // the colour is elsewhere in the palette: the tile that has it there
        let green = game.import_tile(pair, &[3; 64], &colors(&[(3, GREEN)]));
        assert_eq!(green, Ok(Borrowed { tile: 2, palette: 0, colors: Colors::Found, existing: true }));
        // or in another palette
        let blue = game.import_tile(pair, &[1; 64], &colors(&[(1, BLUE)]));
        assert_eq!(blue, Ok(Borrowed { tile: 1, palette: own, colors: Colors::Found, existing: true }));
        assert_eq!(game.borrow_tile(pair, pair, false, 2, 0), green, "a tile taken where it is, in a palette");
        assert_eq!(game.borrow_tile(pair, pair, true, 9, 0), Err(BorrowError::Nothing));
        assert!(!game.commit(), "nothing was written");

        // colours no palette has: a palette no metatile uses takes them, and the tile joins the secondary tileset
        let pixels = shape();
        let new_colors = colors(&[(1, 0x7FFF), (2, 0x1234)]);
        let before = game.tileset(s.secondary).unwrap().tiles;
        let new = game.import_tile(pair, &pixels, &new_colors);
        assert_eq!(new, Ok(Borrowed { tile: first + 1, palette: own + 1, colors: Colors::Added, existing: false }));
        assert_eq!(game.gfx(pair).tile(first + 1), pixels);
        assert_eq!(game.raw_palettes(pair)[own as usize + 1], new_colors);
        assert_eq!(game.tileset_tile_count(pair, true), 2);
        let moved = game.tileset(s.secondary).unwrap().tiles;
        assert!(
            moved != before && moved >= s.bytes.len() - FREE,
            "the tiles no longer fit where they were: they're at the ROM's unused end"
        );

        // a metatile uses it: its palette is taken. The same shape in other colours is the same tile in the next free palette
        let mut entries = game.entries(pair, first + 1);
        entries[0] = (first + 1) | (own as u16 + 1) << 12;
        game.set_entries(pair, first + 1, &entries);
        let recolored = game.import_tile(pair, &pixels, &colors(&[(1, 0x0421), (2, 0x4000)]));
        assert_eq!(recolored, Ok(Borrowed { tile: first + 1, palette: own + 2, colors: Colors::Added, existing: true }));
        // another shape in colours that are there now: one more tile, grown where the tiles are
        let mut other = pixels;
        other[0] = (pixels[0] + 1) % 3;
        let third = game.import_tile(pair, &other, &new_colors);
        assert_eq!(third, Ok(Borrowed { tile: first + 2, palette: own + 1, colors: Colors::Found, existing: false }));
        assert_eq!(game.tileset(s.secondary).unwrap().tiles, moved);
        assert_eq!((game.gfx(pair).tile(first + 1), game.gfx(pair).tile(first + 2)), (pixels, other));
        assert!(game.commit());

        // what changed goes into the edits, and from there into another copy
        let edits = game.collect_edits();
        let told = &edits.tilesets[&format!("0x{:06X}", s.secondary)];
        assert_eq!(told.tiles.keys().collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(told.tiles["1"].len(), 64);
        let mut palettes: Vec<usize> = told.palettes.keys().map(|p| p.parse().unwrap()).collect();
        palettes.sort_unstable();
        assert_eq!(palettes, [own as usize + 1, own as usize + 2]);
        assert_eq!(told.palettes[&(own + 1).to_string()], new_colors);
        assert_eq!(told.metatiles.len(), 1);
        let mut copy = open(&s);
        let applied = copy.apply_edits(&Edits::from_json(&edits.to_json()).unwrap());
        assert_eq!((applied.tilesets, applied.skipped.len()), (1, 0), "{:?}", applied.skipped);
        assert!(copy.commit());
        assert_eq!((copy.gfx(pair).tile(first + 1), copy.gfx(pair).tile(first + 2)), (pixels, other));
        assert_eq!(copy.raw_palettes(pair), game.raw_palettes(pair));
        assert_eq!(copy.entries(pair, first + 1), entries);
        assert_eq!(copy.collect_edits(), edits);
        assert!(copy.render_sheet(pair, 16) == game.render_sheet(pair, 16));

        // undone in one step, tiles, palettes and all
        assert!(game.undo().is_some());
        assert_eq!((game.tileset_tile_count(pair, true), game.tileset(s.secondary).unwrap().tiles), (1, before));
        assert!(!game.modified() && game.collect_edits().is_empty());
        assert!(game.redo().is_some());
        assert_eq!(game.gfx(pair).tile(first + 2), other);
    }
}

#[test]
fn a_tile_that_cannot_be_brought_changes_nothing() {
    for (s, family) in both() {
        let pair = open(&s).maps[0].layout.pair();
        let (own, most) = (open(&s).format.primary_palettes, open(&s).format.secondary_tiles());
        let pixels = shape();
        let mut colors = [0u16; 16];
        (colors[1], colors[2]) = (0x7FFF, 0x1234);

        // no unused space at the ROM's end
        let mut cramped = Game::open(s.bytes[..s.bytes.len() - FREE].to_vec(), "synth.gba", Profile::default()).unwrap();
        assert_eq!(cramped.import_tile(pair, &pixels, &colors), Err(BorrowError::NoRoom), "{family:?}");
        assert!(!cramped.commit() && !cramped.modified());

        // a secondary tileset with every tile it can hold
        let mut full = open(&s);
        let secondary = full.tileset(s.secondary).unwrap();
        assert_eq!(full.store_tiles(&secondary, &vec![0x11; most * 32]), Ok(()));
        assert_eq!(full.tileset_tile_count(pair, true), most);
        full.commit();
        assert_eq!(full.import_tile(pair, &pixels, &colors), Err(BorrowError::Full { tiles: most }));
        assert!(!full.commit(), "not even the palette");
        // tiles that aren't compressed can't be told from what follows them
        let primary = full.tileset(s.primary).unwrap();
        assert_eq!(full.store_tiles(&primary, &[0; 32]), Err(BorrowError::NotCompressed));

        // every palette taken: the closest colours of the closest palette, and no palette is written
        let mut taken = open(&s);
        let first = taken.format.primary_tiles as u16;
        taken.set_entries(pair, first + 1, &std::array::from_fn(|k| ((own + k) as u16) << 12));
        taken.commit();
        let before = taken.raw_palettes(pair);
        let mut reddish = [0u16; 16];
        reddish[1] = 0x001E;
        assert_eq!(
            taken.import_tile(pair, &[1; 64], &reddish),
            Ok(Borrowed { tile: 1, palette: 0, colors: Colors::Approximated, existing: true })
        );
        assert!(taken.raw_palettes(pair) == before && !taken.commit());
    }
}
