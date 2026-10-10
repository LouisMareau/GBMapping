//! The edits file ("GBMapping/1"): what changed in a ROM, small enough to share and
//! to apply to another build of the same game. Maps go by "group.number", tilesets by
//! ROM address.
//!
//! ```json
//! {"format": "GBMapping/1", "game": "BPEE", "title": "POKEMON EMER",
//!  "maps": {"0.9": {"width": 20, "height": 20, "blocks": [...], "border": [...]}},
//!  "tilesets": {"0x3DF704": {"metatiles": {"12": [1, 2, 3, 4, 5, 6, 7, 8]}, "attributes": {"12": 3}}}}
//! ```

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
            if !edit.metatiles.is_empty() || !edit.attributes.is_empty() {
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
            self.apply_tileset_edit(&t, e);
            applied.tilesets += 1;
        }
        applied
    }

    fn apply_tileset_edit(&mut self, t: &Tileset, e: &TilesetEdit) {
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
    }
}
