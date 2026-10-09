// Pokémon Emerald (and hacks built from pokeemerald): the map groups table,
// map headers, layouts, tilesets, metatiles and attributes, read from the ROM.
//
// The game interface every profile implements:
//   static matches(header)           -> does this profile fit the ROM?
//   constructor(rom, profile)         profile: optional overrides (addresses, names, header variant)
//   maps()                            -> [{id, group, num, name, headerOffset}]
//   loadMap(id)                       -> {id, width, height, blocks (Uint16Array), border (Uint16Array), pair}
//   writeBlocks(map) / writeBorder(map)   into the ROM
//   pair.metatileCount, pair.image(id) (ImageData-like rgba 16x16), pair.entries(id), pair.attribute(id)
//   pair.setEntries(id, entries), pair.setAttribute(id, value), pair.tilePixels(tileId), pair.palettes
//   blockMetatile/Collision/Elevation helpers are the GBA ones: metatile | collision<<10 | elevation<<12
'use strict';

import { hex } from '../rom.js';
import { readPalettes, tilePixels, drawMetatile, METATILE } from '../gfx.js';

const NUM_PALS_IN_PRIMARY = 6;
const NUM_METATILES_IN_PRIMARY = 512;
const NUM_TILES_IN_PRIMARY = 512;

// Vanilla ROMs: where the map groups table and the region map names are
const VANILLA = {
  BPEE: { gMapGroups: 0x486578, gRegionMapEntries: 0x5A1C74, mapsec16: false, name: 'Pokémon Emerald (USA, Europe)' },
};

export class Tileset {
  constructor(rom, offset) {
    this.rom = rom;
    this.offset = offset;
    this.isCompressed = rom.u8(offset) !== 0;
    this.isSecondary = rom.u8(offset + 1) !== 0;
    this.tilesPtr = rom.ptr(offset + 4);
    this.palettesPtr = rom.ptr(offset + 8);
    this.metatilesPtr = rom.ptr(offset + 12);
    this.attributesPtr = rom.ptr(offset + 16);
    this.valid = this.tilesPtr >= 0 && this.palettesPtr >= 0 && this.metatilesPtr >= 0 && this.attributesPtr >= 0;
    this.tiles = null;
    this.tileCache = new Map();
  }

  // The tile graphics (4bpp), decompressed if needed
  tileData() {
    if (this.tiles) return this.tiles;
    if (this.isCompressed) this.tiles = this.rom.lz77(this.tilesPtr) || new Uint8Array(0);
    else this.tiles = this.rom.bytes.subarray(this.tilesPtr, this.tilesPtr + NUM_TILES_IN_PRIMARY * 32);
    return this.tiles;
  }

  tile(index) {
    if (!this.tileCache.has(index)) this.tileCache.set(index, tilePixels(this.tileData(), index));
    return this.tileCache.get(index);
  }

  tileCount() { return Math.floor(this.tileData().length / 32); }

  // Palettes: a primary gives 0-5, a secondary 6-12 (all 16 are stored)
  palettes() { return readPalettes(this.rom.bytes, this.palettesPtr, 16); }

  // Metatile i's 8 entries (i local to this tileset)
  entries(i) {
    const o = this.metatilesPtr + i * 16;
    const out = [];
    for (let k = 0; k < 8; k++) out.push(this.rom.u16(o + k * 2));
    return out;
  }

  setEntries(i, entries) {
    const o = this.metatilesPtr + i * 16;
    for (let k = 0; k < 8; k++) this.rom.write16(o + k * 2, entries[k]);
  }

  attribute(i) { return this.rom.u16(this.attributesPtr + i * 2); }
  setAttribute(i, v) { this.rom.write16(this.attributesPtr + i * 2, v); }

  // How many metatiles the tileset holds: the metatiles data runs until the attributes (or 512)
  metatileCount() {
    let n = NUM_METATILES_IN_PRIMARY;
    if (this.attributesPtr > this.metatilesPtr) n = Math.min(n, Math.floor((this.attributesPtr - this.metatilesPtr) / 16));
    return Math.max(0, n);
  }
}

// A primary + secondary tileset pair: metatile images and lookups across both
export class TilesetPair {
  constructor(primary, secondary) {
    this.primary = primary;
    this.secondary = secondary;
    this.key = hex(primary.offset) + '_' + hex(secondary.offset);
    const pp = primary.palettes(), sp = secondary.palettes();
    this.palettes = pp.slice(0, NUM_PALS_IN_PRIMARY).concat(sp.slice(NUM_PALS_IN_PRIMARY, 16));
    this.imageCache = new Map();
    this.metatileCount = NUM_METATILES_IN_PRIMARY + secondary.metatileCount();
  }

  which(id) { return id < NUM_METATILES_IN_PRIMARY ? [this.primary, id] : [this.secondary, id - NUM_METATILES_IN_PRIMARY]; }

  tilePixels(tileId) {
    return tileId < NUM_TILES_IN_PRIMARY ? this.primary.tile(tileId) : this.secondary.tile(tileId - NUM_TILES_IN_PRIMARY);
  }

