//! The same checks on real ROMs, which can't be in the repository: name them in
//! GBMAPPING_TEST_ROMS (paths separated by `;`) and they're opened, drawn and edited.
//! A ROM's profile goes after it, behind a `|`: what the profile tells is checked too.
//!
//!     GBMAPPING_TEST_ROMS="emerald.gba;my-hack.gba|my-hack.json" cargo test -p gbmapping-core --test roms -- --nocapture

use gbmapping_core::format::block;
use gbmapping_core::{Edits, Game, MapId, Profile};
use std::collections::BTreeSet;

/// The ROMs named, each with its profile's text when it has one
fn named() -> Vec<(String, Vec<u8>, Option<String>)> {
    let Ok(list) = std::env::var("GBMAPPING_TEST_ROMS") else {
        eprintln!("GBMAPPING_TEST_ROMS isn't set: nothing checked");
        return Vec::new();
    };
    let read = |path: &str| std::fs::read(path.trim()).unwrap_or_else(|e| panic!("{path}: {e}"));
    list.split(';')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (rom, profile) = entry.split_once('|').map_or((entry, None), |(rom, profile)| (rom, Some(profile)));
            (rom.trim().to_string(), read(rom), profile.map(|p| String::from_utf8(read(p)).expect("a profile is text")))
        })
        .collect()
}

fn roms() -> Vec<(String, Vec<u8>)> {
    named().into_iter().map(|(path, bytes, _)| (path, bytes)).collect()
}

#[test]
fn every_map_and_tileset_is_read_and_drawn() {
    for (path, bytes) in roms() {
        let game = Game::open(bytes, &path, Profile::default()).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(game.maps.len() >= 100, "{path}: {} maps", game.maps.len());
        assert!(game.sections.is_some(), "{path}: no place names");
        let named = game.maps.iter().filter(|m| m.section.is_some()).count();
        assert!(named * 10 >= game.maps.len() * 9, "{path}: only {named} of {} maps have a place's name", game.maps.len());
        for index in 0..game.maps.len() {
            let map = game.load_map(index).unwrap();
            assert_eq!(map.blocks.len(), map.width * map.height);
            let count = game.metatile_count(map.pair) as u16;
            let unknown = map.blocks.iter().filter(|&&b| block::metatile(b) >= count).count();
            assert!(unknown * 20 <= map.blocks.len(), "{path}: map {} uses {unknown} metatiles its tilesets don't hold", map.id);
        }
        for pair in game.pairs() {
            let sheet = game.render_sheet(pair, 16);
            assert_eq!(sheet.width, 256);
            assert!(sheet.rgba.chunks(4).any(|p| p[..3] != sheet.rgba[..3]), "{path}: a blank tileset pair");
            assert!(game.gfx(pair).tile_count() > game.format.primary_tiles);
        }
        let image = game.render_map(&game.load_map(0).unwrap());
        assert!(image.rgba.chunks(4).all(|p| p[3] == 255));
        assert!(!game.modified());
        println!("{path}: {} — {} maps, {} tileset pairs", game.name(), game.maps.len(), game.pairs().len());
    }
}

#[test]
fn vanilla_emerald_is_what_it_is() {
    for (path, bytes) in roms() {
        let game = Game::open(bytes, &path, Profile::default()).unwrap();
        // the English game, untouched: its tables are where they've always been
        if game.rom.header().code != "BPEE" || game.groups.table != 0x486578 {
            continue;
        }
        assert_eq!(game.maps.len(), 518);
        assert_eq!(game.groups.groups.len(), 34);
        assert_eq!(game.sections.map(|s| (s.table, s.count)), Some((0x5A147C, 213)));
        let littleroot = &game.maps[game.map_index(MapId { group: 0, num: 9 }).unwrap()];
        assert_eq!(littleroot.name, "LittlerootTown", "the original game's map names are known");
        assert_eq!(littleroot.section.as_deref(), Some("LITTLEROOT TOWN"));
        assert_eq!(littleroot.group_name, "TownsAndRoutes");
        assert_eq!(game.maps.last().map(|m| m.name.as_str()), Some("Route124_DivingTreasureHuntersHouse"));
        assert_eq!((littleroot.layout.width, littleroot.layout.height), (20, 20));
        assert_eq!(game.metatile_count(littleroot.layout.pair()), 656);
        assert_eq!(game.section_name(66).as_deref(), Some("AQUA HIDEOUT"));
        // tall grass is tall grass
        let route101 = game.load_map(game.map_index(MapId { group: 0, num: 16 }).unwrap()).unwrap();
        assert_eq!(route101.name, "Route101");
        assert_eq!(game.format.behavior_name(game.behavior(route101.pair, 0x0D)), Some("TALL_GRASS"));
    }
}

