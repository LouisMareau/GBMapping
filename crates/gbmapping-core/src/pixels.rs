//! Drawing on tiles, a pixel at a time, through the metatile they're seen in.
//!
//! A metatile's layer is 16x16 pixels: four tiles of 8x8, each drawn in one of the
//! pair's palettes, maybe flipped. A pixel is one of that palette's 16 colours, the
//! first being none (see-through). But a tile isn't a metatile's own: other metatiles
//! draw it too, and the same metatile may draw it in several of its quarters. So a
//! quarter that shares its tile is given a copy of it first, new in the pair's
//! secondary tileset, unless the tile itself is what's to be changed, everywhere.

use crate::game::Game;
use crate::gfx::{TileEntry, METATILE, TILE};
use crate::structs::{Pair, Tileset};
use crate::tiles::{BorrowError, TILE_BYTES};
use std::collections::BTreeSet;
use std::fmt;

/// Why nothing was drawn
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawError {
    /// No such metatile, or no such tile
    Nothing,
    /// A primary tileset's metatile: it can't be given tiles of its own, which would be
    /// the secondary tileset's
    Primary,
    /// A game whose tiles' pictures the editor doesn't write
    Fixed,
    /// The tiles couldn't be written
    Tiles(BorrowError),
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Nothing => write!(f, "There's no such tile to draw on."),
            Self::Primary => write!(
                f,
                "This metatile is the primary tileset's, and shares its tiles with others: Duplicate it to draw on a copy, or turn \"Only this metatile\" off to change the tiles themselves, on every map."
            ),
            Self::Fixed => write!(f, "This game's tiles are shown, not drawn on."),
            Self::Tiles(e) => e.fmt(f),
        }
    }
}

/// A pixel of a metatile's layer: the colour's number in its quarter's palette (0: none), and that palette
pub type LayerPixels = [(u8, u8); METATILE * METATILE];

impl Game {
    /// A metatile's layer (0: bottom, 1: top), pixel by pixel as it's drawn
    pub fn layer_pixels(&self, pair: Pair, id: u16, layer: usize) -> LayerPixels {
        let mut pixels = [(0, 0); METATILE * METATILE];
        let (gfx, entries) = (self.gfx(pair), self.entries(pair, id));
        for q in 0..4 {
            let e = TileEntry::unpack(entries[(layer & 1) * 4 + q]);
            let tile = gfx.tile(e.tile);
            for y in 0..TILE {
                for x in 0..TILE {
                    let (sx, sy) = (if e.hflip { TILE - 1 - x } else { x }, if e.vflip { TILE - 1 - y } else { y });
                    pixels[((q / 2) * TILE + y) * METATILE + (q % 2) * TILE + x] = (tile[sy * TILE + sx], e.palette);
                }
            }
        }
        pixels
    }

    /// How many quarters of metatiles draw a tile of a pair (`tile`: as metatiles number
    /// it). A primary tileset's tile is counted in every tileset that goes with it.
    pub fn tile_entries(&self, pair: Pair, tile: u16) -> usize {
        let count = |offset: usize, primary: bool| -> usize {
            let Some(t) = self.tileset(offset) else { return 0 };
            (0..self.tileset_metatiles(&t, primary))
                .map(|i| self.tileset_entries(&t, i).iter().filter(|&&e| e & 0x3FF == tile).count())
                .sum()
        };
        let secondaries: BTreeSet<usize> = if (tile as usize) < self.format.primary_tiles {
            self.pairs().into_iter().filter(|p| p.primary == pair.primary).map(|p| p.secondary).chain([pair.secondary]).collect()
        } else {
            BTreeSet::from([pair.secondary])
        };
        count(pair.primary, true) + secondaries.into_iter().filter(|&s| s != pair.primary).map(|s| count(s, false)).sum::<usize>()
    }

    /// How many tiles a pair's secondary tileset can still be given: the room it has
    /// left, and the tiles no metatile draws
    pub fn free_tiles(&self, pair: Pair) -> usize {
        match self.tileset(pair.secondary) {
            Some(t) if t.compressed => {
                let data = self.tiles(pair.secondary, false);
                self.tile_room().saturating_sub(data.len() / TILE_BYTES) + self.idle_tiles(pair, &data).len()
            }
            _ => 0,
        }
    }

    /// Gives one of a metatile's 8 tile entries a tile of its own: a copy of its tile as
    /// it's drawn there (its flips undone), new at the end of the pair's secondary
    /// tileset, or in place of a tile no metatile draws when it has no room left.
    /// Returns the tile; to be committed.
    pub fn own_tile(&mut self, pair: Pair, id: u16, entry: usize) -> Result<u16, DrawError> {
        if (id as usize) < self.format.primary_metatiles {
            return Err(DrawError::Primary);
        }
        let (t, local) = self.locate(pair, id).ok_or(DrawError::Nothing)?;
        let mut entries = self.tileset_entries(&t, local);
        let e = TileEntry::unpack(*entries.get(entry).ok_or(DrawError::Nothing)?);
        let seen = self.gfx(pair).tile(e.tile);
        let mut copy = [0u8; TILE_BYTES];
        for y in 0..TILE {
            for x in 0..TILE {
                let (sx, sy) = (if e.hflip { TILE - 1 - x } else { x }, if e.vflip { TILE - 1 - y } else { y });
                copy[y * 4 + x / 2] |= seen[sy * TILE + sx] << (x % 2 * 4);
            }
        }
        let secondary = self.tileset(pair.secondary).ok_or(DrawError::Nothing)?;
        let mut data = (*self.tiles(pair.secondary, false)).clone();
        let count = data.len() / TILE_BYTES;
        // tiles that aren't compressed can't be counted, nor added to
        if !secondary.compressed {
            return Err(DrawError::Tiles(BorrowError::NotCompressed));
        }
        let slot = self.tile_slot(pair, &data).ok_or(DrawError::Tiles(BorrowError::Full { tiles: count }))?;
        data.truncate(count * TILE_BYTES);
        data.resize(data.len().max((slot + 1) * TILE_BYTES), 0);
        data[slot * TILE_BYTES..(slot + 1) * TILE_BYTES].copy_from_slice(&copy);
        self.store_tiles(&secondary, &data).map_err(DrawError::Tiles)?;
        let tile = (self.format.primary_tiles + slot) as u16;
        entries[entry] = TileEntry { tile, hflip: false, vflip: false, palette: e.palette }.pack();
        self.set_tileset_entries(&t, local, &entries);
        Ok(tile)
    }

