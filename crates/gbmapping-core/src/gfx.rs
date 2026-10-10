//! GBA graphics: 4bpp tiles, 15-bit palettes, metatiles drawn to RGBA pixels.

pub const TILE: usize = 8;
pub const METATILE: usize = 16;

pub type Color = [u8; 3];
pub type Palette = [Color; 16];
pub type TilePixels = [u8; TILE * TILE];

/// 15-bit BGR -> [r, g, b]
pub fn color555(v: u16) -> Color {
    let c = |x: u16| {
        let x = (x & 0x1F) as u8;
        (x << 3) | (x >> 2)
    };
    [c(v), c(v >> 5), c(v >> 10)]
}

/// `count` palettes of 16 colors from the bytes at `offset` (missing bytes read as black)
pub fn read_palettes(bytes: &[u8], offset: usize, count: usize) -> Vec<Palette> {
    (0..count)
        .map(|p| {
            let mut pal = [[0u8; 3]; 16];
            for (c, color) in pal.iter_mut().enumerate() {
                let o = offset + (p * 16 + c) * 2;
                if let Some(b) = bytes.get(o..o + 2) {
                    *color = color555(u16::from_le_bytes([b[0], b[1]]));
                }
            }
            pal
        })
        .collect()
}

/// A 4bpp tile: 32 bytes, 2 pixels per byte (low nibble first). Blank past the end of the data.
pub fn tile_pixels(data: &[u8], index: usize) -> TilePixels {
    let mut out = [0u8; TILE * TILE];
    if let Some(tile) = data.get(index * 32..index * 32 + 32) {
        for (i, b) in tile.iter().enumerate() {
            out[i * 2] = b & 0xF;
            out[i * 2 + 1] = b >> 4;
        }
    }
    out
}

/// Draws a tile (8x8) into an RGBA buffer `width` pixels wide at (x, y).
/// Color 0 is skipped when `transparent` is set.
#[allow(clippy::too_many_arguments)]
pub fn draw_tile(
    rgba: &mut [u8],
    width: usize,
    pixels: &TilePixels,
    palette: &Palette,
    x: usize,
    y: usize,
    hflip: bool,
    vflip: bool,
    transparent: bool,
) {
    for py in 0..TILE {
        for px in 0..TILE {
            let sx = if hflip { TILE - 1 - px } else { px };
            let sy = if vflip { TILE - 1 - py } else { py };
            let c = pixels[sy * TILE + sx] as usize;
            if transparent && c == 0 {
                continue;
            }
            let o = ((y + py) * width + x + px) * 4;
            if let Some(dst) = rgba.get_mut(o..o + 4) {
                dst[..3].copy_from_slice(&palette[c]);
                dst[3] = 255;
            }
        }
    }
}

/// One of a metatile's 8 tile entries: `tile | hflip << 10 | vflip << 11 | palette << 12`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileEntry {
    pub tile: u16,
    pub hflip: bool,
    pub vflip: bool,
    pub palette: u8,
}

impl TileEntry {
    pub fn unpack(e: u16) -> Self {
        Self { tile: e & 0x3FF, hflip: e & 0x400 != 0, vflip: e & 0x800 != 0, palette: (e >> 12) as u8 }
    }

    pub fn pack(self) -> u16 {
        (self.tile & 0x3FF) | (self.hflip as u16) << 10 | (self.vflip as u16) << 11 | (self.palette as u16 & 0xF) << 12
    }
}

/// A metatile (8 entries: the bottom layer's 4, then the top's) into RGBA at (x, y).
/// As on the console, a layer's color 0 is transparent and what shows behind both
/// layers is the backdrop: color 0 of palette 0.
pub fn draw_metatile(
    rgba: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    entries: &[u16; 8],
    tile_at: impl Fn(u16) -> TilePixels,
    palettes: &[Palette],
) {
    let backdrop = palettes.first().map(|p| p[0]).unwrap_or([0, 0, 0]);
    for py in 0..METATILE {
        for px in 0..METATILE {
            let o = ((y + py) * width + x + px) * 4;
            if let Some(dst) = rgba.get_mut(o..o + 4) {
                dst[..3].copy_from_slice(&backdrop);
                dst[3] = 255;
            }
        }
    }
    for (i, &e) in entries.iter().enumerate() {
        draw_entry(rgba, width, x + (i % 2) * TILE, y + (i % 4 / 2) * TILE, e, &tile_at, palettes, true);
    }
}

/// One tile entry at (x, y)
#[allow(clippy::too_many_arguments)]
pub fn draw_entry(
    rgba: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    entry: u16,
    tile_at: impl Fn(u16) -> TilePixels,
    palettes: &[Palette],
    transparent: bool,
) {
    let e = TileEntry::unpack(entry);
    const MISSING: Palette = [[255, 0, 255]; 16];
    let palette = palettes.get(e.palette as usize).unwrap_or(&MISSING);
    draw_tile(rgba, width, &tile_at(e.tile), palette, x, y, e.hflip, e.vflip, transparent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(color555(0x7FFF), [255, 255, 255]);
        assert_eq!(color555(0x001F), [255, 0, 0]);
        assert_eq!(color555(0x7C00), [0, 0, 255]);
    }

    #[test]
    fn entries_pack() {
        let e = TileEntry::unpack(0x3C05);
        assert_eq!(e, TileEntry { tile: 5, hflip: true, vflip: true, palette: 3 });
        assert_eq!(e.pack(), 0x3C05);
    }

    #[test]
    fn metatile_layers() {
        // tile 1: all color 1; tile 2: left half color 2, right half transparent
        let mut data = vec![0u8; 96];
        data[32..64].fill(0x11);
        for row in 0..8 {
            data[64 + row * 4] = 0x22;
            data[64 + row * 4 + 1] = 0x22;
        }
        let mut pal = [[0u8; 3]; 16];
        pal[0] = [9, 9, 9];
        pal[1] = [10, 0, 0];
        pal[2] = [0, 20, 0];
        let mut rgba = vec![0u8; 16 * 16 * 4];
        // bottom: tile 1 top-left only; top: tile 2 top-left, flipped top-right
        let entries = [1, 0, 0, 0, 2, 2 | 0x400, 0, 0];
        draw_metatile(&mut rgba, 16, 0, 0, &entries, |t| tile_pixels(&data, t as usize), &[pal]);
        let at = |x: usize, y: usize| [rgba[(y * 16 + x) * 4], rgba[(y * 16 + x) * 4 + 1], rgba[(y * 16 + x) * 4 + 2]];
        assert_eq!(at(0, 0), [0, 20, 0], "the top layer over the bottom one");
        assert_eq!(at(7, 0), [10, 0, 0], "the bottom layer through the top one");
        assert_eq!(at(8, 0), [9, 9, 9], "the backdrop through both");
        assert_eq!(at(15, 0), [0, 20, 0], "the flipped tile");
        assert_eq!(at(0, 8), [9, 9, 9]);
    }
}
