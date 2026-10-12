//! Game Boy Color: Pokémon Crystal, and what is built like it, read into the editor's
//! own model.
//!
//! These games draw their maps with blocks of 4x4 tiles, 32 pixels a side, where the
//! Game Boy Advance games have metatiles of 2x2 tiles. The editor has one model, the
//! Advance games' as Emerald lays it out. A ROM of these games is read into an image
//! laid out that way: each block is four metatiles (its quarters), and a map is twice
//! as wide and as high as it has blocks. So a map of theirs lies tile for tile next to
//! the map an Advance game made of it. Everything the editor does, it does to that
//! image; `Cartridge::export` writes what changed back into the ROM itself.
//!
//! Nothing is at a known address: the map groups and the tilesets are found by their
//! shape, the palettes, the roofs and the places' names through the code that reads them.

use crate::format::block;
use crate::rom::{Rom, GBA_ROM_BASE};
use crate::scan::MapGroups;
use std::collections::HashMap;
use std::rc::Rc;

const BANK: usize = 0x4000;
/// Bytes of a map's entry in its group, and of a tileset's in the tilesets table
const MAP_ENTRY: usize = 9;
const TILESET_ENTRY: usize = 15;
/// A block: 4x4 tiles, a byte each
const BLOCK_BYTES: usize = 16;
/// How many tiles of a tileset's picture go to each of the console's two tile banks
const BANK_TILES: usize = 0x60;
/// Bytes of a tile as these games have it (2 bits a pixel), and as the image has it (4)
const TILE_2BPP: usize = 16;
const TILE_4BPP: usize = 32;
/// The image's tiles go by the ids the blocks give them (a byte). One more is blank:
/// the layer the Advance games' metatiles have and these games' blocks don't.
const BLANK_TILE: u16 = 0x100;
/// Tiles the image keeps room for in a tileset: what the editor reads of one
const IMAGE_TILES: usize = 512;
/// The roofs, a few tiles some tilesets take from the map's group: where they go
const ROOF_TILE: usize = 0x0A;
const ROOF_BYTES: usize = 9 * TILE_2BPP;
/// Where the roof's two colours go by the map's group: the palette, and the byte in it
const ROOF_COLORS: usize = 6 * 8 + 2;
/// The games' times of day, as their palettes are numbered
const DAY: usize = 1;
const NIGHT: usize = 2;
/// Maps outdoors: the ones whose roofs take their group's colours
const TOWN: u8 = 1;
const ROUTE: u8 = 2;

/// The metatile that is a block's quarter (`qx`, `qy`: 0 or 1). The picker shows 8
/// metatiles to a row: numbered this way, it shows the blocks whole, 4 to a row.
pub fn quarter(block: usize, qx: usize, qy: usize) -> u16 {
    (((block / 4) * 2 + qy) * 8 + (block % 4) * 2 + qx) as u16
}

/// The block a metatile is a quarter of, and which quarter (`qx`, `qy`)
pub fn block_of(metatile: u16) -> (usize, usize, usize) {
    let (row, column) = (metatile as usize / 8, metatile as usize % 8);
    ((row / 2) * 4 + column / 2, column % 2, row % 2)
}

/// The block four metatiles make, when they're one block's four quarters in their places
fn whole_block(cells: [u16; 4]) -> Option<u8> {
    let (block, qx, qy) = block_of(cells[0]);
    let whole = (qx, qy) == (0, 0) && (1..4).all(|q| cells[q] == quarter(block, q % 2, q / 2));
    u8::try_from(block).ok().filter(|_| whole)
}

/// Is this a Game Boy or Game Boy Color ROM? (the logo every cartridge of theirs begins with)
pub fn is_game_boy(bytes: &[u8]) -> bool {
    bytes.len() >= 2 * BANK && bytes[0x104..0x10C] == [0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B]
}

/// The cartridge's title, and its four-letter code when it has one
pub fn title(bytes: &[u8]) -> (String, String) {
    let text = |range: std::ops::Range<usize>| -> String {
        let bytes = bytes.get(range).unwrap_or_default();
        bytes.iter().take_while(|&&c| c != 0).map(|&c| if c.is_ascii_graphic() || c == b' ' { c as char } else { '?' }).collect()
    };
    let code = text(0x13F..0x143);
    if code.len() == 4 && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        (text(0x134..0x13F), code)
    } else {
        (text(0x134..0x143), String::new())
    }
}

/// A ROM's bytes, read by bank: nothing read outside it panics
#[derive(Clone, Copy)]
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u8(&self, o: usize) -> u8 {
        self.0.get(o).copied().unwrap_or(0)
    }

    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.u8(o), self.u8(o + 1)])
    }

    fn banks(&self) -> usize {
        self.0.len() / BANK
    }

    /// Where a bank's address is in the file: the banks that are switched in and out
    fn at(&self, bank: usize, address: usize) -> Option<usize> {
        ((BANK..2 * BANK).contains(&address) && (1..self.banks()).contains(&bank)).then(|| bank * BANK + address - BANK)
    }

    /// What the address at `o` points to in a bank
    fn far(&self, bank: usize, o: usize) -> Option<usize> {
        self.at(bank, self.u16(o) as usize)
    }

    /// What a bank's number and an address in it, at `o`, point to
    fn banked(&self, o: usize) -> Option<usize> {
        self.far(self.u8(o) as usize, o + 1)
    }

    /// Where code is, from `from` on: `pattern` is its bytes in hexadecimal, with `??`
    /// for those that change from one build of the game to another (the addresses)
    fn code(&self, pattern: &str, from: usize, to: usize) -> Option<usize> {
        let pattern: Vec<Option<u8>> = pattern.split_whitespace().map(|byte| u8::from_str_radix(byte, 16).ok()).collect();
        let first = pattern.first().copied().flatten()?;
        let last = to.min(self.0.len()).checked_sub(pattern.len())?;
        (from..=last).find(|&o| self.0[o] == first && pattern.iter().enumerate().all(|(i, byte)| byte.is_none_or(|b| self.0[o + i] == b)))
    }
}

