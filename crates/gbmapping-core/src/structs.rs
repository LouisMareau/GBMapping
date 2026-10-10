//! The games' structures as they sit in the ROM (struct Tileset, MapLayout, MapHeader),
//! read strictly: an offset that isn't such a structure gives `None`. That's how the
//! tables of a ROM nobody described are found.

use crate::format::{Format, HEADER_SIZE, TILESET_SIZE};
use crate::rom::Rom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tileset {
    pub offset: usize,
    pub compressed: bool,
    pub secondary: bool,
    pub tiles: usize,
    pub palettes: usize,
    pub metatiles: usize,
    pub attributes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub offset: usize,
    pub width: usize,
    pub height: usize,
    pub border: usize,
    pub blocks: usize,
    pub primary: usize,
    pub secondary: usize,
    pub border_width: usize,
    pub border_height: usize,
}

impl Layout {
    pub fn pair(&self) -> Pair {
        Pair { primary: self.primary, secondary: self.secondary }
    }
}

/// A map's two tilesets (their offsets): what its metatile ids mean
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pair {
    pub primary: usize,
    pub secondary: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub offset: usize,
    pub layout: usize,
    pub events: Option<usize>,
    pub scripts: Option<usize>,
    pub connections: Option<usize>,
    pub music: u16,
    pub layout_id: u16,
    pub mapsec: u16,
    pub cave: u8,
    pub weather: u8,
    pub map_type: u8,
}

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub rom: &'a Rom,
    pub format: &'static Format,
    /// The header's region map section is 16 bits wide (some hacks)
    pub mapsec16: bool,
}

impl Reader<'_> {
    /// A pointer to data: in the ROM, at an even address
    fn data(&self, o: usize) -> Option<usize> {
        self.rom.ptr(o).filter(|p| p.is_multiple_of(2))
    }

    /// A pointer to a structure: in the ROM, word-aligned
    pub fn aligned(&self, o: usize) -> Option<usize> {
        self.rom.ptr(o).filter(|p| p.is_multiple_of(4))
    }

    /// A pointer or NULL; `None` when it's neither
    fn optional(&self, o: usize) -> Option<Option<usize>> {
        if self.rom.is_null(o) {
            Some(None)
        } else {
            self.rom.ptr(o).map(Some)
        }
    }

    pub fn tileset(&self, o: usize) -> Option<Tileset> {
        let (rom, f) = (self.rom, self.format);
        if !o.is_multiple_of(4) || o + TILESET_SIZE > rom.len() {
            return None;
        }
        let (compressed, secondary) = (rom.u8(o), rom.u8(o + 1));
        if compressed > 1 || secondary > 1 || rom.u16(o + 2) != 0 {
            return None;
        }
        // The animation callback: none, or Thumb code in the ROM
        let callback = rom.u32(o + f.tileset_callback);
        if callback != 0 && !(callback & 1 == 1 && rom.ptr(o + f.tileset_callback).is_some()) {
            return None;
        }
        Some(Tileset {
            offset: o,
            compressed: compressed == 1,
            secondary: secondary == 1,
            tiles: self.data(o + 4)?,
            palettes: self.data(o + 8)?,
            metatiles: self.data(o + 12)?,
            attributes: self.data(o + f.tileset_attributes)?,
        })
    }

    pub fn layout(&self, o: usize) -> Option<Layout> {
        let (rom, f) = (self.rom, self.format);
        if !o.is_multiple_of(4) || o + 24 > rom.len() {
            return None;
        }
        let (width, height) = (rom.u32(o) as usize, rom.u32(o + 4) as usize);
        if width == 0 || height == 0 || width > 1024 || height > 1024 || width * height > 0x8000 {
            return None;
        }
        let border = self.data(o + 8)?;
        let blocks = self.data(o + 12)?;
        if blocks + width * height * 2 > rom.len() {
            return None;
        }
        let primary = self.aligned(o + 16)?;
        let secondary = self.aligned(o + 20)?;
        self.tileset(primary)?;
        self.tileset(secondary)?;
        let (mut border_width, mut border_height) = (2, 2);
        if f.layout_border_size {
            let (w, h) = (rom.u8(o + 24) as usize, rom.u8(o + 25) as usize);
            if (1..=8).contains(&w) && (1..=8).contains(&h) {
                (border_width, border_height) = (w, h);
            }
        }
        if border + border_width * border_height * 2 > rom.len() {
            return None;
        }
        Some(Layout { offset: o, width, height, border, blocks, primary, secondary, border_width, border_height })
    }

    pub fn header(&self, o: usize) -> Option<Header> {
        let rom = self.rom;
        if !o.is_multiple_of(4) || o + HEADER_SIZE > rom.len() {
            return None;
        }
        let layout = self.aligned(o)?;
        let events = self.optional(o + 4)?;
        let scripts = self.optional(o + 8)?;
        let connections = self.optional(o + 12)?;
        self.layout(layout)?;
        let (mapsec, cave, weather, map_type) = if self.mapsec16 {
            (rom.u16(o + 20), rom.u8(o + 22), rom.u8(o + 23), rom.u8(o + 24))
        } else {
            (rom.u8(o + 20) as u16, rom.u8(o + 21), rom.u8(o + 22), rom.u8(o + 23))
        };
        Some(Header {
            offset: o,
            layout,
            events,
            scripts,
            connections,
            music: rom.u16(o + 16),
            layout_id: rom.u16(o + 18),
            mapsec,
            cave,
            weather,
            map_type,
        })
    }
}
