//! A ROM opened as a game: its maps, their tilesets, and the edits made to them.
//!
//! Edits are written straight into the ROM image; every `commit` is one undoable step.
//! The image the ROM was opened with is kept, so what changed can always be told.

use crate::format::{block, Family, Format, ENTRIES, METATILE_BYTES};
use crate::gbc::{self, Cartridge};
use crate::gfx::{self, Palette, TilePixels, METATILE, TILE};
use crate::map_names;
use crate::profile::Profile;
use crate::rom::{Patch, Rom};
use crate::scan::{self, MapGroups, SectionNames};
use crate::structs::{Header, Layout, Pair, Reader, Tileset};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;
use std::str::FromStr;

/// A map's place in the map groups table: "group.number"
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MapId {
    pub group: u8,
    pub num: u8,
}

impl fmt::Display for MapId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.group, self.num)
    }
}

impl FromStr for MapId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("not a map id (group.number): {s}");
        let (group, num) = s.split_once('.').ok_or_else(bad)?;
        Ok(Self { group: group.trim().parse().map_err(|_| bad())?, num: num.trim().parse().map_err(|_| bad())? })
    }
}

#[derive(Clone, Debug)]
pub struct MapInfo {
    pub id: MapId,
    /// The profile's name, else the original game's, else the place's name, else the id
    pub name: String,
    /// The place's name on the region map
    pub section: Option<String>,
    pub group_name: String,
    pub header: Header,
    pub layout: Layout,
    /// The map as it was before the hack redrew it, on the tilesets it came with: a
    /// layout no map uses, that the profile points to
    pub original: Option<Layout>,
}

/// A map being edited: a copy of its blocks and border, stored back with `Game::store_map`
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    pub index: usize,
    pub id: MapId,
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub blocks: Vec<u16>,
    pub border: Vec<u16>,
    pub border_width: usize,
    pub border_height: usize,
    pub layout: Layout,
    pub pair: Pair,
}

impl Map {
    pub fn block(&self, x: usize, y: usize) -> Option<u16> {
        (x < self.width && y < self.height).then(|| self.blocks[y * self.width + x])
    }
}

/// RGBA pixels
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, rgba: vec![0; width * height * 4] }
    }

    pub fn filled(width: usize, height: usize, color: [u8; 4]) -> Self {
        Self { width, height, rgba: color.repeat(width * height) }
    }

    /// A part of the picture (what's outside it is left transparent)
    pub fn crop(&self, x: usize, y: usize, width: usize, height: usize) -> Self {
        let mut out = Self::new(width, height);
        for row in 0..height.min(self.height.saturating_sub(y)) {
            let n = width.min(self.width.saturating_sub(x)) * 4;
            let from = ((y + row) * self.width + x) * 4;
            out.rgba[row * width * 4..row * width * 4 + n].copy_from_slice(&self.rgba[from..from + n]);
        }
        out
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let o = (y * self.width + x) * 4;
        [self.rgba[o], self.rgba[o + 1], self.rgba[o + 2], self.rgba[o + 3]]
    }
}

/// A tileset pair's graphics, decoded: what drawing its metatiles needs
pub struct PairGfx {
    format: &'static Format,
    primary: Rc<Vec<u8>>,
    secondary: Rc<Vec<u8>>,
    pub palettes: Vec<Palette>,
}

impl PairGfx {
    pub fn tile(&self, tile: u16) -> TilePixels {
        let tile = tile as usize;
        if tile < self.format.primary_tiles {
            gfx::tile_pixels(&self.primary, tile)
        } else {
            gfx::tile_pixels(&self.secondary, tile - self.format.primary_tiles)
        }
    }

    /// How many tile ids hold graphics
    pub fn tile_count(&self) -> usize {
        self.format.primary_tiles + (self.secondary.len() / 32).min(self.format.secondary_tiles())
    }

    pub fn draw_metatile(&self, image: &mut Image, x: usize, y: usize, entries: &[u16; ENTRIES]) {
        gfx::draw_metatile(&mut image.rgba, image.width, x, y, entries, |t| self.tile(t), &self.palettes);
    }
}

