//! More metatiles for a tileset.
//!
//! A tileset doesn't say how many metatiles it has: they run until their attributes,
//! which follow them, and something else follows those. So a tileset is given more by
//! moving both to where there's room, the ROM's unused end, and pointing the tileset
//! there; once there, they grow where they are. The game itself never counts them: a
//! map's block picks a metatile by its number, whatever it is.

use crate::format::{block, ENTRIES, METATILE_BYTES};
use crate::game::Game;
use crate::rom::GBA_ROM_BASE;
use crate::structs::{Pair, Tileset};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Why a tileset wasn't given more metatiles
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowError {
    /// No such tileset
    Nothing,
    /// A game whose tilesets are made of blocks that aren't the editor's to add to
    Blocks,
    /// The tileset has as many metatiles as the game gives one
    Full { metatiles: usize },
    /// The ROM has no unused space left at its end
    NoRoom,
}

impl fmt::Display for GrowError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Nothing => write!(f, "There's no such tileset."),
            Self::Blocks => write!(f, "This game's tilesets keep the blocks they have."),
            Self::Full { metatiles } => {
                write!(f, "The tileset has {metatiles} metatiles, as many as the game gives one: rebuild one that's on no map instead (Next free).")
            }
            Self::NoRoom => write!(f, "The ROM has no unused space left at its end for more metatiles."),
        }
    }
}

/// Why two metatiles didn't change places
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    /// No such metatile
    Nothing,
    /// They aren't of the same tileset
    Apart,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Nothing => write!(f, "There's no metatile there to change places with."),
            Self::Apart => {
                write!(f, "A metatile moves within its own tileset: the primary one's are every map's, the secondary one's its own maps'.")
            }
        }
    }
}

impl Game {
    /// Exchanges two metatiles of one of a pair's tilesets: what they're made of, their
    /// attributes. Every map drawn with that tileset is renumbered to match (the maps'
    /// originals too), so that no map looks or plays differently. Returns how many of
    /// the maps' tiles were renumbered; to be committed.
    ///
    /// What names a metatile by its number elsewhere isn't known, and isn't changed: a
    /// script that places one, the game's code for the doors it opens.
    pub fn swap_metatiles(&mut self, pair: Pair, a: u16, b: u16) -> Result<usize, MoveError> {
        let ((t, at), (other, to)) = (self.locate(pair, a).ok_or(MoveError::Nothing)?, self.locate(pair, b).ok_or(MoveError::Nothing)?);
        if t.offset != other.offset {
            return Err(MoveError::Apart);
        }
        if at == to {
            return Ok(0);
        }
        let (entries, attributes) = (
            [self.tileset_entries(&t, at), self.tileset_entries(&t, to)],
            [self.tileset_attribute(&t, at), self.tileset_attribute(&t, to)],
        );
        self.set_tileset_entries(&t, at, &entries[1]);
        self.set_tileset_entries(&t, to, &entries[0]);
        self.set_tileset_attribute(&t, at, attributes[1]);
        self.set_tileset_attribute(&t, to, attributes[0]);

        // the tilesets made of those same metatiles (a Game Boy Color tileset is one per
        // set of colours), and the layouts drawn with one: as a primary tileset its
        // metatiles are numbered from 0, as a secondary one after the primary's
        let known = self.tilesets().into_iter().chain(self.original_tilesets());
        let same: BTreeSet<usize> = known.filter(|(known, _)| known.metatiles == t.metatiles).map(|(known, _)| known.offset).collect();
        let first = self.format.primary_metatiles;
        let mut places: BTreeMap<usize, (usize, Vec<(u16, u16)>)> = BTreeMap::new();
        for l in self.maps.iter().flat_map(|m| [Some(m.layout), m.original]).flatten() {
            let mut numbers = Vec::new();
            if same.contains(&l.primary) && at.max(to) < first {
                numbers.push((at as u16, to as u16));
            }
            if same.contains(&l.secondary) && self.format.secondary_metatiles() > 0 {
                numbers.push(((first + at) as u16, (first + to) as u16));
            }
            if !numbers.is_empty() {
                // blocks several layouts are drawn from are renumbered once
                places.entry(l.blocks).or_insert((l.width * l.height, numbers.clone()));
                places.entry(l.border).or_insert((l.border_width * l.border_height, numbers));
            }
        }
        let mut renumbered = 0;
        for (place, (count, numbers)) in places {
            for i in 0..count {
                let tile = self.rom.u16(place + i * 2);
                let id = block::metatile(tile);
                if let Some(&(x, y)) = numbers.iter().find(|&&(x, y)| id == x || id == y) {
                    self.rom.write16(place + i * 2, block::with_metatile(tile, if id == x { y } else { x }));
                    renumbered += 1;
                }
            }
        }
        Ok(renumbered)
    }