  entries(id) { const [t, i] = this.which(id); return t.entries(i); }
  setEntries(id, entries) { const [t, i] = this.which(id); t.setEntries(i, entries); this.imageCache.delete(id); }
  attribute(id) { const [t, i] = this.which(id); return t.attribute(i); }
  setAttribute(id, v) { const [t, i] = this.which(id); t.setAttribute(i, v); }
  behavior(id) { return this.attribute(id) & 0xFF; }

  // The metatile as 16x16 RGBA bytes (cached)
  image(id) {
    if (!this.imageCache.has(id)) {
      const rgba = new Uint8ClampedArray(METATILE * METATILE * 4);
      drawMetatile(rgba, METATILE, 0, 0, this.entries(id), (t) => this.tilePixels(t), this.palettes);
      this.imageCache.set(id, rgba);
    }
    return this.imageCache.get(id);
  }

  invalidate() { this.imageCache.clear(); }
}

export class PokeemeraldGame {
  static matches(header) { return header.code in VANILLA || header.code.startsWith('BPE'); }

  constructor(rom, profile = {}) {
    this.rom = rom;
    const code = rom.header().code;
    const base = VANILLA[code] || VANILLA.BPEE;
    this.profile = { ...base, ...profile };
    this.mapsec16 = !!this.profile.mapsec16;
    this.tilesets = new Map();
    this.pairs = new Map();
    this.regionNames = this.profile.regionNames || null;
    this._maps = null;
  }

  get name() { return this.profile.name || 'pokeemerald ROM'; }

  // -- tables ----------------------------------------------------------------

  tileset(offset) {
    if (!this.tilesets.has(offset)) this.tilesets.set(offset, new Tileset(this.rom, offset));
    return this.tilesets.get(offset);
  }

  pair(primaryOffset, secondaryOffset) {
    const key = primaryOffset + ':' + secondaryOffset;
    if (!this.pairs.has(key)) this.pairs.set(key, new TilesetPair(this.tileset(primaryOffset), this.tileset(secondaryOffset)));
    return this.pairs.get(key);
  }

  layout(offset) {
    const rom = this.rom;
    const width = rom.u32(offset), height = rom.u32(offset + 4);
    const l = { offset, width, height, borderPtr: rom.ptr(offset + 8), blocksPtr: rom.ptr(offset + 12),
      primaryPtr: rom.ptr(offset + 16), secondaryPtr: rom.ptr(offset + 20) };
    l.valid = width > 0 && width <= 1000 && height > 0 && height <= 1000 && l.borderPtr >= 0 && l.blocksPtr >= 0 && l.primaryPtr >= 0 && l.secondaryPtr >= 0
      && l.blocksPtr + width * height * 2 <= rom.size;
    return l;
  }

  // The map header at an offset (pokeemerald's struct MapHeader)
  header(offset) {
    const rom = this.rom;
    const h = { offset, layoutPtr: rom.ptr(offset), eventsPtr: rom.ptr(offset + 4), scriptsPtr: rom.ptr(offset + 8), connectionsPtr: rom.ptr(offset + 12),
      music: rom.u16(offset + 16), layoutId: rom.u16(offset + 18) };
    if (this.mapsec16) {
      h.mapsec = rom.u16(offset + 20); h.cave = rom.u8(offset + 22); h.weather = rom.u8(offset + 23); h.mapType = rom.u8(offset + 24);
    } else {
      h.mapsec = rom.u8(offset + 20); h.cave = rom.u8(offset + 21); h.weather = rom.u8(offset + 22); h.mapType = rom.u8(offset + 23);
    }
    h.valid = h.layoutPtr >= 0 && this.layout(h.layoutPtr).valid;
    return h;
  }

  // The groups' header-pointer arrays, from the map groups table (the counts aren't in the ROM:
  // a group's array runs until the next group's array, or until the pointers stop being map headers)
  groups() {
    if (this._groups) return this._groups;
    const rom = this.rom;
    const table = this.profile.gMapGroups;
    const starts = [];
    for (let o = table; ; o += 4) {
      const p = rom.ptr(o);
      if (p < 0 || starts.includes(p) || starts.length >= 256) break;
      // a group array must start with a header pointer
      const first = rom.ptr(p);
      if (first < 0 || !this.header(first).valid) break;
      starts.push(p);
    }
    const sorted = starts.slice().sort((a, b) => a - b);
    const groups = starts.map((start, gi) => {
      const next = sorted[sorted.indexOf(start) + 1];
      const limit = next !== undefined ? next : start + 4 * 256;
      const headers = [];
      const counts = this.profile.groups && this.profile.groups[gi] ? this.profile.groups[gi].maps.length : null;
      for (let o = start; o < limit && headers.length < 256; o += 4) {
        if (counts !== null && headers.length >= counts) break;
        const p = rom.ptr(o);
        if (p < 0 || !this.header(p).valid) break;
        headers.push(p);
      }
      return { index: gi, start, headers };
    });
    this._groups = groups;
    return groups;
  }

