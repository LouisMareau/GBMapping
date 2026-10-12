//! Tiles: the 8x8 pieces metatiles are made of.
//!
//! A tile has no colours of its own: each metatile that uses it says in which of the
//! pair's palettes. So a tile is shown in the palette it's used with the most. And a
//! metatile can only use the tiles of its own two tilesets: to build one with a tile of
//! another tileset, the tile is copied into the map's secondary tileset, with colours
//! that are the same or as close as the pair's palettes allow.

use crate::format::block;
use crate::game::{Game, Image};
use crate::gfx::{self, TileEntry, TilePixels, TILE};
use crate::rom::{lz77_compress, GBA_ROM_BASE};
use crate::structs::{Pair, Tileset};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

pub(crate) const TILE_BYTES: usize = 32;
/// The tiles the games draw their opening doors into: the last ones a secondary
/// tileset may hold. A tile put there would be drawn over each time a door opens.
pub const DOOR_TILES: usize = 16;

/// How often the metatiles draw each tile of each tileset in each palette
#[derive(Clone, Debug, Default)]
pub struct TileUsage {
    by_tileset: HashMap<usize, Vec<[u32; 16]>>,
}

impl TileUsage {
    fn counts(&self, tileset: usize, tile: usize) -> Option<&[u32; 16]> {
        self.by_tileset.get(&tileset)?.get(tile)
    }

    /// How many times metatiles use a tile
    pub fn uses(&self, tileset: usize, tile: usize) -> u32 {
        self.counts(tileset, tile).map_or(0, |c| c.iter().sum())
    }

    /// The palette a tile is used with the most, if any metatile uses it
    pub fn usual(&self, tileset: usize, tile: usize) -> Option<u8> {
        let counts = self.counts(tileset, tile)?;
        let best = (0..16).max_by_key(|&p| (counts[p], std::cmp::Reverse(p)))?;
        (counts[best] > 0).then_some(best as u8)
    }

    /// The palette to show a tile in: its usual one; for a tile no metatile uses, that
    /// of the nearest tile that has one (tiles drawn together sit together)
    pub fn shown(&self, tileset: usize, tile: usize) -> u8 {
        let known = self.by_tileset.get(&tileset).map_or(0, Vec::len);
        let near = (1..=known.max(tile + 1)).flat_map(|d| [tile.checked_sub(d), Some(tile + d)]).flatten();
        std::iter::once(tile).chain(near).find_map(|t| self.usual(tileset, t)).unwrap_or(0)
    }
}

/// A tileset's tiles, each drawn in a palette
pub struct TileSheet {
    pub image: Image,
    pub count: usize,
    /// The palette each tile is drawn in
    pub palettes: Vec<u8>,
}

/// How a tile brought from another tileset got its colours
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colors {
    /// A palette of the pair has them
    Found,
    /// They were written to a palette of the secondary tileset that no metatile used
    Added,
    /// No palette has them and none is free: the closest colours of the closest palette
    Approximated,
}

/// A tile of another tileset, made usable in a pair
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Borrowed {
    /// Its id in the pair, and the palette to draw it in
    pub tile: u16,
    pub palette: u8,
    pub colors: Colors,
    /// The pair's tilesets had the very same tile: nothing was copied
    pub existing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowError {
    /// No such tile, or no such tileset
    Nothing,
    /// The secondary tileset has no tile left to give
    Full { tiles: usize },
    /// Its tiles aren't compressed: where they end can't be told
    NotCompressed,
    /// The ROM has no unused space left at its end
    NoRoom,
}

impl fmt::Display for BorrowError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Nothing => write!(f, "There's no such tile."),
            Self::Full { tiles } => {
                write!(f, "The map's secondary tileset has no tile left to give: it holds {tiles}, every one drawn by a metatile (the game keeps a tileset's last {DOOR_TILES} for the doors it opens).")
            }
            Self::NotCompressed => {
                write!(f, "The map's secondary tileset keeps its tiles uncompressed: where they end can't be told, so none can be added.")
            }
            Self::NoRoom => write!(f, "The ROM has no unused space left at its end for the tileset's tiles."),
        }
    }
}

/// Where a tileset's tiles go when they're written back
struct Placement {
    at: usize,
    packed: Vec<u8>,
    /// The tileset's pointer is to be changed
    moved: bool,
}