/// How a metatile is used on the maps: how often with each collision and elevation
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockUsage {
    /// Indexed by a block's high 6 bits (`collision | elevation << 2`)
    pub counts: [u32; 64],
}

impl Default for BlockUsage {
    fn default() -> Self {
        Self { counts: [0; 64] }
    }
}

impl BlockUsage {
    /// The collision and elevation to give this metatile when it's painted over `current`:
    /// the usual ones, but the tile's elevation is kept when the metatile is no stranger
    /// to it (grass painted on a plateau stays on the plateau; water painted on land
    /// goes down to the water's level). Returns a block's high bits.
    pub fn choose(&self, current: u16) -> u16 {
        let total: u32 = self.counts.iter().sum();
        if total == 0 {
            return current & !block::METATILE_MASK;
        }
        let best_of =
            |candidates: &mut dyn Iterator<Item = usize>| candidates.max_by_key(|&i| (self.counts[i], std::cmp::Reverse(i))).unwrap_or(0);
        let elevation = block::elevation(current) as usize;
        let at_elevation: u32 = (0..4).map(|c| self.counts[c | elevation << 2]).sum();
        let bits = if at_elevation * 10 >= total { best_of(&mut (0..4).map(|c| c | elevation << 2)) } else { best_of(&mut (0..64)) };
        (bits as u16) << 10
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    Empty,
    NoMaps {
        header: String,
        gba: bool,
    },
    /// A Game Boy or Game Boy Color ROM, but not one of the games read
    NoGameBoyMaps {
        header: String,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "The file is empty."),
            Self::NoMaps { header, gba: true } => {
                let which = if header.is_empty() { String::new() } else { format!(" ({header})") };
                write!(f, "No maps found in this ROM{which}. Pokémon Ruby, Sapphire, Emerald, FireRed, LeafGreen and their hacks are the games GBMapping reads.")
            }
            Self::NoMaps { gba: false, .. } => write!(f, "This isn't a Game Boy Advance or a Game Boy Color ROM."),
            Self::NoGameBoyMaps { header } => {
                let which = if header.is_empty() { String::new() } else { format!(" ({header})") };
                write!(f, "No maps found in this Game Boy ROM{which}. Pokémon Crystal, and the hacks that keep its maps and tilesets the way it has them, are the Game Boy Color games GBMapping reads.")
            }
        }
    }
}

const UNDO_DEPTH: usize = 500;

/// A tileset's metatile `local` in a ROM image: its 8 tile entries
pub(crate) fn entries_in(rom: &Rom, t: &Tileset, local: usize) -> [u16; ENTRIES] {
    std::array::from_fn(|k| rom.u16(t.metatiles + local * METATILE_BYTES + k * 2))
}

/// A tileset's metatile `local` in a ROM image: its attributes
pub(crate) fn attribute_in(rom: &Rom, format: &Format, t: &Tileset, local: usize) -> u32 {
    let o = t.attributes + local * format.attribute_size;
    if format.attribute_size == 4 {
        rom.u32(o)
    } else {
        rom.u16(o) as u32
    }
}

pub struct Game {
    pub rom: Rom,
    original: Rom,
    pub family: Family,
    pub format: &'static Format,
    pub profile: Profile,
    /// The map headers' region map section is 16 bits wide (the profile says so, or the headers do)
    pub mapsec16: bool,
    pub groups: MapGroups,
    pub sections: Option<SectionNames>,
    pub maps: Vec<MapInfo>,
    tiles: RefCell<HashMap<usize, Rc<Vec<u8>>>>,
    undo: Vec<Patch>,
    redo: Vec<Patch>,
    /// The Game Boy Color ROM this was read from, when it's one: `rom` is then what the
    /// editor made of it (see `gbc`), and saving writes the edits back into it
    gbc: Option<Box<Cartridge>>,
}