#[test]
fn edits_survive_the_trip_through_a_file() {
    for (path, bytes) in roms() {
        let mut game = Game::open(bytes.clone(), &path, Profile::default()).unwrap();
        for index in [0, game.maps.len() / 2, game.maps.len() - 1] {
            let mut map = game.load_map(index).unwrap();
            map.blocks[0] ^= 0x0401;
            let last = map.border.len() - 1;
            map.border[last] ^= 1;
            game.store_map(&map);
            let attribute = game.attribute(map.pair, 1);
            game.set_attribute(map.pair, 1, attribute ^ 1);
            assert!(game.commit(), "{path}: map {}", map.id);
        }
        let edits = game.collect_edits();
        assert!(!edits.maps.is_empty() && !edits.tilesets.is_empty());
        let mut copy = Game::open(bytes, &path, Profile::default()).unwrap();
        let applied = copy.apply_edits(&Edits::from_json(&edits.to_json()).unwrap());
        assert!(applied.skipped.is_empty(), "{path}: {:?}", applied.skipped);
        copy.commit();
        assert!(copy.rom.bytes() == game.rom.bytes(), "{path}: the copy differs");
        while game.undo().is_some() {}
        assert!(!game.modified(), "{path}: undoing everything gives the ROM back");
    }
}

#[test]
fn the_maps_originals_come_with_their_own_tilesets() {
    for (path, bytes, profile) in named() {
        let Some(profile) = profile else { continue };
        let game = Game::open(bytes, &path, Profile::from_json(&profile).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"));
        let originals: Vec<usize> = (0..game.maps.len()).filter(|&i| game.maps[i].original.is_some()).collect();
        let told = game.profile.groups.iter().flat_map(|g| &g.maps).filter(|m| m.original().is_some()).count();
        assert_eq!(originals.len(), told, "{path}: every original the profile tells is a layout of the ROM");
        if originals.is_empty() {
            println!("{path}: its profile tells no original");
            continue;
        }
        let mut pairs = BTreeSet::new();
        for &index in &originals {
            let name = &game.maps[index].name;
            let map = game.load_map(index).unwrap();
            let original = game.load_original(index).unwrap();
            assert_eq!((original.index, original.id), (index, map.id));
            assert_ne!(original.layout.offset, map.layout.offset, "{path}: {name}'s original is the map itself");
            assert_eq!(original.blocks.len(), original.width * original.height);
            // the map was redrawn, not resized: its original lies under it tile for tile
            assert_eq!((original.width, original.height), (map.width, map.height), "{path}: {name}");
            // every metatile the original uses is one its own tilesets hold
            let used: BTreeSet<u16> = original.blocks.iter().chain(&original.border).map(|&b| block::metatile(b)).collect();
            let missing: Vec<u16> = used.into_iter().filter(|&id| game.locate(original.pair, id).is_none()).collect();
            assert!(missing.is_empty(), "{path}: {name}'s original uses metatiles its tilesets don't hold: {missing:?}");
            pairs.insert(original.pair);
        }
        // those tilesets are drawn, and they aren't the ones the maps are on now
        let now = game.pairs();
        let own = pairs.iter().filter(|pair| !now.contains(pair)).count();
        assert!(own > 0, "{path}: the originals are on the tilesets the maps are on now");
        for &pair in &pairs {
            let sheet = game.render_sheet(pair, 16);
            assert!(sheet.rgba.chunks(4).any(|p| p[..3] != sheet.rgba[..3]), "{path}: a blank tileset pair for an original");
        }
        let edited = game.collect_edits();
        assert!(edited.is_empty(), "{path}: looking at the originals changes nothing");
        println!(
            "{path}: {} maps come with their original, on {} tileset pairs ({own} of them no map is on now)",
            originals.len(),
            pairs.len()
        );
    }
}