fn pack(pixels: &TilePixels) -> [u8; TILE_BYTES] {
    std::array::from_fn(|i| pixels[i * 2] | pixels[i * 2 + 1] << 4)
}

/// How far apart two 15-bit colours are
fn distance(a: u16, b: u16) -> u32 {
    (0..3).map(|c| ((a >> (c * 5) & 0x1F) as i32 - (b >> (c * 5) & 0x1F) as i32).pow(2) as u32).sum()
}

impl Game {
    /// The tileset pairs the maps' originals are on and no map is: what the ROM keeps of
    /// the tilesets its maps came with
    pub fn original_pairs(&self) -> BTreeSet<Pair> {
        let now = self.pairs();
        self.maps.iter().filter_map(|m| m.original).map(|l| l.pair()).filter(|pair| !now.contains(pair)).collect()
    }

    /// The tilesets only the maps' originals are on, and whether each is a primary one
    pub fn original_tilesets(&self) -> Vec<(Tileset, bool)> {
        let now: BTreeSet<usize> = self.tilesets().iter().map(|(t, _)| t.offset).collect();
        let mut seen = BTreeSet::new();
        for l in self.maps.iter().filter_map(|m| m.original) {
            seen.insert((l.primary, true));
            seen.insert((l.secondary, false));
        }
        let kept = seen.into_iter().filter(|(offset, _)| !now.contains(offset));
        kept.filter_map(|(offset, primary)| Some((self.tileset(offset)?, primary))).collect()
    }

    /// How the metatiles of the maps' tilesets (and of their originals') use their tiles
    pub fn tile_usage(&self) -> TileUsage {
        let f = self.format;
        let mut usage = TileUsage::default();
        let mut primaries = BTreeSet::new();
        for pair in self.pairs().into_iter().chain(self.original_pairs()) {
            let mut sets = vec![(pair.secondary, false)];
            if primaries.insert(pair.primary) {
                sets.push((pair.primary, true));
            }
            for (offset, primary) in sets {
                let Some(t) = self.tileset(offset) else { continue };
                for local in 0..self.tileset_metatiles(&t, primary) {
                    for e in self.tileset_entries(&t, local).map(TileEntry::unpack) {
                        let tile = e.tile as usize;
                        let (owner, tile) =
                            if tile < f.primary_tiles { (pair.primary, tile) } else { (pair.secondary, tile - f.primary_tiles) };
                        // what a primary tileset's metatile shows of a secondary's tiles changes with the map
                        if primary && owner != pair.primary {
                            continue;
                        }
                        let tiles = usage.by_tileset.entry(owner).or_default();
                        if tiles.len() <= tile {
                            tiles.resize(tile + 1, [0; 16]);
                        }
                        tiles[tile][e.palette as usize] += 1;
                    }
                }
            }
        }
        usage
    }

    /// How many tiles of the maps each metatile of each tileset is on
    pub fn metatile_uses(&self) -> HashMap<usize, Vec<u32>> {
        let first_secondary = self.format.primary_metatiles;
        let mut uses: HashMap<usize, Vec<u32>> = HashMap::new();
        let mut seen = BTreeSet::new();
        for l in self.maps.iter().map(|m| m.layout).filter(|l| seen.insert(l.offset)) {
            let blocks = (0..l.width * l.height).map(|i| l.blocks + i * 2);
            let border = (0..l.border_width * l.border_height).map(|i| l.border + i * 2);
            for at in blocks.chain(border) {
                let id = block::metatile(self.rom.u16(at)) as usize;
                let (tileset, local) = if id < first_secondary { (l.primary, id) } else { (l.secondary, id - first_secondary) };
                let counts = uses.entry(tileset).or_default();
                if counts.len() <= local {
                    counts.resize(local + 1, 0);
                }
                counts[local] += 1;
            }
        }
        // tilesets drawn from the same metatiles are on the maps together: a Game Boy Color
        // tileset is one per set of colours its maps give it
        let mut together: HashMap<usize, Vec<usize>> = HashMap::new();
        for &offset in uses.keys() {
            if let Some(t) = self.tileset(offset) {
                together.entry(t.metatiles).or_default().push(offset);
            }
        }
        for offsets in together.values().filter(|offsets| offsets.len() > 1) {
            let mut sum: Vec<u32> = Vec::new();
            for offset in offsets {
                let counts = &uses[offset];
                sum.resize(sum.len().max(counts.len()), 0);
                for (total, count) in sum.iter_mut().zip(counts) {
                    *total += count;
                }
            }
            for &offset in offsets {
                uses.insert(offset, sum.clone());
            }
        }
        uses
    }

