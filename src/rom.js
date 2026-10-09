// A ROM in memory: little-endian readers, GBA pointers, LZ77, edits tracking.
// Works in the browser and in Node (plain ES module, no DOM).
'use strict';

export const GBA_ROM_BASE = 0x08000000;

export class Rom {
  constructor(bytes, name = 'rom') {
    this.bytes = bytes;          // Uint8Array
    this.name = name;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    this.edits = new Map();      // offset -> original byte (for "modified" tracking)
  }

  get size() { return this.bytes.length; }
  u8(o) { return this.bytes[o]; }
  u16(o) { return this.view.getUint16(o, true); }
  u32(o) { return this.view.getUint32(o, true); }
  s16(o) { return this.view.getInt16(o, true); }

  // A GBA ROM pointer (0x08xxxxxx) at offset o, as a ROM offset, or -1
  ptr(o) {
    if (o < 0 || o + 4 > this.size) return -1;
    const v = this.u32(o);
    if (v < GBA_ROM_BASE || v >= GBA_ROM_BASE + this.size) return -1;
    return v - GBA_ROM_BASE;
  }

  isPtr(o) { return this.ptr(o) >= 0; }

  text(o, length) {
    let s = '';
    for (let i = 0; i < length; i++) {
      const c = this.bytes[o + i];
      if (c === 0) break;
      s += String.fromCharCode(c);
    }
    return s;
  }

  // GBA header
  header() {
    return { title: this.text(0xA0, 12), code: this.text(0xAC, 4), maker: this.text(0xB0, 2), version: this.u8(0xBC) };
  }

  write8(o, v) {
    if (!this.edits.has(o)) this.edits.set(o, this.bytes[o]);
    this.bytes[o] = v & 0xFF;
  }

  write16(o, v) {
    this.write8(o, v & 0xFF);
    this.write8(o + 1, (v >> 8) & 0xFF);
  }

  get modified() { return this.edits.size > 0; }

  // GBA BIOS LZ77 (type 0x10): the decompressed bytes, or null if it isn't LZ77 data
  lz77(o) {
    if (o < 0 || o + 4 > this.size || this.bytes[o] !== 0x10) return null;
    const size = this.u32(o) >>> 8;
    if (size === 0 || size > 0x200000) return null;
    const out = new Uint8Array(size);
    let src = o + 4, dst = 0;
    while (dst < size) {
      if (src >= this.size) return null;
      const flags = this.bytes[src++];
      for (let bit = 7; bit >= 0 && dst < size; bit--) {
        if (flags & (1 << bit)) {
          if (src + 2 > this.size) return null;
          const b1 = this.bytes[src], b2 = this.bytes[src + 1];
          src += 2;
          const length = (b1 >> 4) + 3, disp = ((b1 & 0xF) << 8 | b2) + 1;
          if (disp > dst) return null;
          for (let i = 0; i < length && dst < size; i++) { out[dst] = out[dst - disp]; dst++; }
        } else {
          out[dst++] = this.bytes[src++];
        }
      }
    }
    return out;
  }

  // A copy of the bytes, for saving
  export() { return this.bytes.slice(); }
}

export const hex = (n, width = 6) => (n >>> 0).toString(16).toUpperCase().padStart(width, '0');
