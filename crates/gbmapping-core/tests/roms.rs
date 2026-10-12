//! The same checks on real ROMs, which can't be in the repository: name them in
//! GBMAPPING_TEST_ROMS (paths separated by `;`) and they're opened, drawn and edited.
//! A ROM's profile goes after it, behind a `|`: what the profile tells is checked too.
//! Game Boy Color ROMs have their own check.
//!
//!     GBMAPPING_TEST_ROMS="emerald.gba;my-hack.gba|my-hack.json;crystal.gbc" cargo test -p gbmapping-core --test roms -- --nocapture

use gbmapping_core::format::block;
use gbmapping_core::gbc::{is_game_boy, quarter};
use gbmapping_core::metatiles::GrowError;
use gbmapping_core::rom::lz77_compress;
use gbmapping_core::tiles::{BorrowError, Colors};
use gbmapping_core::{Edits, Family, Game, MapId, Pair, Profile, Rom};
use std::collections::{BTreeMap, BTreeSet};

/// The ROMs named, each with its profile's text when it has one
fn named() -> Vec<(String, Vec<u8>, Option<String>)> {
    let Ok(list) = std::env::var("GBMAPPING_TEST_ROMS") else {
        eprintln!("GBMAPPING_TEST_ROMS isn't set: nothing checked");
        return Vec::new();
    };
    let read = |path: &str| std::fs::read(path.trim()).unwrap_or_else(|e| panic!("{path}: {e}"));
    let all = list.split(';').filter(|entry| !entry.trim().is_empty()).map(|entry| {
        let (rom, profile) = entry.split_once('|').map_or((entry, None), |(rom, profile)| (rom, Some(profile)));
        (rom.trim().to_string(), read(rom), profile.map(|p| String::from_utf8(read(p)).expect("a profile is text")))
    });
    all.filter(|(_, bytes, _)| !is_game_boy(bytes)).collect()
}

fn roms() -> Vec<(String, Vec<u8>)> {
    named().into_iter().map(|(path, bytes, _)| (path, bytes)).collect()
}

