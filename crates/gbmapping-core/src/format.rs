//! How a game family lays its map data out in the ROM.
//!
//! Ruby/Sapphire and Emerald share one layout; FireRed/LeafGreen moved a few
//! things (bigger primary tilesets, 32-bit metatile attributes, a border size
//! in the map layout).

/// The games an editor profile covers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    RubySapphire,
    Emerald,
    FireRedLeafGreen,
}

impl Family {
    /// From the cartridge header's game code ("BPEE": Emerald, English)
    pub fn from_code(code: &str) -> Option<Self> {
        match code.get(..3)? {
            "AXV" | "AXP" => Some(Self::RubySapphire),
            "BPE" => Some(Self::Emerald),
            "BPR" | "BPG" => Some(Self::FireRedLeafGreen),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::RubySapphire => "Pokémon Ruby/Sapphire",
            Self::Emerald => "Pokémon Emerald",
            Self::FireRedLeafGreen => "Pokémon FireRed/LeafGreen",
        }
    }

    pub fn format(self) -> &'static Format {
        match self {
            Self::RubySapphire => &HOENN_RS,
            Self::Emerald => &HOENN_E,
            Self::FireRedLeafGreen => &KANTO,
        }
    }

    /// A profile's name for the family ("pokeemerald", "pokefirered", "pokeruby")
    pub fn from_profile(game: &str) -> Option<Self> {
        let g = game.to_ascii_lowercase();
        if g.contains("emerald") {
            Some(Self::Emerald)
        } else if g.contains("firered") || g.contains("leafgreen") || g.contains("frlg") {
            Some(Self::FireRedLeafGreen)
        } else if g.contains("ruby") || g.contains("sapphire") {
            Some(Self::RubySapphire)
        } else {
            None
        }
    }
}

/// A named bit field of a metatile's attributes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrField {
    pub name: &'static str,
    pub mask: u32,
    pub shift: u32,
    /// The values' names, when they're few
    pub values: &'static [&'static str],
}

impl AttrField {
    pub fn get(&self, attr: u32) -> u32 {
        (attr & self.mask) >> self.shift
    }

    pub fn set(&self, attr: u32, value: u32) -> u32 {
        (attr & !self.mask) | ((value << self.shift) & self.mask)
    }

    pub fn max(&self) -> u32 {
        self.mask >> self.shift
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Format {
    pub family: Family,
    /// Metatiles a primary tileset holds; the secondary's ids start there
    pub primary_metatiles: usize,
    pub total_metatiles: usize,
    /// Tiles a primary tileset holds; the secondary's ids start there
    pub primary_tiles: usize,
    pub total_tiles: usize,
    /// Palettes taken from the primary tileset; the next ones come from the secondary
    pub primary_palettes: usize,
    pub total_palettes: usize,
    /// struct Tileset: where the metatile attributes pointer and the callback are
    pub tileset_attributes: usize,
    pub tileset_callback: usize,
    /// Bytes per metatile attribute
    pub attribute_size: usize,
    /// struct MapLayout: border width and height bytes after the tilesets (else 2x2)
    pub layout_border_size: bool,
    pub behavior: AttrField,
    pub layer_type: AttrField,
    /// The other fields worth editing
    pub extra_fields: &'static [AttrField],
    pub behaviors: &'static [(u16, &'static str)],
}

pub const TILESET_SIZE: usize = 24;
pub const HEADER_SIZE: usize = 28;

/// Tile entries per metatile: 4 for the bottom layer, 4 for the top
pub const ENTRIES: usize = 8;
pub const METATILE_BYTES: usize = ENTRIES * 2;

/// Which of the console's background layers a metatile's two layers go to
/// (the player walks between the middle and the top one)
const LAYER_TYPES: &[&str] = &["Normal (middle + top)", "Covered (bottom + middle)", "Split (bottom + top)"];

static HOENN_E: Format = hoenn(Family::Emerald, crate::behaviors::EMERALD);
// Ruby/Sapphire's behaviors aren't all Emerald's: shown as numbers
static HOENN_RS: Format = hoenn(Family::RubySapphire, &[]);

const fn hoenn(family: Family, behaviors: &'static [(u16, &'static str)]) -> Format {
    Format {
        family,
        primary_metatiles: 512,
        total_metatiles: 1024,
        primary_tiles: 512,
        total_tiles: 1024,
        primary_palettes: 6,
        total_palettes: 13,
        tileset_attributes: 16,
        tileset_callback: 20,
        attribute_size: 2,
        layout_border_size: false,
        behavior: AttrField { name: "Behavior", mask: 0x00FF, shift: 0, values: &[] },
        layer_type: AttrField { name: "Layer type", mask: 0xF000, shift: 12, values: LAYER_TYPES },
        extra_fields: &[],
        behaviors,
    }
}

