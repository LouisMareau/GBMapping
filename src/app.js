// The editor: a ROM dropped in, its maps painted with metatiles (collision and
// elevation too), its metatiles edited (tiles, flips, palettes, behavior), the
// ROM saved, the edits exchanged as .json. Everything stays in the browser.
'use strict';

import { Rom, hex } from './rom.js';
import { detectGame } from './games/index.js';
import { METATILE, TILE } from './gfx.js';

const SHEET_COLUMNS = 16;
const $ = (id) => document.getElementById(id);
const canvas = $('map'), ctx = canvas.getContext('2d');
const picker = $('picker-canvas'), pctx = picker.getContext('2d');
const borderCanvas = $('border-canvas'), bctx = borderCanvas.getContext('2d');
const tilesCanvas = $('tiles-canvas'), tctx = tilesCanvas.getContext('2d');

const state = {
  rom: null, original: null, game: null, profile: {},
  maps: [], map: null, sheets: new Map(),   // pair key -> {canvas, rows}
  selection: { x: 0, y: 0, w: 1, h: 1 }, zoom: 2,
  painting: false, lastPainted: null, collisionValue: 1,
  undo: [], redo: [], touchedMaps: new Set(), touchedTilesets: new Set(),
  entry: { layer: 0, quadrant: 0 },
};

// ---- loading ----------------------------------------------------------------

async function loadFiles(files) {
  let romFile = null;
  for (const f of files) {
    if (f.name.endsWith('.json')) {
      try { state.profile = JSON.parse(await f.text()); } catch (e) { alert('Not a profile: ' + f.name); }
    } else {
      romFile = f;
    }
  }
  if (romFile) {
    const bytes = new Uint8Array(await romFile.arrayBuffer());
    openRom(bytes, romFile.name);
  } else if (state.rom && state.profile) {
    openRom(state.original.slice(), state.rom.name);
  }
}

function openRom(bytes, name) {
  const rom = new Rom(bytes, name);
  const game = detectGame(rom, state.profile);
  if (!game) { alert('No profile for this ROM (' + JSON.stringify(rom.header()) + ')'); return; }
  state.rom = rom; state.original = bytes.slice(); state.game = game;
  state.maps = game.maps();
  state.map = null; state.sheets.clear(); state.undo = []; state.redo = []; state.touchedMaps.clear(); state.touchedTilesets.clear();
  const h = rom.header();
  $('rom-info').textContent = `${name} — ${h.title} (${h.code}) — ${game.name} — ${state.maps.length} maps`;
  renderList();
  const saved = localStorage.getItem(storageKey());
  if (saved) {
    try {
      const edits = JSON.parse(saved);
      const n = Object.keys(edits.maps || {}).length + Object.keys(edits.tilesets || {}).length;
      if (n && confirm(`Edits for this ROM are kept in this browser (${n} map(s)/tileset(s)). Apply them?`)) applyEdits(edits);
    } catch (e) { /* ignore */ }
  }
  if (state.maps.length) openMap(state.maps[0].id);
}

function storageKey() {
  const h = state.rom.header();
  return 'GBMapping.' + h.code + '.' + h.title + '.' + state.rom.size;
}

// ---- the list ---------------------------------------------------------------

function renderList() {
  const list = $('map-list');
  const q = $('search').value.trim().toLowerCase();
  list.innerHTML = '';
  let lastGroup = null;
  for (const m of state.maps) {
    if (q && !(m.name + ' ' + m.id + ' ' + m.groupName).toLowerCase().includes(q)) continue;
    if (m.groupName !== lastGroup) {
      const g = document.createElement('li');
      g.className = 'group'; g.textContent = m.groupName;
      list.appendChild(g);
      lastGroup = m.groupName;
    }
    const li = document.createElement('li');
    li.textContent = m.name;
    const small = document.createElement('small');
    small.textContent = m.id;
    li.appendChild(small);
    if (state.map && state.map.id === m.id) li.classList.add('active');
    if (state.touchedMaps.has(m.id)) li.classList.add('edited');
    li.onclick = () => openMap(m.id);
    list.appendChild(li);
  }
}

