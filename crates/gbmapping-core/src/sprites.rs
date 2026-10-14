//! The overworld sprites: how the maps' people and things look in the game.
//!
//! The games keep a table of pointers to struct ObjectEventGraphicsInfo, one per
//! graphics id (an object event's `graphics`), and a table of the sprites' palettes by
//! tag. Neither is at a known place: both are found by their shape (the longest run of
//! entries that are such things). A sprite is drawn as its first frame: the person
//! facing down.

use crate::game::{Game, Image};
use crate::gfx::{color555, Palette, TILE};
use crate::rom::Rom;
use std::collections::HashMap;

const INFO_SIZE: usize = 0x24;
/// No table is shorter than this
const FEWEST: usize = 24;

#[derive(Clone, Debug, Default)]
pub struct Sprites {
    /// Where each graphics id's struct ObjectEventGraphicsInfo is (none: an empty entry)
    infos: Vec<Option<usize>>,
    /// The palettes, by tag
    palettes: HashMap<u16, Palette>,
}

impl Sprites {
    /// How many sprites there are
    pub fn count(&self) -> usize {
        self.infos.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }
}

/// A struct ObjectEventGraphicsInfo at `o`: its width, its palette's tag, where its
/// first frame's tiles are and how many bytes they take (the frame may be less tall
/// than the sprite: a berry tree's first)
fn info(rom: &Rom, o: usize) -> Option<(usize, u16, usize, usize)> {
    if !o.is_multiple_of(4) || o + INFO_SIZE > rom.len() || rom.u16(o) != 0xFFFF {
        return None;
    }
    let (width, height) = (rom.u16(o + 8) as usize, rom.u16(o + 10) as usize);
    // (the ships are wider than the console's sprites: drawn as several, from one picture)
    let fits = |side: usize| (8..=128).contains(&side) && side.is_multiple_of(8);
    if !fits(width) || !fits(height) {
        return None;
    }
    rom.ptr(o + 0x10)?;
    let images = rom.ptr(o + 0x1C).filter(|p| p.is_multiple_of(4))?;
    let size = rom.u16(images + 4) as usize;
    let tiles = rom.ptr(images).filter(|&p| p + size <= rom.len())?;
    (size > 0 && size.is_multiple_of(width * 4) && size <= width * height / 2).then_some((width, rom.u16(o + 2), tiles, size))
}

/// The table of the sprites: the longest run of pointers to struct
/// ObjectEventGraphicsInfo, empty entries (NULL) allowed between them. Where it is, and
/// each entry's struct.
fn sprite_table(rom: &Rom) -> Vec<Option<usize>> {
    /// A run of the table: where it begins, where its last struct is told, how many structs
    #[derive(Clone, Copy, Default)]
    struct Run {
        start: usize,
        last: usize,
        count: usize,
    }
    let (mut best, mut run, mut nulls) = (Run::default(), None::<Run>, 0);
    for o in (0..rom.len().saturating_sub(4)).step_by(4) {
        if rom.ptr(o).is_some_and(|p| info(rom, p).is_some()) {
            let start = run.map_or(o, |run| run.start);
            let count = run.map_or(0, |run| run.count) + 1;
            run = Some(Run { start, last: o, count });
            nulls = 0;
            if count > best.count {
                best = Run { start, last: o, count };
            }
        } else if run.is_some() && rom.u32(o) == 0 && nulls < 32 {
            nulls += 1;
        } else {
            run = None;
        }
    }
    if best.count < FEWEST {
        return Vec::new();
    }
    (best.start..=best.last).step_by(4).map(|o| rom.ptr(o)).collect()
}

/// The longest run of entries of `size` bytes for which `ok` holds
fn longest_run(rom: &Rom, size: usize, ok: impl Fn(usize) -> bool) -> (usize, usize) {
    let (mut best, mut start, mut length) = ((0, 0), 0, 0);
    let mut o = 0;
    while o + size <= rom.len() {
        if ok(o) {
            if length == 0 {
                start = o;
            }
            length += 1;
            if length > best.1 {
                best = (start, length);
            }
            o += size;
        } else {
            // a run that broke may have begun a word later
            o = if length > 0 { start + 4 } else { o + 4 };
            length = 0;
        }
    }
    best
}

impl Game {
    /// The overworld sprites, found in the ROM (none when they aren't)
    pub fn sprites(&self) -> Sprites {
        if self.format.block_size() > 1 {
            return Sprites::default();
        }
        let rom = &self.rom;
        let infos = sprite_table(rom);
        // the palettes: their colours, a tag in the ranges of the sprites' (their own, their
        // reflections' in water; a hack's own range), and nothing
        let ranges: Vec<u16> = infos
            .iter()
            .flatten()
            .filter(|&&o| info(rom, o).is_some())
            .flat_map(|&o| [rom.u16(o + 2) & 0xFF00, rom.u16(o + 4) & 0xFF00])
            .filter(|&range| range != 0xFF00)
            .collect();
        let entry = |o: usize| {
            rom.ptr(o).is_some_and(|p| p + 32 <= rom.len()) && rom.u16(o + 6) == 0 && ranges.contains(&(rom.u16(o + 4) & 0xFF00))
        };
        let (palettes_at, palette_count) = if infos.is_empty() { (0, 0) } else { longest_run(rom, 8, entry) };
        let palettes = (0..palette_count)
            .filter_map(|i| {
                let o = palettes_at + i * 8;
                let colors = rom.ptr(o)?;
                Some((rom.u16(o + 4), std::array::from_fn(|c| color555(rom.u16(colors + c * 2)))))
            })
            .collect();
        Sprites { infos, palettes }
    }

    /// A graphics id's sprite, facing down: color 0 is see-through. A sprite whose
    /// palette isn't found is grey.
    pub fn sprite(&self, sprites: &Sprites, graphics: u16) -> Option<Image> {
        let rom = &self.rom;
        let (width, tag, tiles, size) = info(rom, (*sprites.infos.get(graphics as usize)?)?)?;
        let height = size * 2 / width;
        let grey: Palette = std::array::from_fn(|c| [(c * 16) as u8; 3]);
        let palette = sprites.palettes.get(&tag).unwrap_or(&grey);
        let mut image = Image::new(width, height);
        let across = width / TILE;
        for y in 0..height {
            for x in 0..width {
                let tile = (y / TILE) * across + x / TILE;
                let byte = rom.u8(tiles + tile * 32 + (y % TILE) * 4 + (x % TILE) / 2);
                let color = if x % 2 == 0 { byte & 0xF } else { byte >> 4 };
                if color != 0 {
                    let [r, g, b] = palette[color as usize];
                    let o = (y * width + x) * 4;
                    image.rgba[o..o + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        Some(image)
    }
}