// -- compression -------------------------------------------------------------------

/// The games' own compression: commands of a kind and a length, until 0xFF.
/// `None` when the data isn't that, or gives more than `limit` bytes.
pub fn unpack(rom: &[u8], mut o: usize, limit: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let command = *rom.get(o)?;
        if command == 0xFF {
            return Some(out);
        }
        // a long command holds its kind lower down, and ten bits of length
        let (kind, length) = if command >> 5 == 7 {
            o += 2;
            ((command >> 2) & 7, (((command as usize & 3) << 8) | *rom.get(o - 1)? as usize) + 1)
        } else {
            o += 1;
            (command >> 5, (command as usize & 0x1F) + 1)
        };
        if out.len() + length > limit {
            return None;
        }
        match kind {
            0 => {
                out.extend_from_slice(rom.get(o..o + length)?);
                o += length;
            }
            1 => {
                out.resize(out.len() + length, *rom.get(o)?);
                o += 1;
            }
            2 => {
                let pair = [*rom.get(o)?, *rom.get(o + 1)?];
                out.extend((0..length).map(|i| pair[i % 2]));
                o += 2;
            }
            3 => out.resize(out.len() + length, 0),
            // what was written already, again: as it is, its bits reversed, or backwards
            4..=6 => {
                let first = *rom.get(o)? as usize;
                let mut from = if first & 0x80 != 0 {
                    o += 1;
                    out.len().checked_sub((first & 0x7F) + 1)?
                } else {
                    o += 2;
                    (first << 8) | *rom.get(o - 1)? as usize
                };
                for _ in 0..length {
                    let byte = *out.get(from)?;
                    out.push(if kind == 5 { byte.reverse_bits() } else { byte });
                    from = if kind == 6 { from.wrapping_sub(1) } else { from + 1 };
                }
            }
            _ => return None,
        }
    }
}

// -- the maps ----------------------------------------------------------------------

/// A map as its group and its attributes tell it
#[derive(Clone, Copy)]
struct MapEntry {
    tileset: u8,
    environment: u8,
    landmark: u8,
    music: u8,
    /// The time of day its palettes are of, when it isn't the clock's
    time: u8,
    /// Where its border block is: the first byte of its attributes
    attributes: usize,
    width: usize,
    height: usize,
    blocks: usize,
}

fn map_entry(rom: &Bytes, o: usize) -> Option<MapEntry> {
    let (tileset, environment) = (rom.u8(o + 1), rom.u8(o + 2));
    let attributes = rom.far(rom.u8(o) as usize, o + 3)?;
    let (height, width) = (rom.u8(attributes + 1) as usize, rom.u8(attributes + 2) as usize);
    let blocks = rom.banked(attributes + 3)?;
    let fits = blocks % BANK + width * height <= BANK;
    // the attributes end with the sides the map is connected on: four bits
    let plain =
        environment <= 7 && tileset < 0x80 && (1..=127).contains(&width) && (1..=127).contains(&height) && rom.u8(attributes + 11) <= 0x0F;
    (fits && plain).then_some(MapEntry {
        tileset,
        environment,
        landmark: rom.u8(o + 5),
        music: rom.u8(o + 6),
        time: rom.u8(o + 7) & 0x0F,
        attributes,
        width,
        height,
        blocks,
    })
}

/// The map groups: a table of addresses, one per group, that the first group follows.
/// A group's maps run until the next group. The table that gives the most maps is it.
fn find_map_groups(rom: &Bytes) -> Option<Vec<Vec<MapEntry>>> {
    let mut best: Option<Vec<Vec<MapEntry>>> = None;
    for o in BANK..rom.0.len().saturating_sub(2) {
        let (bank, address) = (o / BANK, BANK + o % BANK);
        let first = rom.u16(o) as usize;
        let count = first.saturating_sub(address) / 2;
        if first <= address || !(first - address).is_multiple_of(2) || count > 64 {
            continue;
        }
        let starts: Vec<usize> = (0..count).map(|g| rom.u16(o + g * 2) as usize).collect();
        if starts.windows(2).any(|w| w[1] <= w[0] || !(w[1] - w[0]).is_multiple_of(MAP_ENTRY)) || starts.iter().any(|&s| s >= 2 * BANK) {
            continue;
        }
        let mut groups: Vec<Vec<MapEntry>> = Vec::new();
        for (g, &start) in starts.iter().enumerate() {
            let Some(start) = rom.at(bank, start) else { break };
            let most = starts.get(g + 1).map_or(255, |next| (next - starts[g]) / MAP_ENTRY);
            // the last group ends where the maps' attributes begin, when they follow it
            let attributes = groups.iter().flatten().map(|m| m.attributes).filter(|&a| a > start).min();
            let end = attributes.filter(|_| g + 1 == count).unwrap_or(usize::MAX);
            let maps: Vec<MapEntry> =
                (0..most).take_while(|m| start + (m + 1) * MAP_ENTRY <= end).map_while(|m| map_entry(rom, start + m * MAP_ENTRY)).collect();
            if maps.is_empty() || (g + 1 < count && maps.len() != most) {
                break;
            }
            groups.push(maps);
        }
        let maps = |groups: &Vec<Vec<MapEntry>>| groups.iter().map(Vec::len).sum::<usize>();
        if groups.len() == count && maps(&groups) > best.as_ref().map_or(0, maps) {
            best = Some(groups);
        }
    }
    best
}