// ---- sheets -------------------------------------------------------------------

function sheetFor(pair) {
  if (!state.sheets.has(pair.key)) {
    const rows = Math.ceil(pair.metatileCount / SHEET_COLUMNS);
    const c = document.createElement('canvas');
    c.width = SHEET_COLUMNS * METATILE; c.height = Math.max(1, rows) * METATILE;
    const cx = c.getContext('2d');
    const img = cx.createImageData(c.width, c.height);
    for (let id = 0; id < pair.metatileCount; id++) {
      const px = pair.image(id);
      const ox = (id % SHEET_COLUMNS) * METATILE, oy = Math.floor(id / SHEET_COLUMNS) * METATILE;
      for (let y = 0; y < METATILE; y++) {
        img.data.set(px.subarray(y * METATILE * 4, (y + 1) * METATILE * 4), ((oy + y) * c.width + ox) * 4);
      }
    }
    cx.putImageData(img, 0, 0);
    state.sheets.set(pair.key, { canvas: c, rows });
  }
  return state.sheets.get(pair.key);
}

function refreshSheet(pair) {
  pair.invalidate();
  state.sheets.delete(pair.key);
  sheetFor(pair);
}

// ---- drawing ------------------------------------------------------------------

function openMap(id) {
  state.map = state.game.loadMap(id);
  state.undo = []; state.redo = [];
  sizeCanvas();
  renderList();
  drawMap(); drawPicker(); drawBorder(); drawMetatileEditor();
  $('status').textContent = `${state.map.name} (${id}) ${state.map.width}×${state.map.height} — tilesets ${hex(state.map.pair.primary.offset)} + ${hex(state.map.pair.secondary.offset)}`;
}

function sizeCanvas() {
  const m = state.map, z = state.zoom;
  canvas.width = m.width * METATILE; canvas.height = m.height * METATILE;
  canvas.style.width = (canvas.width * z) + 'px'; canvas.style.height = (canvas.height * z) + 'px';
}

function drawMetatile(target, pair, id, x, y) {
  const sheet = sheetFor(pair);
  if (id >= pair.metatileCount) { target.fillStyle = '#f0f'; target.fillRect(x, y, METATILE, METATILE); return; }
  target.drawImage(sheet.canvas, (id % SHEET_COLUMNS) * METATILE, Math.floor(id / SHEET_COLUMNS) * METATILE, METATILE, METATILE, x, y, METATILE, METATILE);
}

function drawMap() {
  const m = state.map;
  if (!m) return;
  ctx.imageSmoothingEnabled = false;
  for (let y = 0; y < m.height; y++) for (let x = 0; x < m.width; x++) drawMetatile(ctx, m.pair, m.blocks[y * m.width + x] & 0x3FF, x * METATILE, y * METATILE);
  const showCollision = $('show-collision').checked, showElevation = $('show-elevation').checked, showGrid = $('show-grid').checked;
  if (showCollision || showElevation) {
    ctx.font = '8px monospace';
    for (let y = 0; y < m.height; y++) {
      for (let x = 0; x < m.width; x++) {
        const block = m.blocks[y * m.width + x];
        if (showCollision && ((block >> 10) & 3)) { ctx.fillStyle = 'rgba(229, 83, 61, 0.45)'; ctx.fillRect(x * METATILE, y * METATILE, METATILE, METATILE); }
        if (showElevation) {
          const e = block >> 12;
          ctx.fillStyle = 'rgba(0,0,0,0.6)'; ctx.fillRect(x * METATILE, y * METATILE, 9, 9);
          ctx.fillStyle = e === 0 ? '#ff0' : '#fff'; ctx.fillText(e.toString(16).toUpperCase(), x * METATILE + 1, y * METATILE + 8);
        }
      }
    }
  }
  if (showGrid) {
    ctx.strokeStyle = 'rgba(255,255,255,0.25)'; ctx.lineWidth = 1; ctx.beginPath();
    for (let x = 0; x <= m.width; x++) { ctx.moveTo(x * METATILE + 0.5, 0); ctx.lineTo(x * METATILE + 0.5, m.height * METATILE); }
    for (let y = 0; y <= m.height; y++) { ctx.moveTo(0, y * METATILE + 0.5); ctx.lineTo(m.width * METATILE, y * METATILE + 0.5); }
    ctx.stroke();
  }
}

