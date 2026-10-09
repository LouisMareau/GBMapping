// Reads a ROM (and an optional profile JSON) from the command line and lists
// its maps; renders one map to a PPM when a map id is given:
//   node test/parse.mjs <rom.gba> [profile.json] [map id] [out.ppm]
import { readFileSync, writeFileSync } from 'node:fs';
import { Rom } from '../src/rom.js';
import { detectGame } from '../src/games/index.js';
import { METATILE } from '../src/gfx.js';

const [romPath, ...rest] = process.argv.slice(2);
let profile = {}, mapId = null, out = null;
for (const a of rest) {
  if (a.endsWith('.json')) profile = JSON.parse(readFileSync(a, 'utf8'));
  else if (a.endsWith('.ppm')) out = a;
  else mapId = a;
}
const rom = new Rom(new Uint8Array(readFileSync(romPath)), romPath);
console.log('header', rom.header(), 'size', rom.size);
const game = detectGame(rom, profile);
if (!game) { console.log('no profile fits'); process.exit(1); }
console.log('profile', game.name, 'gMapGroups', game.profile.gMapGroups.toString(16));
const maps = game.maps();
console.log(maps.length, 'maps in', game.groups().length, 'groups');
for (const m of maps.slice(0, 12)) console.log(' ', m.id, m.name, 'type', m.mapType, 'mapsec', m.mapsec);
if (mapId) {
  const map = game.loadMap(mapId);
  console.log('map', map.id, map.name, map.width + 'x' + map.height, 'pair', map.pair.key, 'metatiles', map.pair.metatileCount,
    'tiles', map.pair.primary.tileCount(), '+', map.pair.secondary.tileCount(), 'compressed', map.pair.primary.isCompressed, map.pair.secondary.isCompressed);
  if (out) {
    const w = map.width * METATILE, h = map.height * METATILE;
    const rgb = Buffer.alloc(w * h * 3);
    for (let y = 0; y < map.height; y++) {
      for (let x = 0; x < map.width; x++) {
        const img = map.pair.image(map.blocks[y * map.width + x] & 0x3FF);
        for (let py = 0; py < METATILE; py++) for (let px = 0; px < METATILE; px++) {
          const s = (py * METATILE + px) * 4, d = ((y * METATILE + py) * w + x * METATILE + px) * 3;
          rgb[d] = img[s]; rgb[d + 1] = img[s + 1]; rgb[d + 2] = img[s + 2];
        }
      }
    }
    writeFileSync(out, Buffer.concat([Buffer.from(`P6\n${w} ${h}\n255\n`), rgb]));
    console.log('wrote', out);
  }
}
