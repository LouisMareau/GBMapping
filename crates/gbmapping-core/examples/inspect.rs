//! Lists a ROM's maps and draws one, without a browser:
//!
//!     cargo run -p gbmapping-core --example inspect -- <rom.gba> [profile.json] [map id] [original] [out.png]
//!
//! With a .png and no map id, the tileset pair of the first map is drawn instead.
//! `original` draws the map as it was before the hack redrew it, when the profile tells
//! where the ROM keeps it.

use gbmapping_core::{Game, Image, MapId, Profile};
use std::time::Instant;

fn save_png(path: &str, image: &Image) {
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("can't create the image"));
    let mut encoder = png::Encoder::new(file, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(&image.rgba)).expect("can't write the image");
    println!("wrote {path} ({}x{})", image.width, image.height);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(rom_path) = args.next() else {
        eprintln!("usage: inspect <rom.gba> [profile.json] [map id] [original] [out.png]");
        std::process::exit(2);
    };
    let (mut profile, mut map_id, mut out, mut original) = (Profile::default(), None, None, false);
    for a in args {
        if a.ends_with(".json") {
            profile = Profile::from_json(&std::fs::read_to_string(&a).expect("can't read the profile")).expect("bad profile");
        } else if a.ends_with(".png") {
            out = Some(a);
        } else if a == "original" {
            original = true;
        } else {
            map_id = Some(a.parse::<MapId>().expect("bad map id"));
        }
    }
    let bytes = std::fs::read(&rom_path).expect("can't read the ROM");
    let started = Instant::now();
    let game = match Game::open(bytes, &rom_path, profile) {
        Ok(game) => game,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let header = game.rom.header();
    println!("{} ({}) rev {}, {} bytes: {}", header.title, header.code, header.version, game.rom.len(), game.name());
    println!(
        "map groups at 0x{:06X}: {} maps in {} groups, {} tileset pairs, {} tilesets (opened in {:?})",
        game.groups.table,
        game.maps.len(),
        game.groups.groups.len(),
        game.pairs().len(),
        game.tilesets().len(),
        started.elapsed()
    );
    match game.sections {
        Some(s) => println!("place names at 0x{:06X}: {} entries of {} bytes", s.table, s.count, s.stride),
        None => println!("place names: not found"),
    }
    let per_group: Vec<String> = game.groups.groups.iter().map(|(_, h)| h.len().to_string()).collect();
    println!("maps per group: {}", per_group.join(" "));
    for m in game.maps.iter().take(12) {
        println!(
            "  {:>6}  {:<28} {:>3}x{:<3} type {} section {}",
            m.id.to_string(),
            m.name,
            m.layout.width,
            m.layout.height,
            m.header.map_type,
            m.header.mapsec
        );
    }
    let originals = game.maps.iter().filter(|m| m.original.is_some()).count();
    if originals > 0 {
        println!("{originals} maps come with their original");
    }
    let index = map_id.map(|id| game.map_index(id).expect("no such map"));
    let load = |i: usize| if original { game.load_original(i) } else { game.load_map(i) };
    if let Some(map) = index.and_then(load) {
        let primary = game.tileset(map.pair.primary).unwrap();
        let secondary = game.tileset(map.pair.secondary).unwrap();
        println!(
            "map {} {} {}x{} border {}x{}; tilesets 0x{:06X} + 0x{:06X}; {} metatiles, {} tiles; compressed {} {}; shared by {} map(s)",
            map.id,
            map.name,
            map.width,
            map.height,
            map.border_width,
            map.border_height,
            primary.offset,
            secondary.offset,
            game.metatile_count(map.pair),
            game.gfx(map.pair).tile_count(),
            primary.compressed,
            secondary.compressed,
            game.layout_users(map.layout.offset).len()
        );
        if let Some(out) = &out {
            save_png(out, &game.render_map(&map));
        }
    } else if let (Some(out), Some(first)) = (&out, game.maps.first()) {
        save_png(out, &game.render_sheet(first.layout.pair(), 16));
    }
}