function drawPicker() {
  const m = state.map;
  if (!m) return;
  const sheet = sheetFor(m.pair);
  picker.width = sheet.canvas.width; picker.height = sheet.canvas.height;
  picker.style.width = (picker.width * 2) + 'px'; picker.style.height = (picker.height * 2) + 'px';
  pctx.imageSmoothingEnabled = false;
  pctx.fillStyle = '#111'; pctx.fillRect(0, 0, picker.width, picker.height);
  pctx.drawImage(sheet.canvas, 0, 0);
  const s = state.selection;
  pctx.strokeStyle = '#4caf50'; pctx.lineWidth = 2;
  pctx.strokeRect(s.x * METATILE + 1, s.y * METATILE + 1, s.w * METATILE - 2, s.h * METATILE - 2);
}

function drawBorder() {
  const m = state.map;
  if (!m) return;
  bctx.imageSmoothingEnabled = false;
  for (let i = 0; i < 4; i++) drawMetatile(bctx, m.pair, m.border[i] & 0x3FF, (i % 2) * METATILE, Math.floor(i / 2) * METATILE);
}

// ---- editing -------------------------------------------------------------------

function snapshot() {
  state.undo.push({ blocks: state.map.blocks.slice(), border: state.map.border.slice() });
  if (state.undo.length > 100) state.undo.shift();
  state.redo = [];
}

function commitMap() {
  state.game.writeBlocks(state.map); state.game.writeBorder(state.map);
  state.touchedMaps.add(state.map.id);
  renderList(); autosave();
}

function undo() {
  const s = state.undo.pop();
  if (!s) return;
  state.redo.push({ blocks: state.map.blocks.slice(), border: state.map.border.slice() });
  state.map.blocks = s.blocks; state.map.border = s.border;
  drawMap(); drawBorder(); commitMap();
}

function redo() {
  const s = state.redo.pop();
  if (!s) return;
  state.undo.push({ blocks: state.map.blocks.slice(), border: state.map.border.slice() });
  state.map.blocks = s.blocks; state.map.border = s.border;
  drawMap(); drawBorder(); commitMap();
}

function canvasCell(ev) {
  const r = canvas.getBoundingClientRect();
  const x = Math.floor((ev.clientX - r.left) / (METATILE * state.zoom)), y = Math.floor((ev.clientY - r.top) / (METATILE * state.zoom));
  if (x < 0 || y < 0 || x >= state.map.width || y >= state.map.height) return null;
  return { x, y };
}

function applyTool(cell) {
  const m = state.map, tool = $('tool').value, i = cell.y * m.width + cell.x;
  if (tool === 'paint') {
    const s = state.selection;
    for (let dy = 0; dy < s.h; dy++) for (let dx = 0; dx < s.w; dx++) {
      const x = cell.x + dx, y = cell.y + dy;
      if (x >= m.width || y >= m.height) continue;
      const id = (s.y + dy) * SHEET_COLUMNS + (s.x + dx);
      if (id >= m.pair.metatileCount) continue;
      const j = y * m.width + x;
      m.blocks[j] = (m.blocks[j] & 0xFC00) | id;
    }
  } else if (tool === 'collision') {
    if (state.lastPainted === null) state.collisionValue = ((m.blocks[i] >> 10) & 3) ? 0 : 1;
    m.blocks[i] = (m.blocks[i] & ~0xC00) | (state.collisionValue << 10);
  } else if (tool === 'elevation') {
    const e = Math.max(0, Math.min(15, parseInt($('elevation').value, 10) || 0));
    m.blocks[i] = (m.blocks[i] & 0x0FFF) | (e << 12);
  }
}

function pick(cell) {
  const id = state.map.blocks[cell.y * state.map.width + cell.x] & 0x3FF;
  state.selection = { x: id % SHEET_COLUMNS, y: Math.floor(id / SHEET_COLUMNS), w: 1, h: 1 };
  drawPicker(); drawMetatileEditor();
  $('picker-scroll').scrollTop = Math.max(0, state.selection.y * METATILE * 2 - 100);
}

