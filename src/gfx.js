// GBA graphics: 4bpp tiles, 15-bit palettes, metatiles drawn to RGBA pixels.
'use strict';

export const TILE = 8;
export const METATILE = 16;

// 15-bit BGR -> [r, g, b]
export function color555(v) {
  const r = v & 0x1F, g = (v >> 5) & 0x1F, b = (v >> 10) & 0x1F;
  return [(r << 3) | (r >> 2), (g << 3) | (g >> 2), (b << 3) | (b >> 2)];
}

// 16 palettes of 16 colors from 512 bytes
export function readPalettes(bytes, offset, count = 16) {
  const pals = [];
  for (let p = 0; p < count; p++) {
    const pal = [];
    for (let c = 0; c < 16; c++) {
      const o = offset + (p * 16 + c) * 2;
      pal.push(color555(bytes[o] | (bytes[o + 1] << 8)));
    }
    pals.push(pal);
  }
  return pals;
}

// 4bpp tiles: each tile 32 bytes, 2 pixels per byte (low nibble first)
export function tilePixels(data, index) {
  const out = new Uint8Array(64);
  const base = index * 32;
  if (base + 32 > data.length) return out;
  for (let i = 0; i < 32; i++) {
    const b = data[base + i];
    out[i * 2] = b & 0xF;
    out[i * 2 + 1] = b >> 4;
  }
  return out;
}

// Draws a tile (8x8) into an RGBA buffer of the given width at (x, y); color 0 is transparent
// when `transparent` is set
export function drawTile(rgba, width, pixels, palette, x, y, hflip, vflip, transparent) {
  for (let py = 0; py < TILE; py++) {
    for (let px = 0; px < TILE; px++) {
      const sx = hflip ? TILE - 1 - px : px, sy = vflip ? TILE - 1 - py : py;
      const c = pixels[sy * TILE + sx];
      if (transparent && c === 0) continue;
      const col = palette ? palette[c] : [255, 0, 255];
      const o = ((y + py) * width + (x + px)) * 4;
      rgba[o] = col[0]; rgba[o + 1] = col[1]; rgba[o + 2] = col[2]; rgba[o + 3] = 255;
    }
  }
}

// A metatile (8 entries: bottom 4 then top 4, each tile | hflip<<10 | vflip<<11 | palette<<12) into RGBA 16x16
export function drawMetatile(rgba, width, x, y, entries, tileAt, palettes) {
  for (let layer = 0; layer < 2; layer++) {
    for (let i = 0; i < 4; i++) {
      const e = entries[layer * 4 + i];
      if (e === undefined) continue;
      const pixels = tileAt(e & 0x3FF);
      if (!pixels) continue;
      drawTile(rgba, width, pixels, palettes[e >> 12], x + (i % 2) * TILE, y + Math.floor(i / 2) * TILE,
        (e >> 10) & 1, (e >> 11) & 1, layer === 1);
    }
  }
}