/// The Game Boy Color ROMs named
fn game_boy_roms() -> Vec<(String, Vec<u8>)> {
    let list = std::env::var("GBMAPPING_TEST_ROMS").unwrap_or_default();
    let paths = list.split(';').map(|entry| entry.split('|').next().unwrap_or_default().trim().to_string()).filter(|path| !path.is_empty());
    let read = paths.map(|path| (std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")), path));
    read.filter(|(bytes, _)| is_game_boy(bytes)).map(|(bytes, path)| (path, bytes)).collect()
}

#[test]
fn game_boy_color_roms_are_read_drawn_and_written_back() {
    for (path, bytes) in game_boy_roms() {
        let mut game = Game::open(bytes.clone(), &path, Profile::default()).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!((game.family, game.format.block_size()), (Family::Crystal, 2));
        assert!(game.maps.len() >= 100, "{path}: {} maps", game.maps.len());
        let named = game.maps.iter().filter(|m| m.section.is_some()).count();
        assert!(named * 10 >= game.maps.len() * 9, "{path}: only {named} of {} maps have a place's name", game.maps.len());
        // the original game's maps go by the names its disassembly gives them
        if game.maps.len() == 388 {
            let town = game.maps.iter().find(|m| m.name == "NewBarkTown").unwrap_or_else(|| panic!("{path}: no NewBarkTown"));
            assert_eq!(
                (town.id.to_string().as_str(), town.layout.width, town.layout.height, town.group_name.as_str()),
                ("23.3", 20, 18, "NewBark")
            );
        }
        for index in 0..game.maps.len() {
            let map = game.load_map(index).unwrap();
            assert_eq!((map.blocks.len(), map.width % 2, map.height % 2), (map.width * map.height, 0, 0));
            let count = game.metatile_count(map.pair) as u16;
            let unknown = map.blocks.iter().filter(|&&b| block::metatile(b) >= count).count();
            assert!(unknown * 20 <= map.blocks.len(), "{path}: map {} uses {unknown} blocks its tileset doesn't hold", map.id);
        }
        // the tilesets are drawn, in colours: the palettes were found
        let pairs = game.pairs();
        let mut colored = 0;
        for &pair in &pairs {
            let sheet = game.render_sheet(pair, 8);
            assert!(sheet.rgba.chunks(4).any(|p| p[..3] != sheet.rgba[..3]), "{path}: a blank tileset");
            colored += sheet.rgba.chunks(4).any(|p| p[3] != 0 && (p[0] != p[1] || p[1] != p[2])) as usize;
        }
        assert!(colored * 10 >= pairs.len() * 9, "{path}: only {colored} of {} tilesets are in colours", pairs.len());
        let image = game.render_map(&game.load_map(0).unwrap());
        assert!(image.rgba.chunks(4).all(|p| p[3] == 255));

        // untouched, the ROM saved is the ROM opened; a block painted is one byte of it (and its checksum)
        assert!(game.saved() == bytes, "{path}: saved as it was opened, the ROM differs");
        let mut map = game.load_map(0).unwrap();
        let block = (0..4).find(|&b| map.blocks[0] != quarter(b, 0, 0)).unwrap();
        for q in 0..4 {
            map.blocks[(q / 2) * map.width + q % 2] = quarter(block, q % 2, q / 2);
        }
        game.store_map(&map);
        assert!(game.commit());
        let saved = game.saved();
        let differing = (0..saved.len()).filter(|&i| saved[i] != bytes[i] && !(0x14E..0x150).contains(&i)).count();
        assert_eq!(differing, 1, "{path}: one block painted");
        // and that ROM opens with the block where it was painted
        let again = Game::open(saved, &path, Profile::default()).unwrap_or_else(|e| panic!("{path}, saved: {e}"));
        assert_eq!(again.load_map(0).unwrap().blocks, map.blocks);
        assert!(game.undo().is_some() && game.saved() == bytes, "{path}: undone");
        println!("{path}: {} — {} maps, {} tilesets in their maps' colours", game.name(), game.maps.len(), pairs.len());
    }
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
        // a primary tileset's metatile is on every map that has it, each with another secondary
        // tileset: how many are made of a secondary tileset's tiles all the same
        let (mut mixed, mut on_maps) = (0, 0);
        let uses = game.metatile_uses();
        for (t, _) in game.tilesets().into_iter().filter(|&(_, primary)| primary) {
            for local in 0..game.tileset_metatiles(&t, true) {
                let secondary = game.tileset_entries(&t, local).iter().any(|e| (e & 0x3FF) as usize >= game.format.primary_tiles);
                mixed += secondary as usize;
                on_maps += (secondary && uses.get(&t.offset).and_then(|u| u.get(local)).is_some_and(|&n| n > 0)) as usize;
            }
        }
        println!(
            "{path}: {} — {} maps, {} tileset pairs; {mixed} primary metatiles use a secondary tileset's tiles ({on_maps} of them on a map)",
            game.name(),
            game.maps.len(),
            game.pairs().len()
        );
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

        // the tilesets kept for the originals are there to take tiles from: one of an
        // original's goes into the pair its map is on now, and looks the same there
        let kept = game.original_tilesets();
        assert!(!kept.is_empty() && kept.iter().all(|(t, _)| game.pair_with(t.offset).is_some()), "{path}: the originals' tilesets");
        let usage = game.tile_usage();
        let first = game.format.primary_tiles;
        let mut game = game;
        let mut taken = 0;
        for &index in &originals {
            let (to, from) = (game.maps[index].layout.pair(), game.maps[index].original.unwrap().pair());
            let used = (0..game.tileset_tile_count(from, true)).find_map(|t| Some((t, usage.usual(from.secondary, t)?)));
            let Some((tile, palette)) = used.filter(|_| !now.contains(&from)) else { continue };
            let (shape, colors) = (game.gfx(from).tile((first + tile) as u16), game.palettes(from)[palette as usize]);
            let Ok(borrowed) = game.borrow_tile(to, from, true, tile, palette) else { continue };
            let (there, palette_there) = (game.gfx(to).tile(borrowed.tile), game.palettes(to)[borrowed.palette as usize]);
            for (a, b) in shape.iter().zip(&there) {
                assert_eq!(*a == 0, *b == 0, "{path}: {}: what's see-through stays so", game.maps[index].name);
                if borrowed.colors != Colors::Approximated && *a != 0 {
                    assert_eq!(colors[*a as usize], palette_there[*b as usize], "{path}: {}", game.maps[index].name);
                }
            }
            game.commit();
            while game.undo().is_some() {}
            assert!(!game.modified(), "{path}: undone");
            taken += 1;
            if taken == 20 {
                break;
            }
        }
        assert!(taken > 0, "{path}: no tile of an original's tileset could be brought to its map's");
        println!(
            "{path}: {} maps come with their original, on {} tileset pairs ({own} of them no map is on now); a tile of {taken} originals' tilesets brought to their maps'",
            originals.len(),
            pairs.len()
        );
    }
}

#[test]
fn tiles_are_packed_again_and_brought_from_one_tileset_to_another() {
    for (path, bytes) in roms() {
        let mut game = Game::open(bytes.clone(), &path, Profile::default()).unwrap();
        // every tileset's tiles come back the same from being compressed again, in about the room they took
        let (mut was, mut is) = (0, 0);
        for (t, _) in game.tilesets().into_iter().filter(|(t, _)| t.compressed) {
            let tiles = game.rom.lz77(t.tiles).unwrap_or_else(|| panic!("{path}: the tiles at 0x{:06X}", t.tiles));
            let packed = lz77_compress(&tiles);
            assert!(Rom::new(packed.clone(), "packed").lz77(0).as_ref() == Some(&tiles), "{path}: the tiles at 0x{:06X}", t.tiles);
            was += game.rom.lz77_len(t.tiles).unwrap();
            is += packed.len();
        }
        assert!(is * 10 <= was * 11, "{path}: {is} bytes compressed, {was} in the ROM");

        // tiles of one secondary tileset, used in the pair of another with the same primary
        let pairs: Vec<_> = game.pairs().into_iter().collect();
        let to = pairs[0];
        let from = *pairs.iter().find(|p| p.primary == to.primary && p.secondary != to.secondary).expect("two secondary tilesets");
        let usage = game.tile_usage();
        let first = game.format.primary_tiles;
        let (had, mut brought, mut added, mut inexact) = (game.tileset_tile_count(to, true), 0, 0, 0);
        for tile in 0..game.tileset_tile_count(from, true) {
            let Some(palette) = usage.usual(from.secondary, tile) else { continue };
            let (shape, colors) = (game.gfx(from).tile((first + tile) as u16), game.palettes(from)[palette as usize]);
            let borrowed = match game.borrow_tile(to, from, true, tile, palette) {
                Ok(borrowed) => borrowed,
                Err(BorrowError::Full { .. }) => break,
                Err(e) => panic!("{path}: tile {tile}: {e}"),
            };
            // it looks the same there: pixel for pixel when the colours could be had
            let (there, palette_there) = (game.gfx(to).tile(borrowed.tile), game.palettes(to)[borrowed.palette as usize]);
            for (a, b) in shape.iter().zip(&there) {
                assert_eq!(*a == 0, *b == 0, "{path}: tile {tile}: what's see-through stays so");
                if borrowed.colors != Colors::Approximated && *a != 0 {
                    assert_eq!(colors[*a as usize], palette_there[*b as usize], "{path}: tile {tile}");
                }
            }
            brought += 1;
            added += !borrowed.existing as usize;
            inexact += (borrowed.colors == Colors::Approximated) as usize;
            if brought == 60 {
                break;
            }
        }
        assert!(brought > 0 && inexact == 0, "{path}: {brought} tiles brought, {inexact} in approximate colours");
        assert_eq!(game.tileset_tile_count(to, true), had + added);
        assert!(game.commit());
        // the maps drawn with the pair look as they did: nothing of theirs moved
        let untouched = Game::open(bytes.clone(), &path, Profile::default()).unwrap();
        let index = game.maps.iter().position(|m| m.layout.pair() == to).unwrap();
        assert!(game.render_map(&game.load_map(index).unwrap()) == untouched.render_map(&untouched.load_map(index).unwrap()));

        // the edits carry the tiles and the palette to another copy of the ROM
        let edits = game.collect_edits();
        let told = edits.tilesets.values().next().unwrap();
        assert_eq!((edits.tilesets.len(), told.tiles.len()), (1, added), "{path}");
        let mut copy = Game::open(bytes, &path, Profile::default()).unwrap();
        let applied = copy.apply_edits(&Edits::from_json(&edits.to_json()).unwrap());
        assert!(applied.skipped.is_empty(), "{path}: {:?}", applied.skipped);
        copy.commit();
        assert!(copy.collect_edits() == edits, "{path}: the copy's edits differ");
        assert!(game.undo().is_some() && !game.modified(), "{path}: undone in one step");
        println!("{path}: {brought} tiles brought from one tileset to another ({added} new, {} palette(s) written)", told.palettes.len());
    }
}

#[test]
fn tilesets_are_given_new_metatiles() {
    for (path, bytes) in roms() {
        let untouched = Game::open(bytes.clone(), &path, Profile::default()).unwrap();
        let mut game = Game::open(bytes.clone(), &path, Profile::default()).unwrap();
        // every secondary tileset, in one of its pairs: one more metatile, the copy of its first
        let pairs: BTreeMap<usize, Pair> = game.pairs().into_iter().map(|pair| (pair.secondary, pair)).collect();
        let first = game.format.primary_metatiles as u16;
        let mut added = Vec::new();
        for &pair in pairs.values() {
            let count = game.metatile_count(pair);
            match game.add_metatile(pair, Some(first)) {
                Ok(new) => {
                    assert_eq!((new as usize, game.metatile_count(pair)), (count, count + 1), "{path}: 0x{:06X}", pair.secondary);
                    assert_eq!(
                        (game.entries(pair, new), game.attribute(pair, new)),
                        (game.entries(pair, first), game.attribute(pair, first))
                    );
                    added.push(pair);
                }
                Err(GrowError::Full { .. }) => {}
                Err(e) => panic!("{path}: 0x{:06X}: {e}", pair.secondary),
            }
        }
        assert!(game.commit() && !added.is_empty(), "{path}: no tileset could be given a metatile");
        // the maps look as they did, and the ROM as it is now opens with the same maps and one more metatile each
        let saved =
            Game::open(game.rom.bytes().to_vec(), &path, Profile::default()).unwrap_or_else(|e| panic!("{path}, with new metatiles: {e}"));
        assert_eq!(saved.maps.len(), game.maps.len());
        for &pair in &added {
            let index = game.maps.iter().position(|m| m.layout.pair() == pair).unwrap();
            assert!(
                game.render_map(&game.load_map(index).unwrap()) == untouched.render_map(&untouched.load_map(index).unwrap()),
                "{path}: map {index}"
            );
            assert_eq!(saved.metatile_count(pair), untouched.metatile_count(pair) + 1);
            assert!(saved.render_sheet(pair, 8) == game.render_sheet(pair, 8));
        }
        // the edits tell the new metatile of each, and nothing else: a tileset moved isn't a tileset changed
        let edits = game.collect_edits();
        assert_eq!((edits.maps.len(), edits.tilesets.len()), (0, added.len()), "{path}");
        assert!(
            edits.tilesets.values().all(|t| (t.metatiles.len(), t.attributes.len(), t.tiles.len(), t.palettes.len()) == (1, 1, 0, 0)),
            "{path}"
        );
        let mut copy = Game::open(bytes, &path, Profile::default()).unwrap();
        let applied = copy.apply_edits(&Edits::from_json(&edits.to_json()).unwrap());
        assert!(applied.skipped.is_empty(), "{path}: {:?}", applied.skipped);
        copy.commit();
        assert!(copy.collect_edits() == edits, "{path}: the copy's edits differ");
        assert!(game.undo().is_some() && !game.modified(), "{path}: undone in one step");
        println!("{path}: {} secondary tilesets given a new metatile ({} have all they can)", added.len(), pairs.len() - added.len());
    }
}