function status(cell) {
  if (!cell) return;
  const block = state.map.blocks[cell.y * state.map.width + cell.x], id = block & 0x3FF;
  $('status').textContent = `(${cell.x}, ${cell.y})  metatile ${id} (0x${id.toString(16).toUpperCase()})  behavior ${state.map.pair.behavior(id)}  ` +
    `${((block >> 10) & 3) ? 'impassable' : 'passable'}  elevation ${block >> 12}`;
}

canvas.addEventListener('contextmenu', (ev) => ev.preventDefault());
canvas.addEventListener('mousedown', (ev) => {
  if (!state.map) return;
  const cell = canvasCell(ev);
  if (!cell) return;
  if (ev.button === 2) { pick(cell); return; }
  snapshot();
  state.painting = true; state.lastPainted = null;
  applyTool(cell); state.lastPainted = cell.x + ',' + cell.y;
  drawMap();
});
canvas.addEventListener('mousemove', (ev) => {
  if (!state.map) return;
  const cell = canvasCell(ev);
  status(cell);
  if (!state.painting || !cell) return;
  const key = cell.x + ',' + cell.y;
  if (key === state.lastPainted) return;
  applyTool(cell); state.lastPainted = key;
  drawMap();
});
window.addEventListener('mouseup', () => { if (state.painting) { state.painting = false; state.lastPainted = null; commitMap(); } });

let pickStart = null;
function pickerCell(ev) {
  const r = picker.getBoundingClientRect();
  const rows = sheetFor(state.map.pair).rows;
  return { x: Math.max(0, Math.min(SHEET_COLUMNS - 1, Math.floor((ev.clientX - r.left) / (METATILE * 2)))), y: Math.max(0, Math.min(rows - 1, Math.floor((ev.clientY - r.top) / (METATILE * 2)))) };
}
picker.addEventListener('mousedown', (ev) => { if (!state.map) return; pickStart = pickerCell(ev); state.selection = { ...pickStart, w: 1, h: 1 }; drawPicker(); drawMetatileEditor(); });
picker.addEventListener('mousemove', (ev) => {
  if (!state.map) return;
  const c = pickerCell(ev), id = c.y * SHEET_COLUMNS + c.x;
  $('picker-info').textContent = `metatile ${id} (0x${id.toString(16).toUpperCase()})  behavior ${id < state.map.pair.metatileCount ? state.map.pair.behavior(id) : '-'}`;
  if (!pickStart) return;
  state.selection = { x: Math.min(pickStart.x, c.x), y: Math.min(pickStart.y, c.y), w: Math.abs(c.x - pickStart.x) + 1, h: Math.abs(c.y - pickStart.y) + 1 };
  drawPicker();
});
window.addEventListener('mouseup', () => { pickStart = null; });

borderCanvas.addEventListener('mousedown', (ev) => {
  if (!state.map) return;
  const r = borderCanvas.getBoundingClientRect();
  const x = Math.floor((ev.clientX - r.left) / 32), y = Math.floor((ev.clientY - r.top) / 32);
  snapshot();
  const id = state.selection.y * SHEET_COLUMNS + state.selection.x;
  state.map.border[y * 2 + x] = (state.map.border[y * 2 + x] & 0xFC00) | id;
  drawBorder(); commitMap();
});

// ---- the metatile editor -----------------------------------------------------------

function selectedMetatile() { return state.selection.y * SHEET_COLUMNS + state.selection.x; }