impl Game {
    /// Opens a ROM: its family by the profile, the game code, or whichever finds maps
    pub fn open(bytes: Vec<u8>, name: &str, profile: Profile) -> Result<Self, OpenError> {
        if bytes.is_empty() {
            return Err(OpenError::Empty);
        }
        if gbc::is_game_boy(&bytes) {
            return Self::open_game_boy(&bytes, name, profile);
        }
        let rom = Rom::new(bytes, name);
        let header = rom.header();
        let wanted = profile.game.as_deref().and_then(Family::from_profile).or_else(|| Family::from_code(&header.code));
        let mut families: Vec<Family> = wanted.into_iter().collect();
        for f in [Family::Emerald, Family::FireRedLeafGreen] {
            // Ruby/Sapphire's structures are Emerald's
            if !families.iter().any(|w| w.format().layout_border_size == f.format().layout_border_size) {
                families.push(f);
            }
        }
        let mut found: Option<(Family, MapGroups)> = None;
        for family in families {
            let reader = Reader { rom: &rom, format: family.format(), mapsec16: profile.mapsec16 };
            let groups = profile
                .map_groups
                .and_then(|table| scan::read_map_groups(reader, table, &profile.group_sizes()))
                .or_else(|| scan::find_map_groups(reader));
            if let Some(groups) = groups {
                if groups.map_count() > found.as_ref().map_or(0, |(_, g)| g.map_count()) {
                    found = Some((family, groups));
                }
            }
            if found.is_some() && Some(family) == wanted {
                break;
            }
        }
        let Some((family, groups)) = found else {
            return Err(OpenError::NoMaps { header: format!("{} {}", header.title, header.code).trim().to_string(), gba: rom.is_gba() });
        };
        let sections = scan::find_section_names(&rom, family, profile.region_map_entries);
        let mapsec16 = profile.mapsec16 || scan::wide_sections(&rom, &groups);
        let mut game = Self {
            original: rom.clone(),
            rom,
            family,
            format: family.format(),
            profile,
            mapsec16,
            groups,
            sections,
            maps: Vec::new(),
            tiles: RefCell::new(HashMap::new()),
            undo: Vec::new(),
            redo: Vec::new(),
            gbc: None,
        };
        game.maps = game.list_maps();
        Ok(game)
    }

    /// A Game Boy Color ROM: read into an image laid out as Emerald's ROM is (see `gbc`),
    /// which is what's drawn and edited from then on
    fn open_game_boy(bytes: &[u8], name: &str, profile: Profile) -> Result<Self, OpenError> {
        let Some(read) = gbc::read(bytes) else {
            let (title, code) = gbc::title(bytes);
            return Err(OpenError::NoGameBoyMaps { header: format!("{title} {code}").trim().to_string() });
        };
        let rom = Rom::new(read.image, name);
        let family = Family::Crystal;
        let mut game = Self {
            original: rom.clone(),
            rom,
            family,
            format: family.format(),
            profile,
            mapsec16: false,
            groups: read.groups,
            sections: None,
            maps: Vec::new(),
            tiles: RefCell::new(HashMap::new()),
            undo: Vec::new(),
            redo: Vec::new(),
            gbc: Some(Box::new(read.cartridge)),
        };
        game.maps = game.list_maps();
        Ok(game)
    }

