//! The animations that are part of the maps' tiles: the doors that open, and the
//! tiles a tileset keeps changing (water, flowers).
//!
//! Neither is at a known place. The doors are a table in the ROM, found by its shape:
//! per door, the metatile it is, and where its frames and their palettes are. A
//! tileset's animated tiles are in no table: its structure points to a routine, which
//! is followed (see `walk`) down to the tables of frames it copies over the
//! tileset's tiles, and where over them.
//!
//! A frame is tiles as the console has them, 4 bits a pixel, not compressed: drawing on
//! one writes the ROM where it is.

use crate::game::Game;
use crate::gfx::TILE;
use crate::rom::{Rom, GBA_ROM_BASE};
use crate::structs::Pair;
use crate::tiles::{TileUsage, TILE_BYTES};
use std::collections::{BTreeMap, BTreeSet};

/// What an animation is of
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A door that opens: the metatiles that are such a door, and how many metatiles it
    /// is across and down
    Door { metatiles: Vec<u16>, across: usize, down: usize },
    /// Tiles of a tileset that change as the game runs: the tileset, and the first of
    /// the tiles the frames are copied over (as metatiles number them)
    Tiles { tileset: usize, tile: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animation {
    pub kind: Kind,
    /// Where each frame's tiles are in the ROM
    pub frames: Vec<usize>,
    /// The frames in the order the game shows them
    pub sequence: Vec<usize>,
    /// How many tiles a frame is
    pub tiles: usize,
    /// Where the table that tells it is in the ROM: the doors' (a hack may have several,
    /// one per region, and its code chooses), or the one of the frames
    pub table: usize,
    /// The other tiles of the tileset the same frames are copied over, at other times:
    /// each the first of as many (flowers that don't all move together)
    pub also: Vec<usize>,
    /// A door's palettes: one per tile of a frame
    palettes: Vec<u8>,
}

impl Animation {
    /// How many tiles across a frame is shown
    fn columns(&self) -> usize {
        match self.kind {
            Kind::Door { across, .. } => across * 2,
            Kind::Tiles { .. } if self.tiles <= 4 => 2,
            Kind::Tiles { .. } if self.tiles <= 16 => 4,
            Kind::Tiles { .. } => 8,
        }
    }

    /// A frame's size in pixels
    pub fn size(&self) -> (usize, usize) {
        (self.columns() * TILE, self.tiles.div_ceil(self.columns()) * TILE)
    }

    /// Where a frame's tile is in its picture. A door's tiles go metatile by metatile,
    /// down its left side then down its right; a tileset's, row by row.
    pub fn place(&self, tile: usize) -> (usize, usize) {
        match self.kind {
            Kind::Door { down, .. } => {
                let (side, tile) = (tile / (down * 4), tile % (down * 4));
                ((side * 2 + tile % 2) * TILE, tile / 2 * TILE)
            }
            Kind::Tiles { .. } => (tile % self.columns() * TILE, tile / self.columns() * TILE),
        }
    }

    /// The tile of a frame a pixel of its picture is in
    pub fn tile_at(&self, x: usize, y: usize) -> Option<usize> {
        (0..self.tiles).find(|&tile| self.place(tile) == (x / TILE * TILE, y / TILE * TILE))
    }

    /// What to call it
    pub fn label(&self) -> String {
        match &self.kind {
            Kind::Door { metatiles, across, down } => {
                let ids: Vec<String> = metatiles.iter().map(|id| format!("0x{id:03X}")).collect();
                format!("Door · metatile {} · {across}×{down}", ids.join(", "))
            }
            Kind::Tiles { tile, .. } => {
                let places = if self.also.is_empty() { String::new() } else { format!(" · {} places", self.also.len() + 1) };
                format!("Tiles 0x{tile:03X} to 0x{:03X}{places} · {} frames", tile + self.tiles - 1, self.frames.len())
            }
        }
    }
}

/// What a routine does that tells where an animation is: the constants it loads, and
/// the routines it calls, each with what was in the third argument's register
#[derive(Default)]
struct Walk {
    literals: Vec<u32>,
    calls: Vec<(usize, Option<u32>)>,
}

/// Follows a routine of the console's short instructions from `start`, down every
/// branch it has, until it returns
fn walk(rom: &Rom, start: usize) -> Walk {
    let mut out = Walk::default();
    let mut seen = BTreeSet::new();
    let mut paths = vec![start];
    let jump = |pc: usize, offset: i32| (pc as i64 + 4 + offset as i64).max(0) as usize;
    while let Some(mut pc) = paths.pop() {
        let mut third: Option<u32> = None;
        while seen.insert(pc) && pc + 2 <= rom.len() && seen.len() < 2048 {
            let op = rom.u16(pc);
            let (rd_high, rd_low) = ((op >> 8) & 7, op & 7);
            match op >> 11 {
                // a constant kept after the routine
                0b01001 => {
                    let value = rom.u32(((pc + 4) & !3) + (op & 0xFF) as usize * 4);
                    out.literals.push(value);
                    third = if rd_high == 2 { Some(value) } else { third };
                }
                // a small constant, maybe shifted up after; anything else put there isn't followed
                0b00100 if rd_high == 2 => third = Some((op & 0xFF) as u32),
                0b00000 if rd_low == 2 => third = third.filter(|_| (op >> 3) & 7 == 2).map(|value| value << ((op >> 6) & 31)),
                0b00001..=0b00011 if rd_low == 2 => third = None,
                // a call: two instructions, and the register is the routine's to use
                0b11110 if rom.u16(pc + 2) >> 11 == 0b11111 => {
                    let offset = ((((op & 0x7FF) as i32) << 21) >> 9) + (((rom.u16(pc + 2) & 0x7FF) as i32) << 1);
                    out.calls.push((jump(pc, offset), third.take()));
                    pc += 4;
                    continue;
                }
                // a jump: the routine goes on there
                0b11100 => {
                    pc = jump(pc, (((op & 0x7FF) as i32) << 21) >> 20);
                    continue;
                }
                // one way or the other: both are followed
                0b11010 | 0b11011 if (op >> 8) & 0xF < 0xE => paths.push(jump(pc, ((op & 0xFF) as i8 as i32) << 1)),
                _ => {}
            }
            // the return: by a register, or with the registers taken back
            if op & 0xFF87 == 0x4700 || op & 0xFF00 == 0xBD00 {
                break;
            }
            pc += 2;
        }
    }
    out
}

/// Where an address in the ROM points to, when it's one to data (an even one)
fn data(rom: &Rom, value: u32) -> Option<usize> {
    let offset = value.checked_sub(GBA_ROM_BASE)? as usize;
    (value & 1 == 0 && offset < rom.len()).then_some(offset)
}

/// Is this where the console keeps a tile it draws backgrounds with?
fn video(value: u32) -> bool {
    (0x0600_0000..0x0600_8000).contains(&value) && value.is_multiple_of(TILE_BYTES as u32)
}

impl Game {
    /// The doors: runs of entries of 12 bytes (the metatile, a sound, a size, where the
    /// frames are, where their palettes are) that end with an empty one
    fn doors(&self) -> Vec<Animation> {
        let rom = &self.rom;
        // a size is how many metatiles: 1x2 in every game, 2x2 for Emerald's big doors, 1x1 for FireRed's
        let shape = |size: u8| match size {
            1 => (1, 2),
            2 => (2, 2),
            _ => (1, 1),
        };
        let door = |o: usize| -> Option<(u16, usize, usize, usize, usize)> {
            let (metatile, sound, size) = (rom.u16(o), rom.u8(o + 2), rom.u8(o + 3));
            if metatile == 0 || metatile >= 1024 || sound > 3 || size > 3 {
                return None;
            }
            let (tiles, palettes) = (data(rom, rom.u32(o + 4))?, data(rom, rom.u32(o + 8))?);
            let (across, down) = shape(size);
            let fits = tiles + 3 * across * down * 4 * TILE_BYTES <= rom.len();
            (fits && (0..8).all(|i| rom.u8(palettes + i) < 16)).then_some((metatile, tiles, palettes, across, down))
        };
        let mut found: Vec<Animation> = Vec::new();
        let mut o = 0;
        while o + 12 <= rom.len() {
            let run = (0..).map_while(|i| door(o + i * 12)).collect::<Vec<_>>();
            // a table, when it ends as the games end theirs
            if run.len() >= 4 && (0..12).all(|i| rom.u8(o + run.len() * 12 + i) == 0) {
                for &(metatile, tiles, palettes, across, down) in &run {
                    let per_frame = across * down * 4;
                    let frames: Vec<usize> = (0..3).map(|frame| tiles + frame * per_frame * TILE_BYTES).collect();
                    // a frame's tile is drawn in the palette of its place in its metatile: the top one's, the bottom one's
                    let numbers: Vec<u8> = (0..per_frame).map(|tile| rom.u8(palettes + tile % (down * 4))).collect();
                    match found.iter_mut().find(|known| known.table == o && known.frames == frames && known.palettes == numbers) {
                        Some(Animation { kind: Kind::Door { metatiles, .. }, .. }) => metatiles.push(metatile),
                        _ => found.push(Animation {
                            kind: Kind::Door { metatiles: vec![metatile], across, down },
                            frames,
                            sequence: vec![0, 1, 2],
                            tiles: per_frame,
                            table: o,
                            also: Vec::new(),
                            palettes: numbers,
                        }),
                    }
                }
            }
            o += (run.len() * 12).max(4);
        }
        found
    }

    /// The tiles the tilesets keep changing: each tileset's routine is followed to the
    /// tables of frames it copies, and to where it copies them
    fn animated_tiles(&self) -> Vec<Animation> {
        let rom = &self.rom;
        let code = |value: u32| data(rom, value.wrapping_sub(1)).filter(|_| value & 1 == 1);
        // the routines that copy frames, one per animation, each with the first tileset that runs it
        let mut routines: Vec<(usize, usize)> = Vec::new();
        let mut walked: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (t, _) in self.tilesets().into_iter().chain(self.original_tilesets()) {
            let Some(start) = code(rom.u32(t.offset + self.format.tileset_callback)) else { continue };
            // the tileset's routine sets another to be run each frame, which calls one per animation
            let called = walked.entry(start).or_insert_with(|| {
                let each_frame: Vec<usize> = walk(rom, start).literals.into_iter().filter_map(code).collect();
                each_frame.into_iter().flat_map(|routine| walk(rom, routine).calls).map(|(routine, _)| routine).collect()
            });
            for &routine in called.iter() {
                if !routines.iter().any(|&(_, known)| known == routine) {
                    routines.push((t.offset, routine));
                }
            }
        }
        // (the tileset, the table of frames, where they're copied, how many bytes of them)
        let mut found: Vec<(usize, usize, Vec<u32>, Option<u32>)> = Vec::new();
        let mut tables: BTreeSet<usize> = BTreeSet::new();
        for (tileset, routine) in routines {
            let copies = walk(rom, routine);
            let pointed: Vec<usize> = copies.literals.iter().filter_map(|&value| data(rom, value)).collect();
            // a table of frames holds addresses in the ROM
            let frames: Vec<usize> = pointed.iter().copied().filter(|&o| data(rom, rom.u32(o)).is_some()).collect();
            // where they're copied: addresses in the console's video memory, told as they are or in a table of them
            let told: Vec<Vec<u32>> = copies.literals.iter().filter(|&&value| video(value)).map(|&value| vec![value]).collect();
            // (such a table runs until the next thing the routine points to)
            let until = |o: usize| pointed.iter().copied().filter(|&next| next > o).min().unwrap_or(usize::MAX);
            let listed = pointed.iter().map(|&o| {
                let entries = (0..16).take_while(|i| o + (i + 1) * 4 <= until(o)).map(|i| rom.u32(o + i * 4));
                entries.take_while(|&value| video(value)).collect::<Vec<u32>>()
            });
            let places: Vec<Vec<u32>> = if told.is_empty() { listed.filter(|places| !places.is_empty()).collect() } else { told };
            // each copy tells how many bytes: the first table's frames with the first, and so on
            let copied = |n: &u32| (32..=4096).contains(n) && n.is_multiple_of(32);
            let sizes: Vec<u32> = copies.calls.iter().filter_map(|&(_, third)| third).filter(copied).collect();
            tables.extend(pointed.iter().copied());
            let first = found.len();
            for (i, &table) in frames.iter().enumerate().filter(|_| !places.is_empty()) {
                // a table goes with the place loaded with it; the last one, with all those left
                let its = if places.len() > frames.len() && i + 1 == frames.len() {
                    places[i..].concat()
                } else {
                    places[i % places.len()].clone()
                };
                // the same table again in the routine: its frames are copied to more than a place
                match found[first..].iter_mut().find(|known| known.1 == table) {
                    Some(known) => known.2.extend(its),
                    None => found.push((tileset, table, its, sizes.get(i).or(sizes.last()).copied())),
                }
            }
        }
        let mut animations: Vec<Animation> = Vec::new();
        for (tileset, table, places, bytes) in found {
            // the table runs until what isn't an address, or until the next table
            let end = tables.range(table + 1..).next().copied().unwrap_or(usize::MAX);
            let entries: Vec<usize> =
                (0..64).take_while(|i| table + (i + 1) * 4 <= end).map_while(|i| data(rom, rom.u32(table + i * 4))).collect();
            let mut frames: Vec<usize> = entries.clone();
            frames.sort_unstable();
            frames.dedup();
            // a frame is as many bytes as are copied, which is no more than there are until the next
            // frame; when that isn't told, as far as the next frame is
            let apart = frames.windows(2).map(|pair| pair[1] - pair[0]).min();
            let told = bytes.map(|n| n as usize).filter(|&n| apart.is_none_or(|apart| n <= apart));
            let Some(bytes) = told.or(apart).filter(|n| n.is_multiple_of(TILE_BYTES) && *n > 0) else { continue };
            if frames.iter().any(|&frame| frame + bytes > rom.len()) || frames.is_empty() {
                continue;
            }
            let mut over: Vec<usize> = Vec::new();
            for tile in places.iter().map(|place| (place - 0x0600_0000) as usize / TILE_BYTES) {
                if !over.contains(&tile) {
                    over.push(tile);
                }
            }
            animations.push(Animation {
                kind: Kind::Tiles { tileset, tile: over[0] },
                sequence: entries.iter().map(|entry| frames.binary_search(entry).unwrap_or(0)).collect(),
                frames,
                tiles: bytes / TILE_BYTES,
                table,
                also: over[1..].to_vec(),
                palettes: Vec::new(),
            });
        }
        animations
    }

    /// Every animation found in the ROM: the tilesets' animated tiles, then the doors.
    /// A Game Boy Color game's aren't looked for.
    pub fn animations(&self) -> Vec<Animation> {
        if self.format.block_size() > 1 {
            return Vec::new();
        }
        let mut found = self.animated_tiles();
        found.extend(self.doors());
        found
    }

    /// Where every tile of every frame of the ROM's animations is
    pub(crate) fn frame_tiles(&self) -> BTreeSet<usize> {
        let animations = self.animations();
        animations.iter().flat_map(|a| a.frames.iter().flat_map(|&frame| (0..a.tiles).map(move |tile| frame + tile * TILE_BYTES))).collect()
    }

    /// Is an animation one of a pair of tilesets'? Tiles that change are a tileset's, and
    /// its copies' (the same routine changes them); a door is a pair's when one of the
    /// pair's metatiles is that door.
    pub fn animates(&self, animation: &Animation, pair: Pair) -> bool {
        match &animation.kind {
            Kind::Tiles { tileset, .. } => {
                let routine = |t: usize| self.rom.u32(t + self.format.tileset_callback);
                [pair.primary, pair.secondary].into_iter().any(|t| t == *tileset || routine(t) == routine(*tileset))
            }
            Kind::Door { metatiles, .. } => metatiles.iter().any(|&id| {
                self.locate(pair, id).is_some()
                    && self.format.behavior_name(self.behavior(pair, id)).is_some_and(|name| name.contains("DOOR"))
            }),
        }
    }

    /// The pair to draw an animation with: the one given when the animation is its,
    /// or the first that it's one of
    pub fn animation_pair(&self, animation: &Animation, current: Pair) -> Pair {
        if self.animates(animation, current) {
            return current;
        }
        match animation.kind {
            Kind::Tiles { tileset, .. } => self.pair_with(tileset).map_or(current, |(pair, _)| pair),
            Kind::Door { .. } => self.pairs().into_iter().find(|&pair| self.animates(animation, pair)).unwrap_or(current),
        }
    }

    /// The palette each tile of a frame is drawn in: a door's are told with it; a
    /// tileset's tiles are in the palettes the metatiles draw the tiles they're copied over in
    pub fn animation_palettes(&self, animation: &Animation, usage: &TileUsage) -> Vec<u8> {
        match animation.kind {
            Kind::Door { .. } => animation.palettes.clone(),
            Kind::Tiles { tileset, tile } => {
                let first = self.format.primary_tiles;
                (0..animation.tiles).map(|i| usage.shown(tileset, if tile + i >= first { tile + i - first } else { tile + i })).collect()
            }
        }
    }

    /// A frame, pixel by pixel, row after row: the colour's number in its tile's palette, and that palette
    pub fn frame_pixels(&self, animation: &Animation, frame: usize, palettes: &[u8]) -> Vec<(u8, u8)> {
        let (width, height) = animation.size();
        let mut pixels = vec![(0, 0); width * height];
        let Some(&at) = animation.frames.get(frame) else { return pixels };
        for tile in 0..animation.tiles {
            let (left, top) = animation.place(tile);
            let palette = palettes.get(tile).copied().unwrap_or(0);
            for i in 0..TILE * TILE {
                let byte = self.rom.u8(at + tile * TILE_BYTES + i / 2);
                pixels[(top + i / TILE) * width + left + i % TILE] = (if i % 2 == 0 { byte & 0xF } else { byte >> 4 }, palette);
            }
        }
        pixels
    }

    /// Draws on a frame: each pixel `(x, y, colour)` of its picture, the colour one of
    /// the 16 of its tile's palette (0: none). The frame is written where it is in the
    /// ROM: every door, every tileset that has it shows it. Returns whether anything
    /// changed; to be committed.
    pub fn draw_on_frame(&mut self, animation: &Animation, frame: usize, pixels: &[(usize, usize, u8)]) -> bool {
        let Some(&at) = animation.frames.get(frame) else { return false };
        let mut changed = false;
        for &(x, y, color) in pixels {
            let Some(tile) = animation.tile_at(x, y) else { continue };
            self.drawn_graphics.insert(at + tile * TILE_BYTES);
            let o = at + tile * TILE_BYTES + (y % TILE) * 4 + (x % TILE) / 2;
            let byte = self.rom.u8(o);
            let drawn = if x % 2 == 0 { (byte & 0xF0) | (color & 0xF) } else { (byte & 0x0F) | ((color & 0xF) << 4) };
            changed |= drawn != byte;
            self.rom.write8(o, drawn);
        }
        changed
    }
}