    /// Draws on a metatile's layer: each pixel `(x, y, colour)`, `x` and `y` from 0 to
    /// 15, the colour one of the 16 of the palette its quarter is drawn in (0: none).
    /// `own`: a quarter whose tile is drawn elsewhere too is given a tile of its own
    /// first, so that nothing but this metatile changes; else the tiles themselves are
    /// drawn on, and every metatile that has them shows it.
    /// Returns whether anything changed; to be committed.
    pub fn draw_on_metatile(
        &mut self,
        pair: Pair,
        id: u16,
        layer: usize,
        pixels: &[(usize, usize, u8)],
        own: bool,
    ) -> Result<bool, DrawError> {
        if self.format.block_size() > 1 {
            return Err(DrawError::Fixed);
        }
        let (t, local) = self.locate(pair, id).filter(|_| layer < 2).ok_or(DrawError::Nothing)?;
        // a pixel that has its colour already is nothing to do, and no reason to copy a tile
        let now = self.layer_pixels(pair, id, layer);
        let in_layer = |&(x, y, c): &(usize, usize, u8)| x < METATILE && y < METATILE && now[y * METATILE + x].0 != c & 0xF;
        let wanted: Vec<(usize, usize, u8)> = pixels.iter().copied().filter(in_layer).collect();
        if wanted.is_empty() {
            return Ok(false);
        }
        if own {
            let quarters: BTreeSet<usize> = wanted.iter().map(|&(x, y, _)| (y / TILE) * 2 + x / TILE).collect();
            for q in quarters {
                let tile = self.tileset_entries(&t, local)[layer * 4 + q] & 0x3FF;
                if self.tile_entries(pair, tile) > 1 {
                    self.own_tile(pair, id, layer * 4 + q)?;
                }
            }
        }
        let entries = self.tileset_entries(&t, local).map(TileEntry::unpack);
        let first = self.format.primary_tiles;
        // the pair's two tilesets' tiles, as far as they're drawn on
        let mut drawn: [Option<Vec<u8>>; 2] = [None, None];
        for (x, y, color) in wanted {
            let e = entries[layer * 4 + (y / TILE) * 2 + x / TILE];
            let (x, y) = (x % TILE, y % TILE);
            let (x, y) = (if e.hflip { TILE - 1 - x } else { x }, if e.vflip { TILE - 1 - y } else { y });
            let secondary = (e.tile as usize) >= first;
            let tile = if secondary { e.tile as usize - first } else { e.tile as usize };
            let (offset, most) = if secondary { (pair.secondary, self.format.secondary_tiles()) } else { (pair.primary, first) };
            if tile >= most {
                return Err(DrawError::Nothing);
            }
            let data = drawn[secondary as usize].get_or_insert_with(|| (*self.tiles(offset, !secondary)).clone());
            if data.len() < (tile + 1) * TILE_BYTES {
                data.resize((tile + 1) * TILE_BYTES, 0);
            }
            let byte = &mut data[tile * TILE_BYTES + y * 4 + x / 2];
            *byte = if x % 2 == 0 { (*byte & 0xF0) | (color & 0xF) } else { (*byte & 0x0F) | ((color & 0xF) << 4) };
        }
        for (secondary, data) in drawn.into_iter().enumerate() {
            let Some(data) = data else { continue };
            let tileset = self.tileset(if secondary == 1 { pair.secondary } else { pair.primary }).ok_or(DrawError::Nothing)?;
            self.put_tiles(&tileset, secondary == 0, &data).map_err(DrawError::Tiles)?;
        }
        Ok(true)
    }

    /// Writes a tileset's tiles: compressed and stored again, or where they are when
    /// they aren't compressed (as many as a tileset of its kind holds)
    pub(crate) fn put_tiles(&mut self, t: &Tileset, primary: bool, data: &[u8]) -> Result<(), BorrowError> {
        if t.compressed {
            return self.store_tiles(t, data);
        }
        let most = if primary { self.format.primary_tiles } else { self.format.secondary_tiles() };
        for (i, &b) in data.iter().enumerate().take(most * TILE_BYTES) {
            self.rom.write8(t.tiles + i, b);
        }
        self.forget_tiles();
        Ok(())
    }

    /// Changes one of a pair's colours: number `index` of its palette `palette` (15 bits:
    /// red, green, blue, 5 each). Every tile drawn in that palette shows it, on every
    /// map of the tileset the palette is of. To be committed.
    pub fn set_color(&mut self, pair: Pair, palette: usize, index: usize, color: u16) -> bool {
        let offset = if palette < self.format.primary_palettes { pair.primary } else { pair.secondary };
        let known = palette < 16 && index < 16 && self.format.block_size() == 1;
        let Some(t) = self.tileset(offset).filter(|_| known) else { return false };
        self.rom.write16(t.palettes + (palette * 16 + index) * 2, color & 0x7FFF);
        true
    }
}