// -- the tilesets --------------------------------------------------------------------

/// A tileset as the tilesets table tells it: where its picture, its blocks, their
/// collisions and its tiles' palettes are
#[derive(Clone, Copy)]
struct TilesetEntry {
    picture: usize,
    blocks: usize,
    collisions: usize,
    palettes: usize,
}

fn tileset_entry(rom: &Bytes, o: usize) -> Option<TilesetEntry> {
    // after the three banked addresses: the animation's address, a word left at zero,
    // and the address of the tiles' palettes, in the table's own bank
    if rom.u16(o + 11) != 0 || !(BANK..2 * BANK).contains(&(rom.u16(o + 9) as usize)) {
        return None;
    }
    Some(TilesetEntry {
        picture: rom.banked(o)?,
        blocks: rom.banked(o + 3)?,
        collisions: rom.banked(o + 6)?,
        palettes: rom.far(o / BANK, o + 13)?,
    })
}

/// The tilesets table: the longest run of entries that are tilesets
fn find_tilesets(rom: &Bytes) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    let mut o = BANK;
    while o + TILESET_ENTRY <= rom.0.len() {
        let count = (0..).take_while(|i| tileset_entry(rom, o + i * TILESET_ENTRY).is_some()).count();
        if count >= 8 && count > best.map_or(0, |(_, most)| most) {
            best = Some((o, count));
        }
        o += (count * TILESET_ENTRY).max(1);
    }
    best
}

// -- the colours -------------------------------------------------------------------

/// How the games colour a map, found through the code that loads a map's palettes:
/// by its environment and the time of day, 8 palettes out of a list
struct Colors {
    bank: usize,
    /// Per environment, the address of its palettes' numbers (8 per time of day)
    environments: usize,
    palettes: usize,
    /// Per map group, the roofs' two colours by day, then by night
    roofs: Option<usize>,
    special: Vec<Special>,
}

/// A tileset that comes with palettes of its own
struct Special {
    tileset: u8,
    /// The environment that keeps the usual palettes all the same
    except: Option<u8>,
    /// What's copied over the 8 palettes: where in them, from where in the ROM, how many bytes
    copies: Vec<(usize, usize, usize)>,
}

impl Colors {
    const ENVIRONMENTS: &'static str = "E6 07 5F 16 00 21 ?? ?? 19 19 2A 66 6F";
    const PALETTES: &'static str = "6F 26 00 29 29 29 11 ?? ?? 19 5D 54 E1 0E 08";
    const ROOFS: &'static str = "6F 26 00 29 29 29 11 ?? ?? 19 FA ?? ?? E6 03 FE 02 38 04 23 23 23 23";

    fn find(rom: &Bytes) -> Option<Self> {
        let mut from = BANK;
        while let Some(at) = rom.code(Self::PALETTES, from, rom.0.len()) {
            from = at + 1;
            let bank = at / BANK;
            // the same routine, a little before, reads the environment's list
            let Some(list) = rom.code(Self::ENVIRONMENTS, at.saturating_sub(0x40), at) else { continue };
            let (Some(environments), Some(palettes)) = (rom.far(bank, list + 6), rom.far(bank, at + 7)) else { continue };
            let roofs = rom.code(Self::ROOFS, at, at + 0x80).and_then(|at| rom.far(bank, at + 7));
            return Some(Self { bank, environments, palettes, roofs, special: Self::special(rom, list) });
        }
        None
    }