    /// A pair of the maps' (else of their originals') that holds a tileset, and whether
    /// as its secondary one: the palettes the tileset's tiles are meant for
    pub fn pair_with(&self, tileset: usize) -> Option<(Pair, bool)> {
        self.pairs().into_iter().chain(self.original_pairs()).find_map(|p| {
            if p.secondary == tileset {
                Some((p, true))
            } else {
                (p.primary == tileset).then_some((p, false))
            }
        })
    }

    /// The tiles of a pair's secondary tileset that no metatile draws, the empty ones
    /// first: tiles to give away when the tileset has no room for more. `data`: its tiles.
    pub(crate) fn idle_tiles(&self, pair: Pair, data: &[u8]) -> Vec<usize> {
        let (first, count) = (self.format.primary_tiles, (data.len() / TILE_BYTES).min(self.tile_room()));
        let mut drawn = vec![false; count];
        // its own metatiles draw its tiles; the primary tileset's shouldn't, but are looked at
        for (offset, primary) in [(pair.secondary, false), (pair.primary, true)] {
            let Some(t) = self.tileset(offset) else { continue };
            for local in 0..self.tileset_metatiles(&t, primary) {
                for e in self.tileset_entries(&t, local) {
                    if let Some(tile) = ((e & 0x3FF) as usize).checked_sub(first).filter(|&tile| tile < count) {
                        drawn[tile] = true;
                    }
                }
            }
        }
        let empty = |tile: usize| data[tile * TILE_BYTES..(tile + 1) * TILE_BYTES].iter().all(|&b| b == 0);
        let mut idle: Vec<usize> = (0..count).filter(|&tile| !drawn[tile]).collect();
        idle.sort_by_key(|&tile| !empty(tile));
        idle
    }

    /// How many tiles a secondary tileset may hold: its share, less the doors'
    pub(crate) fn tile_room(&self) -> usize {
        self.format.secondary_tiles().saturating_sub(DOOR_TILES)
    }

    /// Where a pair's secondary tileset takes a new tile: after its last one while it
    /// has room, else in place of one no metatile draws. `None`: every tile is drawn.
    pub(crate) fn tile_slot(&self, pair: Pair, data: &[u8]) -> Option<usize> {
        let count = data.len() / TILE_BYTES;
        if count < self.tile_room() {
            return Some(count);
        }
        self.idle_tiles(pair, data).first().copied()
    }

    /// The pair's 16 palettes as the ROM has them (15-bit colours)
    pub fn raw_palettes(&self, pair: Pair) -> Vec<[u16; 16]> {
        let read = |offset: usize, p: usize| -> [u16; 16] {
            let palettes = self.tileset(offset).map(|t| t.palettes);
            std::array::from_fn(|c| palettes.map_or(0, |at| self.rom.u16(at + (p * 16 + c) * 2)))
        };
        (0..16).map(|p| read(if p < self.format.primary_palettes { pair.primary } else { pair.secondary }, p)).collect()
    }

    /// How many tiles one of a pair's tilesets holds
    pub fn tileset_tile_count(&self, pair: Pair, secondary: bool) -> usize {
        let (offset, most) =
            if secondary { (pair.secondary, self.format.secondary_tiles()) } else { (pair.primary, self.format.primary_tiles) };
        (self.tiles(offset, !secondary).len() / TILE_BYTES).min(most)
    }

    /// How many tiles one of a pair's tilesets holds up to the last one that draws
    /// something: many end with empty tiles
    pub fn tileset_drawn_tiles(&self, pair: Pair, secondary: bool) -> usize {
        let data = self.tiles(if secondary { pair.secondary } else { pair.primary }, !secondary);
        let count = self.tileset_tile_count(pair, secondary);
        (0..count).rev().find(|&t| data[t * TILE_BYTES..(t + 1) * TILE_BYTES].iter().any(|&b| b != 0)).map_or(0, |t| t + 1)
    }

