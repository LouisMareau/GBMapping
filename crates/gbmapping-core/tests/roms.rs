//! The same checks on real ROMs, which can't be in the repository: name them in
//! GBMAPPING_TEST_ROMS (paths separated by `;`) and they're opened, drawn and edited.
//!
//!     GBMAPPING_TEST_ROMS="emerald.gba;my-hack.gba" cargo test -p gbmapping-core --test roms -- --nocapture

use gbmapping_core::format::block;
use gbmapping_core::{Edits, Game, MapId, Profile};

fn roms() -> Vec<(String, Vec<u8>)> {
    let Ok(list) = std::env::var("GBMAPPING_TEST_ROMS") else {
        eprintln!("GBMAPPING_TEST_ROMS isn't set: nothing checked");
        return Vec::new();
    };
    list.split(';')
        .filter(|p| !p.trim().is_empty())
        .map(|p| (p.trim().to_string(), std::fs::read(p.trim()).unwrap_or_else(|e| panic!("{p}: {e}"))))
        .collect()
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