    /// The tilesets with palettes of their own: the routine the palettes' loader calls
    /// first compares the tileset with each, and jumps to what loads its palettes
    fn special(rom: &Bytes, list: usize) -> Vec<Special> {
        let mut special = Vec::new();
        // the call: "ld a, bank / ld hl, address / rst FarCall", then "jr c" and "ld a, [environment]"
        let Some(call) = list.checked_sub(11).filter(|&c| (rom.u8(c), rom.u8(c + 2), rom.u8(c + 5)) == (0x3E, 0x21, 0xCF)) else {
            return special;
        };
        let bank = rom.u8(call + 1) as usize;
        let Some(mut o) = rom.far(bank, call + 3).filter(|&o| rom.u8(o) == 0xFA) else { return special };
        o += 3;
        // "cp tileset / jr z, there", as many times as there are
        while (rom.u8(o), rom.u8(o + 2)) == (0xFE, 0x28) {
            let (tileset, mut there) = (rom.u8(o + 1), (o + 4).wrapping_add_signed(rom.u8(o + 3) as i8 as isize));
            o += 4;
            // one of them is only for some of its maps: "ld a, [environment] / and 7 / cp one / jr z, nothing"
            let mut except = None;
            if (rom.u8(there), rom.u8(there + 3), rom.u8(there + 5), rom.u8(there + 7)) == (0xFA, 0xE6, 0xFE, 0x28) {
                except = Some(rom.u8(there + 6));
                there += 9;
            }
            if rom.u8(there) != 0xCD {
                continue;
            }
            // the loader: "ld a, bank / ld de, to / ld hl, from / ld bc, length / call copy", once or more
            let Some(mut loader) = rom.far(bank, there + 1) else { continue };
            let mut copies = Vec::new();
            let mut base = None;
            while (rom.u8(loader), rom.u8(loader + 2), rom.u8(loader + 5), rom.u8(loader + 8), rom.u8(loader + 11))
                == (0x3E, 0x11, 0x21, 0x01, 0xCD)
            {
                let to = rom.u16(loader + 3) as usize;
                let to = to.wrapping_sub(*base.get_or_insert(to));
                let length = rom.u16(loader + 9) as usize;
                if let Some(from) = rom.far(bank, loader + 6).filter(|_| to + length <= 64) {
                    copies.push((to, from, length));
                }
                loader += 14;
            }
            if !copies.is_empty() {
                special.push(Special { tileset, except, copies });
            }
        }
        special
    }

    /// A map's 8 palettes of 4 colours, as the ROM has them
    fn of(&self, rom: &Bytes, tileset: u8, environment: u8, group: usize, time: usize) -> [u8; 64] {
        let mut colors = [0u8; 64];
        let environment = environment & 7;
        match self.special.iter().find(|s| s.tileset == tileset && s.except != Some(environment)) {
            Some(special) => {
                for &(to, from, length) in &special.copies {
                    for i in 0..length {
                        colors[to + i] = rom.u8(from + i);
                    }
                }
            }
            None => {
                let list = rom.far(self.bank, self.environments + environment as usize * 2);
                for p in 0..8 {
                    let palette = list.map_or(p, |list| rom.u8(list + time * 8 + p) as usize);
                    for i in 0..8 {
                        colors[p * 8 + i] = rom.u8(self.palettes + palette * 8 + i);
                    }
                }
            }
        }
        if let (Some(roofs), TOWN | ROUTE) = (self.roofs, environment) {
            let from = roofs + group * 8 + if time >= NIGHT { 4 } else { 0 };
            for i in 0..4 {
                colors[ROOF_COLORS + i] = rom.u8(from + i);
            }
        }
        colors
    }
}

/// The palettes of a ROM whose palettes weren't found: the first Game Boy's four greys
fn greys() -> [u8; 64] {
    let mut colors = [0u8; 64];
    for (i, grey) in [0x7FFFu16, 0x56B5, 0x294A, 0x0000].into_iter().cycle().take(32).enumerate() {
        colors[i * 2..i * 2 + 2].copy_from_slice(&grey.to_le_bytes());
    }
    colors
}

/// The roofs: tiles some tilesets take from the map's group, towns having roofs of their own
struct Roofs {
    /// Per map group, which roof (0xFF: none)
    groups: usize,
    tiles: usize,
    tilesets: Vec<u8>,
}

impl Roofs {
    const LOADER: &'static str = "5F 16 00 21 ?? ?? 19 7E FE FF C8 21 ?? ?? 01 90 00 CD ?? ?? 11 A0 90";

    fn find(rom: &Bytes) -> Option<Self> {
        let at = rom.code(Self::LOADER, BANK, rom.0.len())?;
        let bank = at / BANK;
        let (groups, tiles) = (rom.far(bank, at + 4)?, rom.far(bank, at + 12)?);
        // the tilesets that take one: those the tileset's loader looks for before it calls
        // this routine ("cp tileset / jr z, call", then a jump over the call)
        let routine = BANK + (at - 3) % BANK;
        let call = format!("3E {bank:02X} 21 {:02X} {:02X} CF", routine & 0xFF, routine >> 8);
        let mut tilesets = Vec::new();
        if let Some(mut o) = rom.code(&call, 0, rom.0.len()).and_then(|call| call.checked_sub(2)).filter(|&o| rom.u8(o) == 0x18) {
            while o >= 4 && (rom.u8(o - 4), rom.u8(o - 2)) == (0xFE, 0x28) {
                tilesets.push(rom.u8(o - 3));
                o -= 4;
            }
        }
        Some(Self { groups, tiles, tilesets })
    }

    /// Where the roof of a tileset's map in a group is, when it takes one
    fn of(&self, rom: &Bytes, tileset: u8, group: usize) -> Option<usize> {
        let roof = rom.u8(self.groups + group);
        (self.tilesets.contains(&tileset) && roof != 0xFF).then(|| self.tiles + roof as usize * ROOF_BYTES)
    }
}

// -- the places' names -------------------------------------------------------------