    /// One of a pair's tilesets, tile by tile, `columns` to a row: each tile in the
    /// palette it's used with (`usage`), or all of them in one `palette`. What a tile
    /// leaves see-through is left so.
    pub fn tile_sheet(&self, pair: Pair, secondary: bool, usage: &TileUsage, palette: Option<u8>, columns: usize) -> TileSheet {
        let offset = if secondary { pair.secondary } else { pair.primary };
        let data = self.tiles(offset, !secondary);
        let count = self.tileset_tile_count(pair, secondary);
        let colors = self.palettes(pair);
        let usual = |t: usize| self.tile_palette(offset, t).unwrap_or_else(|| usage.shown(offset, t));
        let palettes: Vec<u8> = (0..count).map(|t| palette.unwrap_or_else(|| usual(t)) & 0xF).collect();
        let mut image = Image::new(columns * TILE, count.div_ceil(columns).max(1) * TILE);
        for (t, &p) in palettes.iter().enumerate() {
            let pixels = gfx::tile_pixels(&data, t);
            gfx::draw_tile(
                &mut image.rgba,
                image.width,
                &pixels,
                &colors[p as usize],
                (t % columns) * TILE,
                (t / columns) * TILE,
                false,
                false,
                true,
            );
        }
        TileSheet { image, count, palettes }
    }

    /// The palette of a pair's secondary tileset that no metatile uses, if there's one
    fn free_palette(&self, pair: Pair) -> Option<usize> {
        let mut used = [false; 16];
        let mut seen = BTreeSet::new();
        for other in self.pairs().into_iter().filter(|p| p.secondary == pair.secondary).chain([pair]) {
            for (offset, primary) in [(other.primary, true), (other.secondary, false)] {
                let Some(t) = self.tileset(offset).filter(|_| seen.insert(offset)) else { continue };
                for local in 0..self.tileset_metatiles(&t, primary) {
                    for e in self.tileset_entries(&t, local) {
                        used[(e >> 12) as usize] = true;
                    }
                }
            }
        }
        (self.format.primary_palettes..self.format.total_palettes).find(|&p| !used[p])
    }

    /// Where a tileset's tiles would go: over the old ones when they fit there (or when
    /// what follows is unused), else at the ROM's unused end. Nothing is written.
    fn place_tiles(&self, t: &Tileset, data: &[u8]) -> Result<Placement, BorrowError> {
        const MARGIN: usize = 256;
        if !t.compressed {
            return Err(BorrowError::NotCompressed);
        }
        let packed = lz77_compress(data);
        let rom = self.rom.bytes();
        let old = self.rom.lz77_len(t.tiles).ok_or(BorrowError::Nothing)?.next_multiple_of(4);
        let grows_into = t.tiles + old..t.tiles + packed.len() + MARGIN;
        if packed.len() <= old || rom.get(grows_into).is_some_and(|after| after.iter().all(|&b| b == 0xFF)) {
            return Ok(Placement { at: t.tiles, packed, moved: false });
        }
        let at = (self.rom.free_tail() + MARGIN).next_multiple_of(4);
        if at + packed.len() + MARGIN > rom.len() {
            return Err(BorrowError::NoRoom);
        }
        Ok(Placement { at, packed, moved: true })
    }

    fn write_tiles(&mut self, t: &Tileset, placement: &Placement) {
        for (i, &b) in placement.packed.iter().enumerate() {
            self.rom.write8(placement.at + i, b);
        }
        if placement.moved {
            self.rom.write32(t.offset + 4, GBA_ROM_BASE + placement.at as u32);
        }
        self.forget_tiles();
    }

    /// Writes a tileset's tiles (4bpp, 32 bytes each) back into the ROM, to be committed
    pub fn store_tiles(&mut self, t: &Tileset, data: &[u8]) -> Result<(), BorrowError> {
        let placement = self.place_tiles(t, data)?;
        self.write_tiles(t, &placement);
        Ok(())
    }

    /// Makes a tile of another tileset usable in a pair: `tile` of `from`'s primary or
    /// secondary tileset, as it looks in `from`'s `palette`. See `import_tile`.
    pub fn borrow_tile(&mut self, pair: Pair, from: Pair, secondary: bool, tile: usize, palette: u8) -> Result<Borrowed, BorrowError> {
        if tile >= self.tileset_tile_count(from, secondary) {
            return Err(BorrowError::Nothing);
        }
        let data = self.tiles(if secondary { from.secondary } else { from.primary }, !secondary);
        let colors = *self.raw_palettes(from).get(palette as usize).ok_or(BorrowError::Nothing)?;
        self.import_tile(pair, &gfx::tile_pixels(&data, tile), &colors)
    }