function drawMetatileEditor() {
  const m = state.map;
  if (!m) return;
  const id = selectedMetatile(), pair = m.pair;
  if (id >= pair.metatileCount) { $('mt-title').textContent = 'Metatile ' + id + ' (none)'; return; }
  $('mt-title').textContent = `Metatile ${id} (0x${id.toString(16).toUpperCase()}) of ${id < 512 ? 'the primary' : 'the secondary'} tileset`;
  const entries = pair.entries(id);
  const pre = $('mt-preview').getContext('2d');
  pre.imageSmoothingEnabled = false;
  pre.drawImage(sheetFor(pair).canvas, (id % SHEET_COLUMNS) * METATILE, Math.floor(id / SHEET_COLUMNS) * METATILE, METATILE, METATILE, 0, 0, 64, 64);
  for (const [layer, cid] of [[0, 'mt-bottom'], [1, 'mt-top']]) {
    const c = $(cid), cx = c.getContext('2d');
    cx.imageSmoothingEnabled = false;
    cx.fillStyle = '#111'; cx.fillRect(0, 0, 64, 64);
    const img = cx.createImageData(16, 16);
    for (let q = 0; q < 4; q++) {
      const e = entries[layer * 4 + q];
      const pixels = pair.tilePixels(e & 0x3FF), pal = pair.palettes[e >> 12];
      for (let py = 0; py < TILE; py++) for (let px = 0; px < TILE; px++) {
        const sx = (e >> 10) & 1 ? TILE - 1 - px : px, sy = (e >> 11) & 1 ? TILE - 1 - py : py;
        const col = pal[pixels[sy * TILE + sx]];
        const o = (((q >> 1) * TILE + py) * 16 + (q & 1) * TILE + px) * 4;
        img.data[o] = col[0]; img.data[o + 1] = col[1]; img.data[o + 2] = col[2]; img.data[o + 3] = 255;
      }
    }
    const tmp = document.createElement('canvas'); tmp.width = 16; tmp.height = 16; tmp.getContext('2d').putImageData(img, 0, 0);
    cx.drawImage(tmp, 0, 0, 64, 64);
    if (state.entry.layer === layer) {
      cx.strokeStyle = '#4caf50'; cx.lineWidth = 2;
      cx.strokeRect((state.entry.quadrant & 1) * 32 + 1, (state.entry.quadrant >> 1) * 32 + 1, 30, 30);
    }
  }
  const e = entries[state.entry.layer * 4 + state.entry.quadrant];
  $('mt-entry').textContent = `${state.entry.layer ? 'Top' : 'Bottom'} layer, quadrant ${state.entry.quadrant}: tile ${e & 0x3FF}, palette ${e >> 12}` + (((e >> 10) & 3) ? ', flipped' : '');
  $('mt-tile').value = e & 0x3FF; $('mt-hflip').checked = !!((e >> 10) & 1); $('mt-vflip').checked = !!((e >> 11) & 1); $('mt-pal').value = e >> 12;
  const attr = pair.attribute(id);
  $('mt-behavior').value = attr & 0xFF; $('mt-layer').value = (attr >> 12) & 3;
  drawTiles(e >> 12);
}

function drawTiles(palIndex) {
  const pair = state.map.pair;
  const count = 512 + pair.secondary.tileCount();
  const cols = 16, rows = Math.ceil(count / cols);
  tilesCanvas.width = cols * TILE; tilesCanvas.height = rows * TILE;
  tilesCanvas.style.width = (tilesCanvas.width * 2) + 'px'; tilesCanvas.style.height = (tilesCanvas.height * 2) + 'px';
  const img = tctx.createImageData(tilesCanvas.width, tilesCanvas.height);
  const pal = pair.palettes[palIndex] || pair.palettes[0];
  for (let t = 0; t < count; t++) {
    const pixels = pair.tilePixels(t);
    const ox = (t % cols) * TILE, oy = Math.floor(t / cols) * TILE;
    for (let py = 0; py < TILE; py++) for (let px = 0; px < TILE; px++) {
      const col = pal[pixels[py * TILE + px]];
      const o = ((oy + py) * tilesCanvas.width + ox + px) * 4;
      img.data[o] = col[0]; img.data[o + 1] = col[1]; img.data[o + 2] = col[2]; img.data[o + 3] = 255;
    }
  }
  tctx.putImageData(img, 0, 0);
  const sel = parseInt($('mt-tile').value, 10) || 0;
  tctx.strokeStyle = '#4caf50'; tctx.lineWidth = 1;
  tctx.strokeRect((sel % cols) * TILE + 0.5, Math.floor(sel / cols) * TILE + 0.5, TILE - 1, TILE - 1);
}