/// A place's name in the games' own letters. `None` when it isn't a name.
fn name(rom: &Bytes, o: usize) -> Option<String> {
    let mut name = String::new();
    for i in 0..24 {
        match rom.u8(o + i) {
            0x50 => return (!name.trim().is_empty()).then(|| name.split_whitespace().collect::<Vec<_>>().join(" ")),
            c @ 0x80..=0x99 => name.push((b'A' + c - 0x80) as char),
            c @ 0xA0..=0xB9 => name.push((b'a' + c - 0xA0) as char),
            c @ 0xF6..=0xFF => name.push((b'0' + c - 0xF6) as char),
            // a space, and where the Town Map may break the name
            0x7F | 0x1F | 0x22 => name.push(' '),
            0x1E | 0x25 => {}
            0x54 => name.push_str("POKé"),
            0x9A => name.push('('),
            0x9B => name.push(')'),
            0xE0 => name.push('\''),
            0xE3 => name.push('-'),
            0xE6 => name.push('?'),
            0xE7 => name.push('!'),
            0xE8 => name.push('.'),
            0xE9 => name.push('&'),
            0xEA => name.push('é'),
            0xF3 => name.push('/'),
            0xF4 => name.push(','),
            _ => return None,
        }
    }
    None
}

/// The places' names, by the number the maps give their place: found through the
/// routine that copies one
fn places(rom: &Bytes) -> Vec<String> {
    const NAME: &str = "E5 D5 C5 6B 26 00 29 29 11 ?? ?? 19 2A 66 6F";
    let Some(at) = rom.code(NAME, BANK, rom.0.len()) else { return Vec::new() };
    let bank = at / BANK;
    // four bytes a place: where it is on the Town Map, then its name's address
    let Some(names) = rom.far(bank, at + 9) else { return Vec::new() };
    (0..256).map_while(|i| name(rom, rom.far(bank, names + i * 4)?)).collect()
}

// -- the image ---------------------------------------------------------------------

/// The image being made: Emerald's structures, one after the other
struct Image {
    bytes: Vec<u8>,
}

impl Image {
    /// Room for `length` bytes, at a multiple of 4: where it begins
    fn room(&mut self, length: usize) -> usize {
        let at = self.bytes.len().next_multiple_of(4);
        self.bytes.resize(at + length, 0);
        at
    }

    fn put16(&mut self, at: usize, value: u16) {
        self.bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn point(&mut self, at: usize, to: usize) {
        self.put32(at, GBA_ROM_BASE + to as u32);
    }

    /// A tileset's structure
    fn tileset(&mut self, tiles: usize, palettes: usize, metatiles: usize, attributes: usize) -> usize {
        let at = self.room(crate::format::TILESET_SIZE);
        for (field, to) in [tiles, palettes, metatiles, attributes].into_iter().enumerate() {
            self.point(at + 4 + field * 4, to);
        }
        at
    }
}

/// A tileset of the ROM and what the image holds of it
struct TilesetLink {
    /// In the ROM: its blocks, their collisions, how many
    blocks: usize,
    collisions: usize,
    count: usize,
    /// In the image: its metatiles and their attributes
    metatiles: usize,
    attributes: usize,
    /// The palette of each tile id, and whether it's in the console's second tile bank
    tiles: Vec<u8>,
    /// The image's tilesets drawn from it: one per set of colours and roof
    structs: Vec<usize>,
}

/// A map's blocks in the ROM and their four metatiles each in the image
struct BlocksLink {
    blocks: usize,
    width: usize,
    height: usize,
    cells: usize,
}

/// What a ROM of these games was read into, and the ROM itself to write the edits back
pub struct Cartridge {
    bytes: Vec<u8>,
    tilesets: Vec<TilesetLink>,
    maps: Vec<BlocksLink>,
    /// Each map's border block in the ROM, and its four metatiles in the image
    borders: Vec<(usize, usize)>,
    places: Vec<String>,
}

/// A ROM read: the image, where its map groups are, and what links it to the ROM
pub struct Read {
    pub image: Vec<u8>,
    pub groups: MapGroups,
    pub cartridge: Cartridge,
}

struct Reading<'a> {
    rom: Bytes<'a>,
    image: Image,
    tilesets: (usize, usize),
    colors: Option<Colors>,
    roofs: Option<Roofs>,
    /// The highest block each tileset's maps use
    highest: HashMap<u8, usize>,
    /// The image's tileset no map draws from: the second of every pair
    empty: usize,
    links: Vec<TilesetLink>,
    /// Those, by where the blocks and the tiles' palettes are
    by_blocks: HashMap<(usize, usize), usize>,
    /// The image's tilesets, by tileset, colours and roof
    drawn: HashMap<(u8, [u8; 64], Option<usize>), usize>,
    pictures: HashMap<usize, Rc<Vec<u8>>>,
}