    /// Makes a tile usable in a pair, to be committed. Its colours first: a palette of
    /// the pair that has them all, else a palette of the secondary tileset that no
    /// metatile uses (they're written there), else the closest palette. Then the tile
    /// itself: the one the pair's tilesets already have if they do, else a new one at
    /// the end of the secondary tileset.
    pub fn import_tile(&mut self, pair: Pair, pixels: &TilePixels, colors: &[u16; 16]) -> Result<Borrowed, BorrowError> {
        let f = self.format;
        let secondary = self.tileset(pair.secondary).ok_or(BorrowError::Nothing)?;
        let palettes = self.raw_palettes(pair);
        let mut weight = [0u32; 16];
        pixels.iter().for_each(|&p| weight[p as usize & 0xF] += 1);
        // colour 0 is see-through in any palette: it's the others that have to be found
        let used: Vec<usize> = (1..16).filter(|&i| weight[i] > 0).collect();
        let same = |a: u16, b: u16| a & 0x7FFF == b & 0x7FFF;
        let identity: [u8; 16] = std::array::from_fn(|i| i as u8);

        // the palette that has every colour, with the fewest of them elsewhere than in the tile's own
        let exact = |p: usize| -> Option<([u8; 16], usize)> {
            let (mut map, mut moved) = (identity, 0);
            for &i in &used {
                if !same(palettes[p][i], colors[i]) {
                    map[i] = (1..16).find(|&j| same(palettes[p][j], colors[i]))? as u8;
                    moved += 1;
                }
            }
            Some((map, moved))
        };
        let found =
            (0..f.total_palettes).filter_map(|p| exact(p).map(|(map, moved)| (moved, p, map))).min_by_key(|&(moved, p, _)| (moved, p));
        let (palette, map, kind) = match found {
            Some((_, p, map)) => (p, map, Colors::Found),
            None => match self.free_palette(pair) {
                Some(p) => (p, identity, Colors::Added),
                None => {
                    let closest = |p: usize, i: usize| (1..16).map(|j| (distance(palettes[p][j], colors[i]), j)).min().unwrap_or((0, i));
                    let cost = |p: usize| used.iter().map(|&i| weight[i] * closest(p, i).0).sum::<u32>();
                    let p = (0..f.total_palettes).min_by_key(|&p| (cost(p), p)).unwrap_or(0);
                    let mut map = identity;
                    used.iter().for_each(|&i| map[i] = closest(p, i).1 as u8);
                    (p, map, Colors::Approximated)
                }
            },
        };
        let packed = pack(&pixels.map(|p| map[p as usize & 0xF]));

        // the tile the pair has already, or a new one
        let (first, second) = (self.tiles(pair.primary, true), self.tiles(pair.secondary, false));
        let find = |data: &[u8], count: usize| (0..count).find(|&t| data[t * TILE_BYTES..(t + 1) * TILE_BYTES] == packed);
        let existing = find(&first, self.tileset_tile_count(pair, false))
            .or_else(|| find(&second, self.tileset_tile_count(pair, true)).map(|t| t + f.primary_tiles));
        let tile = match existing {
            Some(tile) => tile,
            None => {
                let count = second.len() / TILE_BYTES;
                let slot = self.tile_slot(pair, &second).ok_or(BorrowError::Full { tiles: count })?;
                let mut data = second[..count * TILE_BYTES].to_vec();
                data.resize(data.len().max((slot + 1) * TILE_BYTES), 0);
                data[slot * TILE_BYTES..(slot + 1) * TILE_BYTES].copy_from_slice(&packed);
                let placement = self.place_tiles(&secondary, &data)?;
                self.write_tiles(&secondary, &placement);
                f.primary_tiles + slot
            }
        };
        if kind == Colors::Added {
            for (c, &color) in colors.iter().enumerate() {
                self.rom.write16(secondary.palettes + (palette * 16 + c) * 2, color);
            }
        }
        Ok(Borrowed { tile: tile as u16, palette: palette as u8, colors: kind, existing: existing.is_some() })
    }
}