static KANTO: Format = Format {
    family: Family::FireRedLeafGreen,
    primary_metatiles: 640,
    total_metatiles: 1024,
    primary_tiles: 640,
    total_tiles: 1024,
    primary_palettes: 7,
    total_palettes: 13,
    tileset_attributes: 20,
    tileset_callback: 16,
    attribute_size: 4,
    layout_border_size: true,
    behavior: AttrField { name: "Behavior", mask: 0x0000_01FF, shift: 0, values: &[] },
    layer_type: AttrField { name: "Layer type", mask: 0x6000_0000, shift: 29, values: LAYER_TYPES },
    extra_fields: &[
        AttrField { name: "Terrain", mask: 0x0000_3E00, shift: 9, values: &["Normal", "Grass", "Water", "Waterfall"] },
        AttrField { name: "Encounter", mask: 0x0700_0000, shift: 24, values: &["None", "Land", "Water"] },
    ],
    behaviors: crate::behaviors::FIRERED,
};

impl Format {
    pub fn behavior_name(&self, behavior: u32) -> Option<&'static str> {
        let i = self.behaviors.binary_search_by_key(&behavior, |&(id, _)| id as u32).ok()?;
        Some(self.behaviors[i].1)
    }

    /// The secondary tileset's share of the metatile ids
    pub fn secondary_metatiles(&self) -> usize {
        self.total_metatiles - self.primary_metatiles
    }

    pub fn secondary_tiles(&self) -> usize {
        self.total_tiles - self.primary_tiles
    }
}

/// A map block: `metatile | collision << 10 | elevation << 12`
pub mod block {
    pub const METATILE_MASK: u16 = 0x03FF;
    pub const COLLISION_MASK: u16 = 0x0C00;
    pub const ELEVATION_MASK: u16 = 0xF000;

    pub fn metatile(b: u16) -> u16 {
        b & METATILE_MASK
    }
    pub fn collision(b: u16) -> u8 {
        ((b & COLLISION_MASK) >> 10) as u8
    }
    pub fn elevation(b: u16) -> u8 {
        (b >> 12) as u8
    }
    pub fn with_metatile(b: u16, id: u16) -> u16 {
        (b & !METATILE_MASK) | (id & METATILE_MASK)
    }
    pub fn with_collision(b: u16, c: u8) -> u16 {
        (b & !COLLISION_MASK) | ((c as u16 & 3) << 10)
    }
    pub fn with_elevation(b: u16, e: u8) -> u16 {
        (b & !ELEVATION_MASK) | ((e as u16 & 0xF) << 12)
    }
    pub fn pack(id: u16, collision: u8, elevation: u8) -> u16 {
        with_elevation(with_collision(id & METATILE_MASK, collision), elevation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(Family::from_code("BPEE"), Some(Family::Emerald));
        assert_eq!(Family::from_code("BPEF"), Some(Family::Emerald));
        assert_eq!(Family::from_code("BPRE"), Some(Family::FireRedLeafGreen));
        assert_eq!(Family::from_code("AXVE"), Some(Family::RubySapphire));
        assert_eq!(Family::from_code("AGBJ"), None);
        assert_eq!(Family::from_code(""), None);
        assert_eq!(Family::from_profile("pokeemerald"), Some(Family::Emerald));
    }

    #[test]
    fn fields() {
        let f = Family::FireRedLeafGreen.format();
        let attr = f.layer_type.set(f.behavior.set(0, 0x1FF), 2);
        assert_eq!(attr, 0x4000_01FF);
        assert_eq!(f.layer_type.get(attr), 2);
        assert_eq!(f.behavior.max(), 0x1FF);
        let e = Family::Emerald.format();
        assert_eq!(e.layer_type.set(0x0012, 1), 0x1012);
    }

    #[test]
    fn blocks() {
        let b = block::pack(0x215, 1, 3);
        assert_eq!(b, 0x3615);
        assert_eq!((block::metatile(b), block::collision(b), block::elevation(b)), (0x215, 1, 3));
        assert_eq!(block::with_metatile(b, 7), 0x3407);
    }

    #[test]
    fn behavior_names_are_sorted() {
        for f in [Family::Emerald, Family::FireRedLeafGreen] {
            let b = f.format().behaviors;
            assert!(!b.is_empty());
            assert!(b.windows(2).all(|w| w[0].0 < w[1].0));
        }
        assert_eq!(Family::Emerald.format().behavior_name(2), Some("TALL_GRASS"));
        assert_eq!(Family::FireRedLeafGreen.format().behavior_name(2), Some("TALL_GRASS"));
    }
}