impl Reading<'_> {
    /// What the image holds of a tileset's blocks (once per tileset: the ROM's tilesets
    /// that share their blocks share them in the image too)
    fn link(&mut self, tileset: u8, entry: &TilesetEntry) -> usize {
        if let Some(&link) = self.by_blocks.get(&(entry.blocks, entry.palettes)) {
            return link;
        }
        let rom = self.rom;
        // how many blocks: they run until their collisions, which follow them. Where they
        // don't, as many as the maps use.
        let used = self.highest.get(&tileset).map_or(1, |&highest| highest + 1);
        let between = entry
            .collisions
            .checked_sub(entry.blocks)
            .filter(|d| entry.collisions / BANK == entry.blocks / BANK && d.is_multiple_of(BLOCK_BYTES));
        let count = between.map_or(used, |d| d / BLOCK_BYTES).min((BANK - entry.blocks % BANK) / BLOCK_BYTES).clamp(1, 256);
        let tiles: Vec<u8> = (0..256).map(|tile| (rom.u8(entry.palettes + tile / 2) >> (tile % 2 * 4)) & 0xF).collect();

        // four metatiles a block, the blocks 4 to a row: whole rows
        let metatiles_count = count.div_ceil(4) * 16;
        let metatiles = self.image.room(metatiles_count * crate::format::METATILE_BYTES);
        let attributes = self.image.room(metatiles_count * 2);
        for b in 0..metatiles_count / 4 {
            for q in 0..4 {
                let (qx, qy) = (q % 2, q / 2);
                let id = quarter(b, qx, qy) as usize;
                for k in 0..crate::format::ENTRIES {
                    // the bottom layer is the block's quarter; the top one is nothing
                    let tile = if k < 4 && b < count {
                        let tile = rom.u8(entry.blocks + b * BLOCK_BYTES + (qy * 2 + k / 2) * 4 + qx * 2 + k % 2);
                        tile as u16 | (((tiles[tile as usize] & 7) as u16) << 12)
                    } else {
                        BLANK_TILE
                    };
                    self.image.put16(metatiles + id * crate::format::METATILE_BYTES + k * 2, tile);
                }
                if b < count {
                    self.image.put16(attributes + id * 2, rom.u8(entry.collisions + b * 4 + q) as u16);
                }
            }
        }
        self.links.push(TilesetLink {
            blocks: entry.blocks,
            collisions: entry.collisions,
            count,
            metatiles,
            attributes,
            tiles,
            structs: Vec::new(),
        });
        self.by_blocks.insert((entry.blocks, entry.palettes), self.links.len() - 1);
        self.links.len() - 1
    }

    /// The image's tileset for a map: its tileset's blocks, with the picture and the
    /// colours the map gives them (its environment, its group's roof, its time of day)
    fn tileset(&mut self, map: &MapEntry, group: usize) -> usize {
        let rom = self.rom;
        let Some(entry) = Some(map.tileset as usize)
            .filter(|&t| t < self.tilesets.1)
            .and_then(|t| tileset_entry(&rom, self.tilesets.0 + t * TILESET_ENTRY))
        else {
            return self.empty;
        };
        // the clock's time is the day's; a dark cave is shown as it is once lit
        let time = match map.time {
            2 | 4 => NIGHT,
            3 => 0,
            _ => DAY,
        };
        let colors = self.colors.as_ref().map_or_else(greys, |colors| colors.of(&rom, map.tileset, map.environment, group, time));
        let roof = self.roofs.as_ref().and_then(|roofs| roofs.of(&rom, map.tileset, group));
        if let Some(&drawn) = self.drawn.get(&(map.tileset, colors, roof)) {
            return drawn;
        }
        let link = self.link(map.tileset, &entry);
        let picture =
            self.pictures.entry(entry.picture).or_insert_with(|| Rc::new(unpack(rom.0, entry.picture, 0x4000).unwrap_or_default())).clone();
        let mut picture = (*picture).clone();
        if let Some(roof) = roof {
            let at = ROOF_TILE * TILE_2BPP;
            picture.resize(picture.len().max(at + ROOF_BYTES), 0);
            for i in 0..ROOF_BYTES {
                picture[at + i] = rom.u8(roof + i);
            }
        }

        // the tiles, by the ids the blocks give them: the second half of the picture is
        // in the console's second tile bank, where the tiles' palettes say which tiles are
        let tiles = self.image.room(IMAGE_TILES * TILE_4BPP);
        for id in 0..256 {
            let (local, second) = (id & 0x7F, self.links[link].tiles[id] & 8 != 0);
            let from = (local + if second { BANK_TILES } else { 0 }) * TILE_2BPP;
            let Some(tile) = picture.get(from..from + TILE_2BPP).filter(|_| local < BANK_TILES) else { continue };
            for row in 0..8 {
                let (low, high) = (tile[row * 2], tile[row * 2 + 1]);
                // colours 1 to 4: the editor's colour 0 is see-through, and these games have no such thing
                let pixel = |x: usize| (((high >> (7 - x)) & 1) << 1) | ((low >> (7 - x)) & 1);
                for pair in 0..4 {
                    self.image.bytes[tiles + id * TILE_4BPP + row * 4 + pair] = (pixel(pair * 2) + 1) | ((pixel(pair * 2 + 1) + 1) << 4);
                }
            }
        }
        let palettes = self.image.room(16 * 32);
        for (i, &color) in colors.as_chunks::<2>().0.iter().enumerate() {
            self.image.put16(palettes + ((i / 4) * 16 + i % 4 + 1) * 2, u16::from_le_bytes(color));
        }
        let (metatiles, attributes) = (self.links[link].metatiles, self.links[link].attributes);
        let drawn = self.image.tileset(tiles, palettes, metatiles, attributes);
        self.links[link].structs.push(drawn);
        self.drawn.insert((map.tileset, colors, roof), drawn);
        drawn
    }
}