    fn reader(&self) -> Reader<'_> {
        Reader { rom: &self.rom, format: self.format, mapsec16: self.mapsec16 }
    }

    /// The original game's map names, when this ROM's groups are still the original's:
    /// most hacks redraw maps where they are, and a build of a decompilation adds its
    /// own after them. A group whose size changed keeps its places' names instead.
    fn original_names(&self) -> &'static [(&'static str, &'static [&'static str])] {
        let names = match self.family {
            Family::Emerald => map_names::EMERALD,
            Family::FireRedLeafGreen => map_names::FIRERED,
            Family::Crystal => map_names::CRYSTAL,
            Family::RubySapphire => &[],
        };
        let same = names.iter().zip(&self.groups.groups).filter(|((_, maps), (_, headers))| maps.len() == headers.len()).count();
        if same * 5 >= names.len() * 4 {
            names
        } else {
            &[]
        }
    }

    fn list_maps(&self) -> Vec<MapInfo> {
        let reader = self.reader();
        let original = self.original_names();
        let mut maps = Vec::new();
        for (g, (_, headers)) in self.groups.groups.iter().enumerate() {
            let original_maps = original.get(g).map(|&(_, maps)| maps).filter(|maps| maps.len() == headers.len());
            for (n, &offset) in headers.iter().enumerate() {
                let Some(header) = reader.header(offset) else { continue };
                let Some(layout) = reader.layout(header.layout) else { continue };
                let id = MapId { group: g as u8, num: n as u8 };
                let section = self.section_name(header.mapsec);
                let name = self
                    .profile
                    .map_name(g, n)
                    .or_else(|| original_maps.map(|maps| maps[n]))
                    .map(str::to_string)
                    .or_else(|| section.clone())
                    .unwrap_or_else(|| format!("Map {id}"));
                let group_name = self
                    .profile
                    .group_name(g)
                    .or_else(|| original.get(g).map(|&(name, _)| name))
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Group {g}"));
                let original =
                    self.profile.original_layout(g, n).and_then(|id| self.layout_by_id(id)).filter(|o| o.offset != layout.offset);
                maps.push(MapInfo { id, name, section, group_name, header, layout, original });
            }
        }
        maps
    }

    /// A layout by its id: the entry `id - 1` of the layouts table the profile gives
    pub fn layout_by_id(&self, id: usize) -> Option<Layout> {
        let reader = self.reader();
        reader.layout(reader.aligned(self.profile.map_layouts? + id.checked_sub(1)? * 4)?)
    }

    /// A region map section's name: the profile's, else the ROM's
    pub fn section_name(&self, mapsec: u16) -> Option<String> {
        if let Some(names) = &self.profile.region_names {
            return names.get(mapsec).map(str::to_string);
        }
        if let Some(cartridge) = &self.gbc {
            return cartridge.place(mapsec).map(str::to_string);
        }
        self.sections?.get(&self.rom, mapsec)
    }

    /// What to call the game: the profile's name, else the family's
    pub fn name(&self) -> String {
        self.profile.name.clone().unwrap_or_else(|| self.family.name().to_string())
    }

    /// The ROM as it was opened (what the editor made of it then, for a Game Boy Color one)
    pub fn original(&self) -> &Rom {
        &self.original
    }

    /// The file this was opened from, as it was
    pub fn source(&self) -> &[u8] {
        self.gbc.as_ref().map_or(self.original.bytes(), |cartridge| cartridge.bytes())
    }

    /// The ROM with the edits, to save. A Game Boy Color game's is its own ROM with
    /// what changed written back into it.
    pub fn saved(&self) -> Vec<u8> {
        match &self.gbc {
            Some(cartridge) => cartridge.export(&self.rom, &self.original),
            None => self.rom.bytes().to_vec(),
        }
    }

    /// What the game's files end with
    pub fn extension(&self) -> &'static str {
        if self.gbc.is_some() {
            "gbc"
        } else {
            "gba"
        }
    }

    /// The palette a tile is drawn in, in the games where that goes with the tile and
    /// not with the metatile it's in
    pub fn tile_palette(&self, tileset: usize, tile: usize) -> Option<u8> {
        self.gbc.as_ref()?.tile_palette(tileset, tile)
    }

    /// The map of this game that is `other`'s map `index`: the same map in another build
    /// of the game, or the map it was made from in another game. By name when both games
    /// name their maps; else the map of the same place and size (the n-th of them when
    /// several are alike, as a town's houses are).
    pub fn counterpart(&self, other: &Game, index: usize) -> Option<usize> {
        let wanted = other.maps.get(index)?;
        // a map has a name of its own when it isn't just its place's
        let named = |m: &MapInfo| m.section.as_deref() != Some(m.name.as_str());
        if named(wanted) {
            let mut same = (0..self.maps.len()).filter(|&i| named(&self.maps[i]) && self.maps[i].name.eq_ignore_ascii_case(&wanted.name));
            if let (Some(i), None) = (same.next(), same.next()) {
                return Some(i);
            }
            // a hack that brings another game's maps may name them after their region:
            // "JohtoNewBarkTown" for "NewBarkTown". The longest name that ends the other is it.
            let ends = |long: &str, short: &str| {
                short.len() >= 4
                    && long.len() > short.len()
                    && long.is_char_boundary(long.len() - short.len())
                    && long[long.len() - short.len()..].eq_ignore_ascii_case(short)
            };
            let alike =
                |i: &usize| named(&self.maps[*i]) && (ends(&wanted.name, &self.maps[*i].name) || ends(&self.maps[*i].name, &wanted.name));
            if let Some(i) =
                (0..self.maps.len()).filter(alike).max_by_key(|&i| (self.maps[i].name.len().min(wanted.name.len()), std::cmp::Reverse(i)))
            {
                return Some(i);
            }
        }
        let alike = |m: &MapInfo| {
            m.section.is_some()
                && m.section == wanted.section
                && (m.layout.width, m.layout.height) == (wanted.layout.width, wanted.layout.height)
        };
        let rank = other.maps[..index].iter().filter(|m| alike(m)).count();
        let mine: Vec<usize> = (0..self.maps.len()).filter(|&i| alike(&self.maps[i])).collect();
        mine.get(rank).or(mine.first()).copied()
    }

    pub fn map_index(&self, id: MapId) -> Option<usize> {
        self.maps.iter().position(|m| m.id == id)
    }

    // -- maps -------------------------------------------------------------------

    pub fn load_map(&self, index: usize) -> Option<Map> {
        self.map_of(index, self.maps.get(index)?.layout)
    }

    /// A map as it was before the hack redrew it, when the ROM keeps it (see
    /// `MapInfo::original`): its blocks then, on its tilesets then. To look at: it's
    /// no map of the game's.
    pub fn load_original(&self, index: usize) -> Option<Map> {
        self.map_of(index, self.maps.get(index)?.original?)
    }

    fn map_of(&self, index: usize, l: Layout) -> Option<Map> {
        let info = self.maps.get(index)?;
        let read = |at: usize, count: usize| (0..count).map(|i| self.rom.u16(at + i * 2)).collect::<Vec<u16>>();
        Some(Map {
            index,
            id: info.id,
            name: info.name.clone(),
            width: l.width,
            height: l.height,
            blocks: read(l.blocks, l.width * l.height),
            border: read(l.border, l.border_width * l.border_height),
            border_width: l.border_width,
            border_height: l.border_height,
            layout: l,
            pair: l.pair(),
        })
    }

    /// Writes a map's blocks and border into the ROM (to be committed)
    pub fn store_map(&mut self, map: &Map) {
        for (i, &b) in map.blocks.iter().enumerate().take(map.layout.width * map.layout.height) {
            self.rom.write16(map.layout.blocks + i * 2, b);
        }
        for (i, &b) in map.border.iter().enumerate().take(map.layout.border_width * map.layout.border_height) {
            self.rom.write16(map.layout.border + i * 2, b);
        }
    }

    /// The maps drawn from the same layout as this one (itself included), or from the
    /// same blocks (a Game Boy Color game's houses and gates often are): they change together
    pub fn layout_users(&self, layout: usize) -> Vec<usize> {
        let blocks = self.maps.iter().find(|m| m.layout.offset == layout).map(|m| m.layout.blocks);
        (0..self.maps.len()).filter(|&i| self.maps[i].layout.offset == layout || Some(self.maps[i].layout.blocks) == blocks).collect()
    }

    /// Does a layout's data differ from the ROM as it was opened?
    pub fn layout_edited(&self, l: &Layout) -> bool {
        let differs = |at: usize, len: usize| self.rom.bytes().get(at..at + len) != self.original.bytes().get(at..at + len);
        differs(l.blocks, l.width * l.height * 2) || differs(l.border, l.border_width * l.border_height * 2)
    }

    /// The layouts (their offsets) edited since the ROM was opened
    pub fn edited_layouts(&self) -> BTreeSet<usize> {
        self.maps.iter().filter(|m| self.layout_edited(&m.layout)).map(|m| m.layout.offset).collect()
    }

    // -- tilesets ----------------------------------------------------------------

    pub fn tileset(&self, offset: usize) -> Option<Tileset> {
        self.reader().tileset(offset)
    }

    /// Every tileset the maps use, with whether it's used as a primary one
    pub fn tilesets(&self) -> Vec<(Tileset, bool)> {
        let mut seen = BTreeSet::new();
        for m in &self.maps {
            seen.insert((m.layout.primary, true));
            seen.insert((m.layout.secondary, false));
        }
        seen.into_iter().filter_map(|(offset, primary)| Some((self.tileset(offset)?, primary))).collect()
    }

    /// The tileset pairs the maps use
    pub fn pairs(&self) -> BTreeSet<Pair> {
        self.maps.iter().map(|m| m.layout.pair()).collect()
    }

    /// The tileset struct at an offset, as it was when the ROM was opened
    pub(crate) fn original_tileset(&self, offset: usize) -> Option<Tileset> {
        Reader { rom: &self.original, format: self.format, mapsec16: self.mapsec16 }.tileset(offset)
    }

    /// The tiles read so far are read again: the ROM changed under them
    pub(crate) fn forget_tiles(&self) {
        self.tiles.borrow_mut().clear();
    }

    /// A tileset's tile graphics (4bpp), decompressed if needed
    pub(crate) fn tiles(&self, offset: usize, primary: bool) -> Rc<Vec<u8>> {
        if let Some(t) = self.tiles.borrow().get(&offset) {
            return t.clone();
        }
        let data = self
            .tileset(offset)
            .map(|t| {
                if t.compressed {
                    self.rom.lz77(t.tiles).unwrap_or_default()
                } else {
                    let count = if primary { self.format.primary_tiles } else { self.format.secondary_tiles() };
                    let end = (t.tiles + count * 32).min(self.rom.len());
                    self.rom.bytes()[t.tiles..end].to_vec()
                }
            })
            .unwrap_or_default();
        let data = Rc::new(data);
        self.tiles.borrow_mut().insert(offset, data.clone());
        data
    }

    /// The pair's 16 palettes: the first ones from the primary tileset, the rest from the secondary
    pub fn palettes(&self, pair: Pair) -> Vec<Palette> {
        let read = |offset: usize| {
            self.tileset(offset).map(|t| gfx::read_palettes(self.rom.bytes(), t.palettes, 16)).unwrap_or_else(|| vec![[[0; 3]; 16]; 16])
        };
        let mut palettes = read(pair.primary);
        let secondary = read(pair.secondary);
        palettes[self.format.primary_palettes..].copy_from_slice(&secondary[self.format.primary_palettes..]);
        palettes
    }

    pub fn gfx(&self, pair: Pair) -> PairGfx {
        PairGfx {
            format: self.format,
            primary: self.tiles(pair.primary, true),
            secondary: self.tiles(pair.secondary, false),
            palettes: self.palettes(pair),
        }
    }

    /// How many metatiles a tileset holds. The ROM doesn't say: its metatiles run until
    /// its attributes when those follow (they do in the games and their decompilations),
    /// else as many as its kind may hold.
    pub fn tileset_metatiles(&self, t: &Tileset, primary: bool) -> usize {
        let mut n = if primary { self.format.primary_metatiles } else { self.format.secondary_metatiles() };
        if t.attributes > t.metatiles {
            n = n.min((t.attributes - t.metatiles) / METATILE_BYTES);
        }
        n.min(self.rom.len().saturating_sub(t.metatiles) / METATILE_BYTES)
    }

    /// The pair's metatile ids run from 0 to this (the secondary's start after the primary's share)
    pub fn metatile_count(&self, pair: Pair) -> usize {
        let secondary = self.tileset(pair.secondary).map_or(0, |t| self.tileset_metatiles(&t, false));
        // where a map has one tileset, every id is its own: as many as it holds
        let primary = match self.format.secondary_metatiles() {
            0 => self.tileset(pair.primary).map_or(0, |t| self.tileset_metatiles(&t, true)),
            _ => self.format.primary_metatiles,
        };
        primary + secondary
    }

    /// The tileset a metatile id belongs to and its index there; `None` when the id holds nothing
    pub fn locate(&self, pair: Pair, id: u16) -> Option<(Tileset, usize)> {
        let id = id as usize;
        let (offset, local, primary) = if id < self.format.primary_metatiles {
            (pair.primary, id, true)
        } else {
            (pair.secondary, id - self.format.primary_metatiles, false)
        };
        let t = self.tileset(offset)?;
        (local < self.tileset_metatiles(&t, primary)).then_some((t, local))
    }

    pub fn tileset_entries(&self, t: &Tileset, local: usize) -> [u16; ENTRIES] {
        entries_in(&self.rom, t, local)
    }

    pub fn tileset_attribute(&self, t: &Tileset, local: usize) -> u32 {
        attribute_in(&self.rom, self.format, t, local)
    }

    pub fn set_tileset_entries(&mut self, t: &Tileset, local: usize, entries: &[u16; ENTRIES]) {
        for (k, &e) in entries.iter().enumerate() {
            self.rom.write16(t.metatiles + local * METATILE_BYTES + k * 2, e);
        }
    }

    pub fn set_tileset_attribute(&mut self, t: &Tileset, local: usize, value: u32) {
        let o = t.attributes + local * self.format.attribute_size;
        if self.format.attribute_size == 4 {
            self.rom.write32(o, value);
        } else {
            self.rom.write16(o, value as u16);
        }
    }

    /// A metatile's 8 tile entries (blank when the id holds nothing)
    pub fn entries(&self, pair: Pair, id: u16) -> [u16; ENTRIES] {
        self.locate(pair, id).map_or([0; ENTRIES], |(t, local)| self.tileset_entries(&t, local))
    }

    pub fn attribute(&self, pair: Pair, id: u16) -> u32 {
        self.locate(pair, id).map_or(0, |(t, local)| self.tileset_attribute(&t, local))
    }

    pub fn behavior(&self, pair: Pair, id: u16) -> u32 {
        self.format.behavior.get(self.attribute(pair, id))
    }

    pub fn set_entries(&mut self, pair: Pair, id: u16, entries: &[u16; ENTRIES]) {
        if let Some((t, local)) = self.locate(pair, id) {
            self.set_tileset_entries(&t, local, entries);
        }
    }

    pub fn set_attribute(&mut self, pair: Pair, id: u16, value: u32) {
        if let Some((t, local)) = self.locate(pair, id) {
            self.set_tileset_attribute(&t, local, value);
        }
    }

    /// How each metatile of a pair is used across the maps drawn with it
    pub fn block_usage(&self, pair: Pair) -> HashMap<u16, BlockUsage> {
        let mut usage: HashMap<u16, BlockUsage> = HashMap::new();
        let mut seen = BTreeSet::new();
        for m in self.maps.iter().filter(|m| m.layout.pair() == pair && seen.insert(m.layout.offset)) {
            for i in 0..m.layout.width * m.layout.height {
                let b = self.rom.u16(m.layout.blocks + i * 2);
                usage.entry(block::metatile(b)).or_default().counts[(b >> 10) as usize] += 1;
            }
        }
        usage
    }

    // -- undo ---------------------------------------------------------------------

    /// Ends an edit: what was written since the last commit becomes one undoable step.
    /// Returns whether anything changed.
    pub fn commit(&mut self) -> bool {
        let patch = self.rom.take_patch();
        if patch.is_empty() {
            return false;
        }
        self.undo.push(patch);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last step. Returns where in the ROM it had changed something first
    /// (see `map_at`), or `None` when there's nothing to undo.
    pub fn undo(&mut self) -> Option<usize> {
        self.commit_pending();
        let patch = self.undo.pop()?;
        self.rom.revert(&patch);
        self.forget_tiles();
        let at = patch.changes.first().map(|c| c.offset as usize);
        self.redo.push(patch);
        at
    }

    pub fn redo(&mut self) -> Option<usize> {
        self.commit_pending();
        let patch = self.redo.pop()?;
        self.rom.reapply(&patch);
        self.forget_tiles();
        let at = patch.changes.first().map(|c| c.offset as usize);
        self.undo.push(patch);
        at
    }

    /// The first map whose blocks or border hold this ROM offset
    pub fn map_at(&self, offset: usize) -> Option<usize> {
        self.maps.iter().position(|m| {
            let l = &m.layout;
            (l.blocks..l.blocks + l.width * l.height * 2).contains(&offset)
                || (l.border..l.border + l.border_width * l.border_height * 2).contains(&offset)
        })
    }

    // Writes nobody committed are a step of their own: undo never skips over them
    fn commit_pending(&mut self) {
        let patch = self.rom.take_patch();
        if !patch.is_empty() {
            self.undo.push(patch);
            self.redo.clear();
        }
    }

    /// Does the ROM differ from the one opened?
    pub fn modified(&self) -> bool {
        self.rom.bytes() != self.original.bytes()
    }

    // -- drawing ------------------------------------------------------------------

    /// Every metatile of a pair, `columns` to a row
    pub fn render_sheet(&self, pair: Pair, columns: usize) -> Image {
        let count = self.metatile_count(pair);
        let gfx = self.gfx(pair);
        let mut image = Image::filled(columns * METATILE, count.div_ceil(columns).max(1) * METATILE, [0, 0, 0, 0]);
        for id in 0..count {
            if self.locate(pair, id as u16).is_some() {
                gfx.draw_metatile(&mut image, (id % columns) * METATILE, (id / columns) * METATILE, &self.entries(pair, id as u16));
            }
        }
        image
    }

    /// One layer of a metatile (0: bottom, 1: top) on a transparent ground, 16x16
    pub fn render_layer(&self, pair: Pair, entries: &[u16; ENTRIES], layer: usize) -> Image {
        let gfx = self.gfx(pair);
        let mut image = Image::new(METATILE, METATILE);
        for q in 0..4 {
            gfx::draw_entry(
                &mut image.rgba,
                METATILE,
                (q % 2) * TILE,
                (q / 2) * TILE,
                entries[layer * 4 + q],
                |t| gfx.tile(t),
                &gfx.palettes,
                true,
            );
        }
        image
    }

    /// A whole map
    pub fn render_map(&self, map: &Map) -> Image {
        let gfx = self.gfx(map.pair);
        let mut image = Image::filled(map.width * METATILE, map.height * METATILE, [255, 0, 255, 255]);
        let mut cache: HashMap<u16, [u16; ENTRIES]> = HashMap::new();
        for y in 0..map.height {
            for x in 0..map.width {
                let id = block::metatile(map.blocks[y * map.width + x]);
                if self.locate(map.pair, id).is_none() {
                    continue;
                }
                let entries = *cache.entry(id).or_insert_with(|| self.entries(map.pair, id));
                gfx.draw_metatile(&mut image, x * METATILE, y * METATILE, &entries);
            }
        }
        image
    }
}

/// The metatile of a sheet (see `Game::render_sheet`: `count` metatiles, `columns` to a
/// row) that looks most like a 16x16 picture: the one whose pixels differ the least
pub fn closest_metatile(sheet: &Image, columns: usize, count: usize, target: &Image) -> Option<u16> {
    let mut best: Option<(u64, u16)> = None;
    for id in 0..count {
        let (ox, oy) = ((id % columns) * METATILE, (id / columns) * METATILE);
        if oy + METATILE > sheet.height || ox + METATILE > sheet.width || sheet.pixel(ox, oy)[3] == 0 {
            continue; // nothing at this id
        }
        let limit = best.map_or(u64::MAX, |(d, _)| d);
        let mut distance = 0u64;
        'pixels: for y in 0..METATILE.min(target.height) {
            for x in 0..METATILE.min(target.width) {
                let (a, b) = (sheet.pixel(ox + x, oy + y), target.pixel(x, y));
                distance += (0..3).map(|c| (a[c] as i64 - b[c] as i64).pow(2) as u64).sum::<u64>();
                if distance >= limit {
                    break 'pixels;
                }
            }
        }
        if distance < limit {
            best = Some((distance, id as u16));
        }
    }
    best.map(|(_, id)| id)
}
