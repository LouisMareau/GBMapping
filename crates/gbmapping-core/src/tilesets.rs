//! Whole tilesets: a copy of one, a new one, and which ones a map is drawn with.
//!
//! A tileset is a small structure in the ROM that points to its tiles, its palettes,
//! its metatiles and their attributes; a map's layout points to two of them. A new one
//! is written whole at the ROM's unused end, and a layout pointed to it: the maps
//! drawn from that layout then have a tileset to themselves, to change without
//! changing any other map.

use crate::format::{ENTRIES, METATILE_BYTES, TILESET_SIZE};
use crate::game::Game;
use crate::metatiles::GrowError;
use crate::rom::{lz77_compress, GBA_ROM_BASE};
use crate::tiles::TILE_BYTES;

/// What a tileset is made of
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilesetContent {
    pub secondary: bool,
    /// The routine that animates its tiles, as the ROM points to it (0: none)
    pub callback: u32,
    /// Its tiles, 32 bytes each
    pub tiles: Vec<u8>,
    /// Its 16 palettes of 16 colours, one after the other
    pub palettes: Vec<u16>,
    pub metatiles: Vec<[u16; ENTRIES]>,
    pub attributes: Vec<u32>,
}

impl Game {
    /// Everything a tileset holds. `primary`: whether it's drawn with as a primary one.
    pub fn tileset_content(&self, offset: usize, primary: bool) -> Option<TilesetContent> {
        let t = self.tileset(offset)?;
        let count = self.tileset_metatiles(&t, primary);
        let metatiles: Vec<[u16; ENTRIES]> = (0..count).map(|i| self.tileset_entries(&t, i)).collect();
        let mut tiles = (*self.tiles(offset, primary)).clone();
        if !t.compressed && !primary {
            // tiles that aren't compressed end nowhere that can be told: a secondary
            // tileset's are those its metatiles draw (a primary one's, others draw too)
            let (first, most) = (self.format.primary_tiles, self.format.secondary_tiles());
            let drawn = metatiles.iter().flatten().filter_map(|e| ((e & 0x3FF) as usize).checked_sub(first)).filter(|&tile| tile < most);
            tiles.truncate(drawn.max().map_or(1, |last| last + 1) * TILE_BYTES);
        }
        Some(TilesetContent {
            secondary: !primary,
            callback: self.rom.u32(offset + self.format.tileset_callback),
            tiles,
            palettes: (0..256).map(|i| self.rom.u16(t.palettes + i * 2)).collect(),
            attributes: (0..count).map(|i| self.tileset_attribute(&t, i)).collect(),
            metatiles,
        })
    }

    /// Writes a new tileset at the ROM's unused end: where it is. No map is drawn with
    /// it until a layout is given it (`set_layout_tileset`). To be committed.
    pub fn create_tileset(&mut self, content: &TilesetContent) -> Result<usize, GrowError> {
        const MARGIN: usize = 256;
        const SPARE: usize = 16;
        if self.format.block_size() > 1 {
            return Err(GrowError::Blocks);
        }
        let f = self.format;
        // one metatile at least: with none, where they end couldn't be told
        let count = content.metatiles.len().max(1);
        let most = if content.secondary { f.secondary_metatiles() } else { f.primary_metatiles };
        if count > most {
            return Err(GrowError::Full { metatiles: most });
        }
        let empty = [0u8; TILE_BYTES];
        let packed = lz77_compress(if content.tiles.is_empty() { &empty } else { &content.tiles });
        // the structure, the palettes, the tiles, then the metatiles and their
        // attributes last: more of those can then be added where they are
        let at = (self.rom.free_tail() + MARGIN).next_multiple_of(4);
        let palettes = at + TILESET_SIZE;
        let tiles = palettes + 16 * 32;
        let metatiles = (tiles + packed.len()).next_multiple_of(4);
        let attributes = metatiles + count * METATILE_BYTES;
        if attributes + count * f.attribute_size + SPARE > self.rom.len() {
            return Err(GrowError::NoRoom);
        }
        let rom = &mut self.rom;
        rom.write32(at, 1 | ((content.secondary as u32) << 8));
        for (field, to) in [(4, tiles), (8, palettes), (12, metatiles), (f.tileset_attributes, attributes)] {
            rom.write32(at + field, GBA_ROM_BASE + to as u32);
        }
        rom.write32(at + f.tileset_callback, content.callback);
        for i in 0..256 {
            rom.write16(palettes + i * 2, content.palettes.get(i).copied().unwrap_or(0));
        }
        for (i, &b) in packed.iter().enumerate() {
            rom.write8(tiles + i, b);
        }
        for i in 0..count {
            for (k, &e) in content.metatiles.get(i).unwrap_or(&[0; ENTRIES]).iter().enumerate() {
                rom.write16(metatiles + i * METATILE_BYTES + k * 2, e);
            }
            let attribute = content.attributes.get(i).copied().unwrap_or(0);
            match f.attribute_size {
                4 => rom.write32(attributes + i * 4, attribute),
                _ => rom.write16(attributes + i * 2, attribute as u16),
            }
        }
        self.forget_tiles();
        Ok(at)
    }

    /// A copy of a tileset, new in the ROM: its tiles, its palettes, its metatiles, its
    /// animation. Where the copy is; to be given to a layout, and committed.
    pub fn duplicate_tileset(&mut self, offset: usize, primary: bool) -> Result<usize, GrowError> {
        let content = self.tileset_content(offset, primary).ok_or(GrowError::Nothing)?;
        let copy = self.create_tileset(&content)?;
        self.copies.insert(copy, offset);
        Ok(copy)
    }

    /// A new secondary tileset with nothing in it but the palettes of another (`like`),
    /// one empty tile and one empty metatile. Where it is; to be given to a layout, and committed.
    pub fn empty_tileset(&mut self, like: usize) -> Result<usize, GrowError> {
        let palettes = self.tileset(like).map_or(Vec::new(), |t| (0..256).map(|i| self.rom.u16(t.palettes + i * 2)).collect());
        self.create_tileset(&TilesetContent {
            secondary: true,
            callback: 0,
            tiles: Vec::new(),
            palettes,
            metatiles: Vec::new(),
            attributes: Vec::new(),
        })
    }

    /// The tileset a tileset made here is the copy of
    pub fn copied_from(&self, offset: usize) -> Option<usize> {
        self.copies.get(&offset).copied()
    }

    /// Was a tileset made since the ROM was opened?
    pub fn is_new_tileset(&self, offset: usize) -> bool {
        self.original_tileset(offset).is_none()
    }

    /// Gives a layout another primary or secondary tileset: every map drawn from that
    /// layout is then drawn with it. To be committed.
    pub fn set_layout_tileset(&mut self, layout: usize, primary: bool, tileset: usize) -> bool {
        if self.format.block_size() > 1 || self.tileset(tileset).is_none() || !self.maps.iter().any(|m| m.layout.offset == layout) {
            return false;
        }
        self.rom.write32(layout + if primary { 16 } else { 20 }, GBA_ROM_BASE + tileset as u32);
        self.refresh_layouts();
        true
    }
}