function writeEntry() {
  const m = state.map;
  if (!m) return;
  const id = selectedMetatile(), pair = m.pair;
  if (id >= pair.metatileCount) return;
  const entries = pair.entries(id);
  const tile = Math.max(0, Math.min(1023, parseInt($('mt-tile').value, 10) || 0));
  const pal = Math.max(0, Math.min(15, parseInt($('mt-pal').value, 10) || 0));
  entries[state.entry.layer * 4 + state.entry.quadrant] = tile | (($('mt-hflip').checked ? 1 : 0) << 10) | (($('mt-vflip').checked ? 1 : 0) << 11) | (pal << 12);
  pair.setEntries(id, entries);
  const attr = (pair.attribute(id) & ~0x30FF) | (Math.max(0, Math.min(255, parseInt($('mt-behavior').value, 10) || 0))) | ((parseInt($('mt-layer').value, 10) & 3) << 12);
  pair.setAttribute(id, attr);
  state.touchedTilesets.add(pair.which(id)[0].offset);
  refreshSheet(pair);
  drawMap(); drawPicker(); drawBorder(); drawMetatileEditor(); autosave();
}

for (const [cid, layer] of [['mt-bottom', 0], ['mt-top', 1]]) {
  $(cid).addEventListener('mousedown', (ev) => {
    const r = $(cid).getBoundingClientRect();
    state.entry = { layer, quadrant: Math.floor((ev.clientX - r.left) / 32) + Math.floor((ev.clientY - r.top) / 32) * 2 };
    drawMetatileEditor();
  });
}
tilesCanvas.addEventListener('mousedown', (ev) => {
  const r = tilesCanvas.getBoundingClientRect();
  const t = Math.floor((ev.clientX - r.left) / (TILE * 2)) + Math.floor((ev.clientY - r.top) / (TILE * 2)) * 16;
  $('mt-tile').value = t; writeEntry();
});
for (const id of ['mt-tile', 'mt-hflip', 'mt-vflip', 'mt-pal', 'mt-behavior', 'mt-layer']) $(id).addEventListener('change', writeEntry);

// ---- files ---------------------------------------------------------------------------

function download(name, data, type) {
  const blob = new Blob([data], { type });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob); a.download = name; a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

// The edits: every touched map's blocks and border, every touched tileset's changed metatiles
function collectEdits() {
  const game = state.game, original = new Rom(state.original);
  const edits = { format: 'GBMapping/1', game: state.rom.header().code, title: state.rom.header().title, maps: {}, tilesets: {} };
  for (const id of state.touchedMaps) {
    const m = game.loadMap(id);
    const o = m.layout;
    let changed = false;
    for (let i = 0; i < m.blocks.length && !changed; i++) if (original.u16(o.blocksPtr + i * 2) !== m.blocks[i]) changed = true;
    for (let i = 0; i < 4 && !changed; i++) if (original.u16(o.borderPtr + i * 2) !== m.border[i]) changed = true;
    if (changed) edits.maps[id] = { width: m.width, height: m.height, blocks: Array.from(m.blocks), border: Array.from(m.border) };
  }
  for (const offset of state.touchedTilesets) {
    const t = game.tileset(offset);
    const entry = { metatiles: {}, attributes: {} };
    for (let i = 0; i < t.metatileCount(); i++) {
      const now = t.entries(i), was = [];
      for (let k = 0; k < 8; k++) was.push(original.u16(t.metatilesPtr + i * 16 + k * 2));
      if (now.some((v, k) => v !== was[k])) entry.metatiles[i] = now;
      const a = t.attribute(i);
      if (a !== original.u16(t.attributesPtr + i * 2)) entry.attributes[i] = a;
    }
    if (Object.keys(entry.metatiles).length || Object.keys(entry.attributes).length) edits.tilesets['0x' + hex(offset)] = entry;
  }
  return edits;
}