/// Reads a ROM of these games into an image the editor reads. `None` when no maps or
/// no tilesets are found in it.
pub fn read(bytes: &[u8]) -> Option<Read> {
    let rom = Bytes(bytes);
    let groups = find_map_groups(&rom)?;
    let tilesets = find_tilesets(&rom)?;
    let mut highest: HashMap<u8, usize> = HashMap::new();
    for map in groups.iter().flatten() {
        let most = (0..map.width * map.height).map(|i| rom.u8(map.blocks + i)).max().unwrap_or(0).max(rom.u8(map.attributes));
        let highest = highest.entry(map.tileset).or_default();
        *highest = (*highest).max(most as usize);
    }

    // what tells the image's ROM apart: the cartridge's title and code where an Advance ROM has its own
    let mut image = Image { bytes: vec![0; 0xC0] };
    let (title, code) = title(bytes);
    image.bytes[0xA0..0xA0 + title.len().min(12)].copy_from_slice(&title.as_bytes()[..title.len().min(12)]);
    image.bytes[0xAC..0xAC + code.len()].copy_from_slice(code.as_bytes());
    // the tileset that is every map's second one: these games' maps have one tileset
    let (tiles, palettes, metatiles) =
        (image.room(IMAGE_TILES * TILE_4BPP), image.room(16 * 32), image.room(crate::format::METATILE_BYTES));
    let empty = image.tileset(tiles, palettes, metatiles, metatiles);
    image.bytes[empty + 1] = 1;

    let mut reading = Reading {
        rom,
        image,
        tilesets,
        colors: Colors::find(&rom),
        roofs: Roofs::find(&rom),
        highest,
        empty,
        links: Vec::new(),
        by_blocks: HashMap::new(),
        drawn: HashMap::new(),
        pictures: HashMap::new(),
    };
    let mut maps: Vec<BlocksLink> = Vec::new();
    let mut cells_of: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut borders = Vec::new();
    let mut headers: Vec<Vec<usize>> = Vec::new();
    for (g, group) in groups.iter().enumerate() {
        let mut group_headers = Vec::new();
        for map in group {
            // the games count their groups from 1
            let tileset = reading.tileset(map, g + 1);
            let image = &mut reading.image;
            let (width, height) = (map.width * 2, map.height * 2);
            // maps that share their blocks in the ROM share them in the image
            let cells = *cells_of.entry((map.blocks, map.width, map.height)).or_insert_with(|| {
                let cells = image.room(width * height * 2);
                for i in 0..width * height {
                    let (x, y) = (i % width, i / width);
                    let block = rom.u8(map.blocks + (y / 2) * map.width + x / 2);
                    image.put16(cells + i * 2, quarter(block as usize, x % 2, y % 2));
                }
                maps.push(BlocksLink { blocks: map.blocks, width: map.width, height: map.height, cells });
                cells
            });
            let border = image.room(4 * 2);
            for q in 0..4 {
                image.put16(border + q * 2, quarter(rom.u8(map.attributes) as usize, q % 2, q / 2));
            }
            borders.push((map.attributes, border));
            let layout = image.room(24);
            image.put32(layout, width as u32);
            image.put32(layout + 4, height as u32);
            for (field, to) in [border, cells, tileset, empty].into_iter().enumerate() {
                image.point(layout + 8 + field * 4, to);
            }
            let header = image.room(crate::format::HEADER_SIZE);
            image.point(header, layout);
            image.put16(header + 16, map.music as u16);
            image.put16(header + 18, borders.len() as u16);
            image.bytes[header + 20] = map.landmark;
            image.bytes[header + 23] = map.environment;
            group_headers.push(header);
        }
        headers.push(group_headers);
    }
    let image = &mut reading.image;
    let arrays: Vec<usize> = headers
        .iter()
        .map(|group| {
            let array = image.room(group.len() * 4);
            for (i, &header) in group.iter().enumerate() {
                image.point(array + i * 4, header);
            }
            array
        })
        .collect();
    let table = image.room(arrays.len() * 4 + 4);
    for (g, &array) in arrays.iter().enumerate() {
        image.point(table + g * 4, array);
    }
    let groups = MapGroups { table, groups: arrays.into_iter().zip(headers).collect() };
    let cartridge = Cartridge { bytes: bytes.to_vec(), tilesets: reading.links, maps, borders, places: places(&rom) };
    Some(Read { image: reading.image.bytes, groups, cartridge })
}

impl Cartridge {
    /// The ROM as it was opened
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A place's name, by the number the maps give their place
    pub fn place(&self, number: u16) -> Option<&str> {
        self.places.get(number as usize).map(String::as_str)
    }

    /// The palette these games draw a tile in: it goes with the tile, whatever block it's in
    pub fn tile_palette(&self, tileset: usize, tile: usize) -> Option<u8> {
        let link = self.tilesets.iter().find(|link| link.structs.contains(&tileset))?;
        Some(link.tiles.get(tile)? & 7)
    }