    /// Gives a tileset more metatiles: `count` in all, the new ones empty (no tile, no
    /// behavior). `primary`: whether it's a primary tileset, which tells how many it may
    /// hold. Returns the tileset as it is then; to be committed.
    pub fn grow_metatiles(&mut self, t: &Tileset, primary: bool, count: usize) -> Result<Tileset, GrowError> {
        /// What's left unused between this and what comes next at the ROM's end, and what
        /// has to be unused after the metatiles for them to grow where they are
        const MARGIN: usize = 256;
        const SPARE: usize = 16;
        if self.format.block_size() > 1 {
            return Err(GrowError::Blocks);
        }
        let most = if primary { self.format.primary_metatiles } else { self.format.secondary_metatiles() };
        let had = self.tileset_metatiles(t, primary);
        if count <= had {
            return Ok(*t);
        }
        if count > most {
            return Err(GrowError::Full { metatiles: most });
        }
        let size = self.format.attribute_size;
        let rom = self.rom.bytes();
        let metatiles_then = rom.get(t.metatiles..t.metatiles + had * METATILE_BYTES).ok_or(GrowError::Nothing)?.to_vec();
        let attributes_then = rom.get(t.attributes..t.attributes + had * size).ok_or(GrowError::Nothing)?.to_vec();
        // where they are, when the attributes follow the metatiles and the ROM's filler
        // follows them (they were moved to its end before); else to its end
        let (end, more) = (t.attributes + had * size, (count - had) * (METATILE_BYTES + size));
        let follows = t.attributes == t.metatiles + had * METATILE_BYTES;
        let unused = rom.get(end..end + more + SPARE).is_some_and(|after| after.iter().all(|&b| b == 0xFF));
        let metatiles = if follows && unused {
            t.metatiles
        } else {
            let at = (self.rom.free_tail() + MARGIN).next_multiple_of(4);
            if at + count * (METATILE_BYTES + size) + SPARE > rom.len() {
                return Err(GrowError::NoRoom);
            }
            at
        };
        let attributes = metatiles + count * METATILE_BYTES;
        let grown = metatiles_then.iter().copied().chain(std::iter::repeat_n(0, (count - had) * METATILE_BYTES));
        for (i, b) in grown.chain(attributes_then.iter().copied()).chain(std::iter::repeat_n(0, (count - had) * size)).enumerate() {
            self.rom.write8(metatiles + i, b);
        }
        self.rom.write32(t.offset + 12, GBA_ROM_BASE + metatiles as u32);
        self.rom.write32(t.offset + self.format.tileset_attributes, GBA_ROM_BASE + attributes as u32);
        self.tileset(t.offset).ok_or(GrowError::Nothing)
    }

    /// A new metatile at the end of a pair's secondary tileset: one no map shows and no
    /// script places, since it wasn't there. Empty, or the copy of another one of the
    /// pair (`like`): its tiles and its attributes. Returns its id; to be committed.
    pub fn add_metatile(&mut self, pair: Pair, like: Option<u16>) -> Result<u16, GrowError> {
        let t = self.tileset(pair.secondary).ok_or(GrowError::Nothing)?;
        let had = self.tileset_metatiles(&t, false);
        let copied: Option<([u16; ENTRIES], u32)> = like.map(|id| (self.entries(pair, id), self.attribute(pair, id)));
        let t = self.grow_metatiles(&t, false, had + 1)?;
        if let Some((entries, attribute)) = copied {
            self.set_tileset_entries(&t, had, &entries);
            self.set_tileset_attribute(&t, had, attribute);
        }
        Ok((self.format.primary_metatiles + had) as u16)
    }
}