function applyEdits(edits) {
  const game = state.game;
  for (const [id, e] of Object.entries(edits.maps || {})) {
    let m;
    try { m = game.loadMap(id); } catch (err) { console.warn('no map', id); continue; }
    if (e.width !== m.width || e.height !== m.height) { console.warn('size differs for', id); continue; }
    m.blocks = Uint16Array.from(e.blocks); m.border = Uint16Array.from(e.border);
    game.writeBlocks(m); game.writeBorder(m); state.touchedMaps.add(id);
  }
  for (const [addr, e] of Object.entries(edits.tilesets || {})) {
    const t = game.tileset(parseInt(addr, 16));
    if (!t.valid) { console.warn('no tileset at', addr); continue; }
    for (const [i, entries] of Object.entries(e.metatiles || {})) t.setEntries(Number(i), entries);
    for (const [i, a] of Object.entries(e.attributes || {})) t.setAttribute(Number(i), a);
    state.touchedTilesets.add(t.offset);
  }
  state.sheets.clear();
  for (const p of game.pairs.values()) p.invalidate();
  if (state.map) openMap(state.map.id); else renderList();
}

function autosave() {
  try {
    const edits = collectEdits();
    if (Object.keys(edits.maps).length || Object.keys(edits.tilesets).length) localStorage.setItem(storageKey(), JSON.stringify(edits));
    else localStorage.removeItem(storageKey());
  } catch (e) { /* storage full or blocked: the ROM still holds the edits */ }
}

$('save-rom').onclick = () => {
  if (!state.rom) return;
  const name = state.rom.name.replace(/\.(gba|gbc|gb)$/i, '') + '-edited.gba';
  download(name, state.rom.export(), 'application/octet-stream');
};
$('export-edits').onclick = () => {
  if (!state.rom) return;
  download(state.rom.header().title.trim().replace(/\s+/g, '-').toLowerCase() + '-edits.json', JSON.stringify(collectEdits()), 'application/json');
};
$('import-edits').addEventListener('change', async (ev) => {
  const f = ev.target.files[0];
  if (!f || !state.rom) return;
  try { applyEdits(JSON.parse(await f.text())); autosave(); } catch (e) { alert('Not an edits file: ' + e.message); }
  ev.target.value = '';
});
$('rom-file').addEventListener('change', (ev) => { loadFiles([...ev.target.files]); ev.target.value = ''; });
document.body.addEventListener('dragover', (ev) => { ev.preventDefault(); document.body.classList.add('dragging'); });
document.body.addEventListener('dragleave', () => document.body.classList.remove('dragging'));
document.body.addEventListener('drop', (ev) => { ev.preventDefault(); document.body.classList.remove('dragging'); loadFiles([...ev.dataTransfer.files]); });

// ---- ui ---------------------------------------------------------------------------------

$('search').oninput = renderList;
$('zoom').oninput = () => { state.zoom = parseInt($('zoom').value, 10); if (state.map) { sizeCanvas(); drawMap(); } };
for (const id of ['show-grid', 'show-collision', 'show-elevation']) $(id).onchange = drawMap;
$('tool').onchange = () => { $('elevation-label').style.display = $('tool').value === 'elevation' ? '' : 'none'; };
$('tool').onchange();
$('undo').onclick = undo;
$('redo').onclick = redo;
document.querySelectorAll('.tab').forEach((b) => {
  b.onclick = () => {
    document.querySelectorAll('.tab').forEach((x) => x.classList.toggle('active', x === b));
    document.querySelectorAll('.tab-page').forEach((p) => p.classList.toggle('hidden', p.id !== 'tab-' + b.dataset.tab));
    if (b.dataset.tab === 'tileset') drawMetatileEditor();
  };
});
window.addEventListener('keydown', (ev) => {
  if (ev.target.tagName === 'INPUT' || ev.target.tagName === 'SELECT') return;
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 'z') { ev.preventDefault(); ev.shiftKey ? redo() : undo(); }
  else if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 'y') { ev.preventDefault(); redo(); }
  else if (ev.key === 'g') { $('show-grid').checked = !$('show-grid').checked; drawMap(); }
  else if (ev.key === 'c') { $('show-collision').checked = !$('show-collision').checked; drawMap(); }
  else if (ev.key === 'e') { $('show-elevation').checked = !$('show-elevation').checked; drawMap(); }
});
