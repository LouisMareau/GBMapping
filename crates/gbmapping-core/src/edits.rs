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
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct MapEdit {
    pub width: usize,
    pub height: usize,
    pub blocks: Vec<u16>,
    #[serde(default)]
    pub border: Vec<u16>,
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
}

impl TilesetEdit {
    pub fn is_empty(&self) -> bool {
        self.metatiles.is_empty() && self.attributes.is_empty() && self.tiles.is_empty() && self.palettes.is_empty()
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
        self.maps.is_empty() && self.tilesets.is_empty()
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
                edits.maps.insert(info.id.to_string(), MapEdit { width: m.width, height: m.height, blocks: m.blocks, border: m.border });
            }
        }
        let mut seen = BTreeSet::new();
        for (t, primary) in self.tilesets() {
            if !seen.insert(t.offset) {
                continue;
            }
            let mut edit = TilesetEdit::default();
            for i in 0..self.tileset_metatiles(&t, primary) {
                let entries = self.tileset_entries(&t, i);
                if entries != entries_in(self.original(), &t, i) {
                    edit.metatiles.insert(i.to_string(), entries.to_vec());
                }
                let attribute = self.tileset_attribute(&t, i);
                if attribute != attribute_in(self.original(), self.format, &t, i) {
                    edit.attributes.insert(i.to_string(), attribute);
                }
            }
            self.collect_tiles(&t, primary, &mut edit);
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
            applied.maps += 1;
        }
        for (key, e) in &edits.tilesets {
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

    /// A tileset's tiles and palettes that aren't what they were
    fn collect_tiles(&self, t: &Tileset, primary: bool, edit: &mut TilesetEdit) {
        let Some(before) = self.original_tileset(t.offset) else { return };
        let (now, then) = (self.rom.bytes(), self.original().bytes());
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
        }
        for p in 0..16 {
            let at = |palettes: usize| palettes + p * 32..palettes + p * 32 + 32;
            if let (Some(colors), Some(old)) = (now.get(at(t.palettes)), then.get(at(before.palettes))) {
                if colors != old {
                    edit.palettes.insert(p.to_string(), colors.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect());
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
            if let Err(e) = self.store_tiles(t, &data) {
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
        let limit = self.format.primary_metatiles.max(self.format.secondary_metatiles());
        for (i, entries) in &e.metatiles {
            let (Ok(i), Ok(entries)) = (i.parse::<usize>(), <[u16; ENTRIES]>::try_from(entries.as_slice())) else { continue };
            if i < limit {
                self.set_tileset_entries(t, i, &entries);
            }
        }
        for (i, &attribute) in &e.attributes {
            let Ok(i) = i.parse::<usize>() else { continue };
            if i < limit {
                self.set_tileset_attribute(t, i, attribute);
            }
        }
        problems
    }
}