  // Every map: id "group.num", a name (the profile's, else the region section's name, else the ids)
  maps() {
    if (this._maps) return this._maps;
    const out = [];
    for (const g of this.groups()) {
      g.headers.forEach((offset, num) => {
        const h = this.header(offset);
        const profileName = this.profile.groups && this.profile.groups[g.index] && this.profile.groups[g.index].maps[num];
        const name = (profileName && (profileName.name || profileName)) || this.sectionName(h.mapsec) || `map ${g.index}.${num}`;
        out.push({ id: `${g.index}.${num}`, group: g.index, num, name, headerOffset: offset, mapsec: h.mapsec, mapType: h.mapType,
          groupName: this.profile.groups && this.profile.groups[g.index] ? this.profile.groups[g.index].name : `group ${g.index}` });
      });
    }
    this._maps = out;
    return out;
  }

  // The region map entries table (x, y, width, height, name pointer; 8 bytes each): the profile's
  // address when it holds a pointer to "LITTLEROOT TOWN" as entry 0, else found by that string
  regionEntries() {
    if (this._regionEntries !== undefined) return this._regionEntries;
    const rom = this.rom;
    const isTable = (o) => { const p = rom.ptr(o + 4); return p >= 0 && decodeText(rom, p, 16) === 'LITTLEROOT TOWN'; };
    let table = this.profile.gRegionMapEntries;
    if (!(table && isTable(table))) {
      table = null;
      const needle = encodeText('LITTLEROOT TOWN');
      for (let o = 0; o + needle.length <= rom.size && table === null; o++) {
        let k = 0;
        while (k < needle.length && rom.bytes[o + k] === needle[k]) k++;
        if (k < needle.length || rom.bytes[o + k] !== 0xFF) continue;
        // the pointer to it, aligned
        const want = (o + 0x08000000) >>> 0;
        for (let q = 0; q + 4 <= rom.size; q += 4) {
          if (rom.u32(q) === want && isTable(q - 4)) { table = q - 4; break; }
        }
      }
    }
    this._regionEntries = table;
    return table;
  }

  // The region map section's name
  sectionName(mapsec) {
    if (this.regionNames) return this.regionNames[mapsec] || null;
    const table = this.regionEntries();
    if (table === null) return null;
    const namePtr = this.rom.ptr(table + mapsec * 8 + 4);
    if (namePtr < 0) return null;
    return decodeText(this.rom, namePtr, 24) || null;
  }

  // -- maps -------------------------------------------------------------------

  loadMap(id) {
    const m = this.maps().find((x) => x.id === id);
    if (!m) throw new Error('no map ' + id);
    const h = this.header(m.headerOffset);
    const l = this.layout(h.layoutPtr);
    const rom = this.rom;
    const blocks = new Uint16Array(l.width * l.height);
    for (let i = 0; i < blocks.length; i++) blocks[i] = rom.u16(l.blocksPtr + i * 2);
    const border = new Uint16Array(4);
    for (let i = 0; i < 4; i++) border[i] = rom.u16(l.borderPtr + i * 2);
    return { ...m, width: l.width, height: l.height, blocks, border, layout: l, header: h, pair: this.pair(l.primaryPtr, l.secondaryPtr) };
  }

  writeBlocks(map) {
    for (let i = 0; i < map.blocks.length; i++) {
      const o = map.layout.blocksPtr + i * 2;
      if (this.rom.u16(o) !== map.blocks[i]) this.rom.write16(o, map.blocks[i]);
    }
  }

  writeBorder(map) {
    for (let i = 0; i < 4; i++) {
      const o = map.layout.borderPtr + i * 2;
      if (this.rom.u16(o) !== map.border[i]) this.rom.write16(o, map.border[i]);
    }
  }
}

// Emerald's text: the usual characters (enough for map names)
const CHARS = {};
(() => {
  const set = (start, s) => { for (let i = 0; i < s.length; i++) CHARS[start + i] = s[i]; };
  set(0x00, ' ');
  set(0xA1, '0123456789!?.-');
  CHARS[0xAD] = '.'; CHARS[0xAE] = '-'; CHARS[0xB0] = '…'; CHARS[0xB1] = '“'; CHARS[0xB2] = '”'; CHARS[0xB3] = '‘'; CHARS[0xB4] = '’';
  CHARS[0xB5] = '♂'; CHARS[0xB6] = '♀'; CHARS[0xB7] = '$'; CHARS[0xB8] = ','; CHARS[0xB9] = '×'; CHARS[0xBA] = '/';
  set(0xBB, 'ABCDEFGHIJKLMNOPQRSTUVWXYZ');
  set(0xD5, 'abcdefghijklmnopqrstuvwxyz');
  CHARS[0x1B] = 'é';
})();

export function encodeText(s) {
  const inv = {};
  for (const [k, v] of Object.entries(CHARS)) if (inv[v] === undefined) inv[v] = Number(k);
  return Uint8Array.from(s, (ch) => inv[ch] !== undefined ? inv[ch] : 0);
}

export function decodeText(rom, offset, max = 32) {
  let s = '';
  for (let i = 0; i < max; i++) {
    const c = rom.u8(offset + i);
    if (c === 0xFF) break;
    s += CHARS[c] !== undefined ? CHARS[c] : '?';
  }
  return s;
}