    /// The ROM with what changed in the image since it was read (`opened`): the maps'
    /// blocks where four metatiles make one, the blocks' tiles and their collisions.
    /// What didn't change is left as it is, byte for byte.
    pub fn export(&self, now: &Rom, opened: &Rom) -> Vec<u8> {
        let mut out = self.bytes.clone();
        let changed = |at: usize| now.u16(at) != opened.u16(at);
        let cell = |at: usize| block::metatile(now.u16(at));
        for map in &self.maps {
            for b in 0..map.width * map.height {
                let (x, y) = (b % map.width * 2, b / map.width * 2);
                let at: [usize; 4] = std::array::from_fn(|q| map.cells + ((y + q / 2) * map.width * 2 + x + q % 2) * 2);
                if at.iter().any(|&at| changed(at)) {
                    if let Some(block) = whole_block(at.map(cell)) {
                        out[map.blocks + b] = block;
                    }
                }
            }
        }
        for &(border, cells) in &self.borders {
            let at: [usize; 4] = std::array::from_fn(|q| cells + q * 2);
            if at.iter().any(|&at| changed(at)) {
                if let Some(block) = whole_block(at.map(cell)) {
                    out[border] = block;
                }
            }
        }
        for tileset in &self.tilesets {
            for b in 0..tileset.count {
                for q in 0..4 {
                    let (qx, qy) = (q % 2, q / 2);
                    let id = quarter(b, qx, qy) as usize;
                    for k in 0..4 {
                        let at = tileset.metatiles + id * crate::format::METATILE_BYTES + k * 2;
                        // a tile of these games' is a byte: the blank one of the image is none of theirs
                        if let Some(tile) = u8::try_from(now.u16(at) & 0x3FF).ok().filter(|_| changed(at)) {
                            out[tileset.blocks + b * BLOCK_BYTES + (qy * 2 + k / 2) * 4 + qx * 2 + k % 2] = tile;
                        }
                    }
                    let at = tileset.attributes + id * 2;
                    if changed(at) {
                        out[tileset.collisions + b * 4 + q] = now.u16(at) as u8;
                    }
                }
            }
        }
        // the cartridge's checksum: every byte but its own two
        if out != self.bytes && out.len() > 0x150 {
            let sum =
                out.iter().enumerate().filter(|(i, _)| !(0x14E..0x150).contains(i)).fold(0u16, |sum, (_, &b)| sum.wrapping_add(b as u16));
            out[0x14E..0x150].copy_from_slice(&sum.to_be_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarters() {
        assert_eq!([quarter(0, 0, 0), quarter(0, 1, 0), quarter(0, 0, 1), quarter(0, 1, 1)], [0, 1, 8, 9]);
        assert_eq!([quarter(3, 1, 1), quarter(4, 0, 0), quarter(127, 1, 1)], [15, 16, 511]);
        for block in 0..256 {
            for q in 0..4 {
                assert_eq!(block_of(quarter(block, q % 2, q / 2)), (block, q % 2, q / 2));
            }
            let cells: [u16; 4] = std::array::from_fn(|q| quarter(block, q % 2, q / 2));
            assert_eq!(whole_block(cells), Some(block as u8));
            assert_eq!(whole_block([cells[1], cells[0], cells[2], cells[3]]), None);
        }
        assert_eq!(whole_block([0, 1, 8, 25]), None, "quarters of two blocks");
    }

    #[test]
    fn unpacked() {
        // 3 bytes as they are, one 4 times, two in turn 5 times, 2 zeros, the end
        let packed = [0x02, 1, 2, 3, 0x23, 9, 0x44, 7, 8, 0x61, 0xFF];
        assert_eq!(unpack(&packed, 0, 64), Some(vec![1, 2, 3, 9, 9, 9, 9, 7, 8, 7, 8, 7, 0, 0]));
        // what was written: 2 bytes from the start, 2 from 3 back with their bits reversed, 2 backwards from the second
        let again = [0x02, 0x01, 0x02, 0x80, 0x81, 0x00, 0x00, 0xA1, 0x82, 0xC1, 0x00, 0x01, 0xFF];
        assert_eq!(unpack(&again, 0, 64), Some(vec![1, 2, 0x80, 1, 2, 0x01, 0x80, 2, 1]));
        // a long command: 300 zeros
        assert_eq!(unpack(&[0xED, 0x2B, 0xFF], 0, 512).map(|out| (out.len(), out[299])), Some((300, 0)));
        assert_eq!(unpack(&[0xED, 0x2B, 0xFF], 0, 64), None, "more than is wanted");
        assert_eq!(unpack(&[0x02, 1, 2], 0, 64), None, "no end");
        assert_eq!(unpack(&[0x80, 0x00, 0x05, 0xFF], 0, 64), None, "nothing written yet to write again");
    }

    #[test]
    fn names() {
        // "NEW BARK" (a break) "TOWN", "MT.MOON", "ROUTE 29"
        let bytes = [
            0x8D, 0x84, 0x96, 0x7F, 0x81, 0x80, 0x91, 0x8A, 0x1F, 0x93, 0x8E, 0x96, 0x8D, 0x50, 0x8C, 0x93, 0xE8, 0x8C, 0x8E, 0x8E, 0x8D,
            0x50,
        ];
        let rom = Bytes(&bytes);
        assert_eq!(name(&rom, 0).as_deref(), Some("NEW BARK TOWN"));
        assert_eq!(name(&rom, 14).as_deref(), Some("MT.MOON"));
        assert_eq!(name(&Bytes(&[0x91, 0x8E, 0x94, 0x93, 0x84, 0x7F, 0xF8, 0xFF, 0x50]), 0).as_deref(), Some("ROUTE 29"));
        assert_eq!(name(&Bytes(&[0x91, 0x4E, 0x50]), 0), None, "not a name's letter");
        assert_eq!(name(&Bytes(&[0x50]), 0), None);
        assert_eq!(name(&Bytes(&[0x91; 40]), 0), None, "no end");
    }

    #[test]
    fn not_these_games() {
        assert!(!is_game_boy(&[0; 0x8000]));
        assert!(read(&[0; 0x8000]).is_none());
        assert!(read(&[]).is_none());
    }
}
