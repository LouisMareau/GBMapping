//! The edits file ("GBMapping/1"): what changed in a ROM, small enough to share and
//! to apply to another build of the same game. Maps go by "group.number", tilesets by
//! ROM address.
//!
//! ```json
//! {"format": "GBMapping/1", "game": "BPEE", "title": "POKEMON EMER",
//!  "maps": {"0.9": {"width": 20, "height": 20, "blocks": [...], "border": [...]}},
//!  "tilesets": {"0x3DF704": {"metatiles": {"12": [1, 2, 3, 4, 5, 6, 7, 8]}, "attributes": {"12": 3},
//!                            "tiles": {"498": "00112233..."}, "palettes": {"12": [0, 32767, ...]}}}}
//! ```
//!
//! `tiles` and `palettes` are there when tiles were brought from another tileset: a
//! tile's 32 bytes in hexadecimal (4 bits a pixel, as the console has them) by its
//! number in the tileset, a palette's 16 colours (15 bits each) by its number.

use crate::format::ENTRIES;
use crate::game::{attribute_in, entries_in, Game, MapId};
use crate::structs::Tileset;
use crate::tiles::TileUsage;
use crate::tilesets::TilesetContent;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT: &str = "GBMapping/1";

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Edits {
    pub format: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub maps: BTreeMap<String, MapEdit>,
    #[serde(default)]
    pub tilesets: BTreeMap<String, TilesetEdit>,
    /// The maps' originals (the layouts a profile tells the ROM keeps them in) that were
    /// renumbered: metatiles that changed places are followed there too
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub originals: BTreeMap<String, MapEdit>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct MapEdit {
    pub width: usize,
    pub height: usize,
    pub blocks: Vec<u16>,
    #[serde(default)]
    pub border: Vec<u16>,
    /// The tilesets the map was given, when they aren't the ones it had
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct TilesetEdit {
    /// Metatile index in the tileset -> its 8 tile entries
    #[serde(default)]
    pub metatiles: BTreeMap<String, Vec<u16>>,
    /// Metatile index in the tileset -> its attributes
    #[serde(default)]
    pub attributes: BTreeMap<String, u32>,
    /// Tile index in the tileset -> its 32 bytes (4 bits a pixel), in hexadecimal
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiles: BTreeMap<String, String>,
    /// Palette index in the tileset -> its 16 colours (15 bits each)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub palettes: BTreeMap<String, Vec<u16>>,
    /// A tileset the ROM didn't have: everything above is then all of it, not what changed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<NewTileset>,
}

/// A tileset made in the editor
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTileset {
    pub secondary: bool,
    /// The tileset it was made as a copy of: its tiles are animated the same
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<String>,
}

impl TilesetEdit {
    pub fn is_empty(&self) -> bool {
        self.metatiles.is_empty() && self.attributes.is_empty() && self.tiles.is_empty() && self.palettes.is_empty() && self.new.is_none()
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim();
    (hex.len().is_multiple_of(2) && hex.is_ascii())
        .then(|| (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect())?
}

/// What applying an edits file did
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub maps: usize,
    pub tilesets: usize,
    /// What couldn't be applied, and why
    pub skipped: Vec<String>,
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.maps.is_empty() && self.tilesets.is_empty() && self.originals.is_empty()
    }

    /// How many maps and tilesets the file changes
    pub fn len(&self) -> usize {
        self.maps.len() + self.tilesets.len()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let edits: Self = serde_json::from_str(json).map_err(|e| format!("not an edits file: {e}"))?;
        if !edits.format.starts_with("GBMapping/") {
            return Err(format!("not an edits file (format \"{}\")", edits.format));
        }
        Ok(edits)
    }

    /// Is this JSON an edits file (and not a profile)?
    pub fn looks_like(json: &str) -> bool {
        #[derive(Deserialize)]
        struct Probe {
            format: String,
        }
        serde_json::from_str::<Probe>(json).is_ok_and(|p| p.format.starts_with("GBMapping/"))
    }
}

fn tileset_key(offset: usize) -> String {
    format!("0x{offset:06X}")
}

fn parse_address(key: &str) -> Option<usize> {
    let key = key.trim();
    match key.strip_prefix("0x").or_else(|| key.strip_prefix("0X")) {
        Some(hex) => usize::from_str_radix(hex, 16).ok(),
        None => key.parse().ok(),
    }
}

impl Game {
    /// Everything that differs from the ROM as it was opened
    pub fn collect_edits(&self) -> Edits {
        let header = self.rom.header();
        let mut edits = Edits { format: FORMAT.to_string(), game: header.code, title: header.title, ..Default::default() };
        // A layout shared by several maps is told once, by the first of them
        let mut layouts = BTreeSet::new();
        for (index, info) in self.maps.iter().enumerate() {
            if !layouts.insert(info.layout.offset) || !self.layout_edited(&info.layout) {
                continue;
            }
            if let Some(m) = self.load_map(index) {
                let given = self.layout_tilesets_changed(&info.layout);
                let (primary, secondary) = (given.0.then(|| tileset_key(m.pair.primary)), given.1.then(|| tileset_key(m.pair.secondary)));
                edits.maps.insert(
                    info.id.to_string(),
                    MapEdit { width: m.width, height: m.height, blocks: m.blocks, border: m.border, primary, secondary },
                );
            }
        }
        // the maps' originals aren't edited, but they're renumbered when metatiles change places
        for (index, info) in self.maps.iter().enumerate() {
            let Some(original) = info.original.filter(|l| self.layout_edited(l)) else { continue };
            if let Some(m) = self.load_original(index).filter(|_| layouts.insert(original.offset)) {
                let renumbered =
                    MapEdit { width: m.width, height: m.height, blocks: m.blocks, border: m.border, primary: None, secondary: None };
                edits.originals.insert(info.id.to_string(), renumbered);
            }
        }
        let mut seen = BTreeSet::new();
        let usage = self.tile_usage();
        for (t, primary) in self.tilesets() {
            if !seen.insert(t.offset) {
                continue;
            }
            let mut edit = TilesetEdit::default();
            // the tileset as it was: its metatiles may have been moved since, and added to
            let before = self.original_tileset(t.offset);
            let had = before.map_or(0, |before| self.tileset_metatiles(&before, primary));
            if before.is_none() {
                edit.new = Some(NewTileset { secondary: !primary, copy_of: self.copied_from(t.offset).map(tileset_key) });
            }
            for i in 0..self.tileset_metatiles(&t, primary) {
                // a metatile the tileset didn't have is told whole
                let then = before.filter(|_| i < had);
                let entries = self.tileset_entries(&t, i);
                if then.is_none_or(|then| entries != entries_in(self.original(), &then, i)) {
                    edit.metatiles.insert(i.to_string(), entries.to_vec());
                }
                let attribute = self.tileset_attribute(&t, i);
                if then.is_none_or(|then| attribute != attribute_in(self.original(), self.format, &then, i)) {
                    edit.attributes.insert(i.to_string(), attribute);
                }
            }
            self.collect_tiles(&t, primary, &usage, &mut edit);
            if !edit.is_empty() {
                edits.tilesets.insert(tileset_key(t.offset), edit);
            }
        }
        edits
    }

    /// Applies an edits file (to be committed). A map of another size, a map or a tileset
    /// this ROM doesn't have are skipped and reported.
    pub fn apply_edits(&mut self, edits: &Edits) -> Applied {
        let mut applied = Applied::default();
        // the tilesets the file makes come first: maps may be given them
        let mut made: BTreeMap<&str, usize> = BTreeMap::new();
        for (key, e) in &edits.tilesets {
            let there = parse_address(key).is_some_and(|o| self.tileset(o).is_some());
            let Some(new) = e.new.as_ref().filter(|_| !there) else { continue };
            match self.make_tileset(e, new) {
                Ok(at) => {
                    made.insert(key, at);
                    applied.tilesets += 1;
                }
                Err(problem) => applied.skipped.push(format!("tileset {key}: {problem}")),
            }
        }
        for (key, e) in &edits.maps {
            let index = key.parse::<MapId>().ok().and_then(|id| self.map_index(id));
            let Some(mut map) = index.and_then(|i| self.load_map(i)) else {
                applied.skipped.push(format!("map {key}: not in this ROM"));
                continue;
            };
            if (e.width, e.height) != (map.width, map.height) || e.blocks.len() != map.blocks.len() {
                applied.skipped.push(format!(
                    "map {key} ({}): {}x{} in the file, {}x{} in this ROM",
                    map.name, e.width, e.height, map.width, map.height
                ));
                continue;
            }
            map.blocks.copy_from_slice(&e.blocks);
            if e.border.len() == map.border.len() {
                map.border.copy_from_slice(&e.border);
            }
            self.store_map(&map);
            // the tilesets it was given: one the file made, or one of the ROM's
            for (primary, tileset) in [(true, &e.primary), (false, &e.secondary)] {
                let Some(tileset) = tileset else { continue };
                let at = made.get(tileset.as_str()).copied().or_else(|| parse_address(tileset));
                if !at.is_some_and(|at| self.set_layout_tileset(map.layout.offset, primary, at)) {
                    applied.skipped.push(format!("map {key} ({}): its tileset {tileset} isn't in this ROM", map.name));
                }
            }
            applied.maps += 1;
        }
        for (key, e) in &edits.originals {
            let index = key.parse::<MapId>().ok().and_then(|id| self.map_index(id));
            match index
                .and_then(|i| self.load_original(i))
                .filter(|m| (m.width, m.height, m.blocks.len(), m.border.len()) == (e.width, e.height, e.blocks.len(), e.border.len()))
            {
                Some(mut original) => {
                    original.blocks.copy_from_slice(&e.blocks);
                    original.border.copy_from_slice(&e.border);
                    self.store_map(&original);
                }
                None => applied.skipped.push(format!("the original of map {key}: not in this ROM")),
            }
        }
        for (key, e) in &edits.tilesets {
            if made.contains_key(key.as_str()) {
                continue;
            }
            let Some(t) = parse_address(key).and_then(|o| self.tileset(o)) else {
                applied.skipped.push(format!("tileset {key}: not in this ROM"));
                continue;
            };
            let problems = self.apply_tileset_edit(&t, e);
            applied.skipped.extend(problems.into_iter().map(|problem| format!("tileset {key}: {problem}")));
            applied.tilesets += 1;
        }
        applied
    }

    /// Makes the tileset an edit tells whole: where it is in this ROM
    fn make_tileset(&mut self, e: &TilesetEdit, new: &NewTileset) -> Result<usize, String> {
        let numbered = |i: &String| i.parse::<usize>().ok();
        let count = e.metatiles.keys().chain(e.attributes.keys()).filter_map(numbered).max().map_or(0, |last| last + 1);
        let mut content = TilesetContent {
            secondary: new.secondary,
            callback: 0,
            tiles: vec![0; e.tiles.keys().filter_map(numbered).max().map_or(0, |last| last + 1).min(1024) * 32],
            palettes: vec![0; 256],
            metatiles: vec![[0; ENTRIES]; count.min(1024)],
            attributes: vec![0; count.min(1024)],
        };
        for (i, entries) in &e.metatiles {
            if let (Some(place), Ok(entries)) =
                (numbered(i).and_then(|i| content.metatiles.get_mut(i)), <[u16; ENTRIES]>::try_from(entries.as_slice()))
            {
                *place = entries;
            }
        }
        for (i, &attribute) in &e.attributes {
            if let Some(place) = numbered(i).and_then(|i| content.attributes.get_mut(i)) {
                *place = attribute;
            }
        }
        for (i, hex) in &e.tiles {
            if let (Some(i), Some(tile)) = (numbered(i).filter(|&i| i < 1024), from_hex(hex).filter(|tile| tile.len() == 32)) {
                content.tiles[i * 32..(i + 1) * 32].copy_from_slice(&tile);
            }
        }
        for (p, colors) in &e.palettes {
            if let Some(p) = numbered(p).filter(|&p| p < 16 && colors.len() == 16) {
                content.palettes[p * 16..(p + 1) * 16].copy_from_slice(colors);
            }
        }
        // its tiles are animated as those of the tileset it's the copy of, when this ROM has that one
        let source = new.copy_of.as_deref().and_then(parse_address).filter(|&o| self.tileset(o).is_some());
        content.callback = source.map_or(0, |o| self.rom.u32(o + self.format.tileset_callback));
        let at = self.create_tileset(&content).map_err(|e| e.to_string())?;
        if let Some(source) = source {
            self.copies.insert(at, source);
        }
        Ok(at)
    }

    /// A tileset's tiles and palettes that aren't what they were
    fn collect_tiles(&self, t: &Tileset, primary: bool, usage: &TileUsage, edit: &mut TilesetEdit) {
        let (now, then) = (self.rom.bytes(), self.original().bytes());
        let colors = |bytes: &[u8]| bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect::<Vec<u16>>();
        // a tileset the ROM didn't have: every tile that isn't empty, every palette
        let Some(before) = self.original_tileset(t.offset) else {
            for (i, tile) in self.tiles(t.offset, primary).as_chunks::<32>().0.iter().enumerate() {
                if tile.iter().any(|&b| b != 0) {
                    edit.tiles.insert(i.to_string(), to_hex(tile));
                }
            }
            for p in 0..16 {
                if let Some(palette) = now.get(t.palettes + p * 32..t.palettes + p * 32 + 32) {
                    edit.palettes.insert(p.to_string(), colors(palette));
                }
            }
            return;
        };
        // the tiles, unless they're the same bytes at the same place
        let packed = |rom: &crate::rom::Rom, at: usize| rom.lz77_len(at).and_then(|len| rom.bytes().get(at..at + len).map(<[u8]>::to_vec));
        if t.compressed && (t.tiles != before.tiles || packed(&self.rom, t.tiles) != packed(self.original(), before.tiles)) {
            let tiles = self.tiles(t.offset, primary);
            let old = self.original().lz77(before.tiles).unwrap_or_default();
            for (i, tile) in tiles.as_chunks::<32>().0.iter().enumerate() {
                if old.get(i * 32..i * 32 + 32) != Some(&tile[..]) {
                    edit.tiles.insert(i.to_string(), to_hex(tile));
                }
            }
        } else if !t.compressed && !before.compressed {
            // not compressed: the tiles are where they were, but how many they are can't be
            // told. Only those a metatile draws are tiles for sure: what follows is something else.
            let tiles = self.tiles(t.offset, primary);
            for (i, tile) in tiles.as_chunks::<32>().0.iter().enumerate() {
                if usage.uses(t.offset, i) > 0 && then.get(before.tiles + i * 32..before.tiles + i * 32 + 32) != Some(&tile[..]) {
                    edit.tiles.insert(i.to_string(), to_hex(tile));
                }
            }
        }
        for p in 0..16 {
            let at = |palettes: usize| palettes + p * 32..palettes + p * 32 + 32;
            if let (Some(palette), Some(old)) = (now.get(at(t.palettes)), then.get(at(before.palettes))) {
                if palette != old {
                    edit.palettes.insert(p.to_string(), colors(palette));
                }
            }
        }
    }

    /// Returns what couldn't be applied
    fn apply_tileset_edit(&mut self, t: &Tileset, e: &TilesetEdit) -> Vec<String> {
        let mut problems = Vec::new();
        if !e.tiles.is_empty() {
            let most = self.format.primary_tiles.max(self.format.secondary_tiles());
            let mut data = (*self.tiles(t.offset, !t.secondary)).clone();
            for (i, hex) in &e.tiles {
                match (i.parse::<usize>(), from_hex(hex)) {
                    (Ok(i), Some(tile)) if i < most && tile.len() == 32 => {
                        if data.len() < (i + 1) * 32 {
                            data.resize((i + 1) * 32, 0);
                        }
                        data[i * 32..(i + 1) * 32].copy_from_slice(&tile);
                    }
                    _ => problems.push(format!("tile {i} isn't one")),
                }
            }
            if let Err(e) = self.put_tiles(t, !t.secondary, &data) {
                problems.push(format!("its tiles: {e}"));
            }
        }
        for (p, colors) in &e.palettes {
            match p.parse::<usize>() {
                Ok(p) if p < 16 && colors.len() == 16 => {
                    for (c, &color) in colors.iter().enumerate() {
                        self.rom.write16(t.palettes + (p * 16 + c) * 2, color);
                    }
                }
                _ => problems.push(format!("palette {p} isn't one")),
            }
        }
        // metatiles the tileset doesn't have yet: it's given them first
        let primary = !t.secondary;
        let wanted = e.metatiles.keys().chain(e.attributes.keys()).filter_map(|i| i.parse::<usize>().ok()).max().map_or(0, |i| i + 1);
        let mut t = *t;
        if wanted > self.tileset_metatiles(&t, primary) {
            match self.grow_metatiles(&t, primary, wanted) {
                Ok(grown) => t = grown,
                Err(e) => problems.push(format!("its new metatiles: {e}")),
            }
        }
        let count = self.tileset_metatiles(&t, primary);
        for (i, entries) in &e.metatiles {
            let (Ok(i), Ok(entries)) = (i.parse::<usize>(), <[u16; ENTRIES]>::try_from(entries.as_slice())) else { continue };
            if i < count {
                self.set_tileset_entries(&t, i, &entries);
            }
        }
        for (i, &attribute) in &e.attributes {
            let Ok(i) = i.parse::<usize>() else { continue };
            if i < count {
                self.set_tileset_attribute(&t, i, attribute);
            }
        }
        problems
    }
}
