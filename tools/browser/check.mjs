// The editor checked in a real browser, end to end: the built site (dist/) is served,
// a headless Chrome opens it, a ROM goes in through the file chooser, tiles are painted
// with the mouse, and what comes out (the saved ROM, the exported edits) is compared
// with what should. Screenshots of each step land in target/browser-check/.
//
//     trunk build --release
//     node tools/browser/check.mjs <rom.gba | rom.gbc> [profile.json] [reference.gbc [map]]
//
// A second ROM is opened as the reference of the first (a Game Boy Color game next to
// the Advance one made of it), with the first's map `map` on screen.
//
// The editor draws itself on a canvas: the script clicks where things are in a
// 1400x900 page. When the layout changes, the positions below are to be measured again
// on the screenshots.
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Browser } from './cdp.mjs';
import { decodePng, difference } from './png.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const dist = join(root, 'dist');
const out = join(root, 'target', 'browser-check');
const downloads = join(out, 'downloads');
const [first, ...rest] = process.argv.slice(2);
const isRom = (name) => /\.(gba|gbc|gb)$/i.test(name);
const rom = first && resolve(first);
const [profile, reference] = [rest.find((name) => /\.json$/i.test(name)), rest.find(isRom)].map((path) => path && resolve(path));
/// The map of the ROM to have on screen next to the reference
const beside = rest.find((name) => !isRom(name) && !/\.json$/i.test(name));
if (!rom || !existsSync(join(dist, 'index.html'))) {
  console.error('usage: node tools/browser/check.mjs <rom.gba | rom.gbc> [profile.json] [reference.gbc [map]]   (after: trunk build --release)');
  process.exit(2);
}
/// A Game Boy Color ROM: its blocks are four tiles each, and they're painted whole
const gameBoy = /\.gbc?$/i.test(rom);

// -- where things are -------------------------------------------------------------
const [WIDTH, HEIGHT] = [1400, 900];
const ZOOM = 8;
const toolbar = { open: [124, 15], save: [191, 15], exportEdits: [272, 15] };
const firstMap = [100, 83];
const searchBox = [110, 49];
const tilesetTab = [WIDTH - 292 + 101, 49];
const metatilesTab = [WIDTH - 292 + 46, 49];
/// The picker's metatile `id` (8 to a row, 32 pixels each), under the two rows of the
/// map's tilesets (which a Game Boy Color map, of one tileset, doesn't have)
const metatile = (id) => [WIDTH - 292 + 9 + (id % 8) * 32 + 16, (gameBoy ? 168 : 219) + Math.floor(id / 8) * 32 + 16];
/// Over the picker: the tool that makes a metatile dragged onto another change places with it
const moveTool = [WIDTH - 292 + 24, gameBoy ? 79 : 130];
/// In those rows: the button that gives the map a copy of its secondary tileset
const ownTileset = [WIDTH - 292 + 210, 100];
/// The map's tile (x, y) at zoom 8, the map scrolled to its top left
const tile = (x, y) => [242 + x * 16 * ZOOM + 8 * ZOOM, 46 + y * 16 * ZOOM + 8 * ZOOM];
/// With the reference shown, the map and the reference share the middle of the page,
/// each under a strip of controls: their tiles (x, y), at zoom 8 too. Only the first
/// three columns are whole in each; `across` is how far into the tile to aim.
const besideTile = (x, y, across = 0.5) => [242 + (x + across) * 16 * ZOOM, 97 + (y + 0.5) * 16 * ZOOM];
const referenceTile = (x, y, across = 0.5) => [682 + (x + across) * 16 * ZOOM, 97 + (y + 0.5) * 16 * ZOOM];
/// In the tileset tab: the buttons that copy the metatile in hand into a new one and
/// that go to a metatile no map shows, the "All tilesets" box, and three places in the
/// list of tiles under them
const tilesetPanel = {
  duplicate: [WIDTH - 292 + 82, 123],
  nextFree: [WIDTH - 292 + 154, 123],
  allTilesets: [WIDTH - 292 + 119, 343],
  list: [[WIDTH - 220, 500], [WIDTH - 100, 620], [WIDTH - 180, 740]],
};
/// The Draw tab: its tab, the eraser, the pixel (x, y) of the metatile drawn on, and the
/// colour `i` of the palette under it
const draw = {
  tab: [WIDTH - 292 + 148, 49],
  eraser: [WIDTH - 292 + 81, 154],
  pixel: (x, y) => [WIDTH - 292 + 9 + x * 17 + 8, 187 + y * 17 + 8],
  color: (i) => [WIDTH - 292 + 9 + i * 17 + 8, 495],
};
/// The Anim tab: its tab, the box that lists every animation of the ROM, the list and its
/// first entry under its tileset's name (tiles of 16x16 pixels, in the games checked), the frame `i` of those side
/// by side, the Ghost box, the pixel (x, y) of the frame drawn on, and the colour `i` of
/// the palette under it
const anim = {
  tab: [WIDTH - 292 + 190, 49],
  all: [WIDTH - 292 + 16, 79],
  list: [WIDTH - 292 + 140, 100],
  first: [WIDTH - 292 + 100, 137],
  frame: (i) => [WIDTH - 292 + 35 + i * 54, 161],
  ghost: [WIDTH - 292 + 114, 199],
  pixel: (x, y) => [WIDTH - 292 + 9 + x * 17 + 8, 232 + y * 17 + 8],
  color: (i) => [WIDTH - 292 + 9 + i * 17 + 8, 532],
};
/// The Events mode, on Littleroot Town at zoom 2 (its events' panel where the reference
/// is, 360 wide): the map's tile (x, y), the button that adds a person, the text a new
/// person says, and the button that writes it
const eventsPanel = WIDTH - 292 - 360;
const events = {
  tile: (x, y) => [242 + x * 32 + 16, 135 + y * 32 + 16],
  addPerson: [eventsPanel + 57, 93],
  text: [eventsPanel + 180, 752],
  write: [eventsPanel + 54, 820],
};
/// Where the two maps' top left corners are then
const [mapCorner, referenceCorner] = [[242, 97], [682, 97]];
/// The reference's source menu, and its entries when the ROM keeps its maps' originals
const referenceSource = { menu: [WIDTH - 292 - 193, 47], originals: [WIDTH - 292 - 273, 72], opened: [WIDTH - 292 - 273, 93] };
/// Its "Another ROM…" entry: the last one, under one or two of this ROM's
const anotherRom = (originals) => [WIDTH - 292 - 273, originals ? 114 : 93];
/// The "Apply them" button of the kept edits' dialog, from the middle of the page
const applyKept = [WIDTH / 2 - 174, HEIGHT / 2 + 37];

// -- helpers --------------------------------------------------------------------------
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json', '.png': 'image/png' };
const server = createServer((request, response) => {
  let path = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
  if (path === '/__rom') {
    // the ROM, for the page to make a File of (as the system does when it launches the app with one)
    response.writeHead(200, { 'Content-Type': 'application/octet-stream' }).end(readFileSync(rom));
    return;
  }
  if (path.endsWith('/')) path += 'index.html';
  const file = join(dist, path);
  if (!file.startsWith(dist) || !existsSync(file)) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((done) => server.listen(0, '127.0.0.1', done));
const url = `http://localhost:${server.address().port}/`;

let failures = 0;
const check = (what, ok, detail = '') => {
  console.log(`${ok ? '  ok  ' : ' FAIL '} ${what}${detail ? `  (${detail})` : ''}`);
  if (!ok) failures++;
};

rmSync(out, { recursive: true, force: true });
mkdirSync(downloads, { recursive: true });
const browser = await Browser.launch({ width: WIDTH, height: HEIGHT, profiles: join(root, 'target', 'browser-profiles') });
let step = 0;
const shot = async (name) => { await browser.wait(300); await browser.screenshot(join(out, `${String(++step).padStart(2, '0')}-${name}.png`)); };

/// What the editor has downloaded. A new Chrome profile also drops files of its own in
/// the folder while it sets itself up (and holds them open): those are left alone.
const downloaded = () => readdirSync(downloads).filter((name) => /\.(gba|gbc|json)$/i.test(name));

/// Clicks a button that downloads a file, and returns the file's bytes
const download = async (button) => {
  for (const name of downloaded()) rmSync(join(downloads, name));
  await browser.click(...button);
  for (let i = 0; i < 100; i++) {
    await browser.wait(100);
    const done = downloaded();
    if (done.length) return readFileSync(join(downloads, done[0]));
  }
  return null;
};
const started = () => browser.waitFor("!document.getElementById('loading')", { what: 'the editor to start' });
const opened = () => browser.waitFor("document.title.includes(' · ')", { what: 'a map to open' });
const differing = (a, b) => { let n = a.length === b.length ? 0 : Infinity; for (let i = 0; i < a.length && n < Infinity; i++) n += a[i] !== b[i]; return n; };

try {
  const original = readFileSync(rom);
  await browser.viewport(WIDTH, HEIGHT);
  await browser.downloadsTo(downloads);
  await browser.goto(url);
  await started();
  await shot('welcome');

  // -- an app that can be installed ----------------------------------------------------
  const manifest = await browser.send('Page.getAppManifest');
  check('the web app manifest is read without error', manifest.url.endsWith('manifest.json') && manifest.errors.length === 0, JSON.stringify(manifest.errors));
  const installable = await browser.send('Page.getInstallabilityErrors');
  check('Chrome finds it installable', installable.installabilityErrors.length === 0, JSON.stringify(installable.installabilityErrors));
  const worker = await browser.evaluate("Promise.race([navigator.serviceWorker.ready.then(() => 'ready'), new Promise((r) => setTimeout(() => r('none'), 8000))])");
  check('the service worker is up (the editor works offline)', worker === 'ready');

  // -- a ROM in, through the file chooser ------------------------------------------------
  await browser.choose(profile ? [rom, profile] : [rom], ...toolbar.open);
  await opened();
  const title = await browser.evaluate('document.title');
  check('the ROM opens and the first map shows', title.endsWith('— GBMapping'), title);
  await shot('opened');

  let exported = null;
  if (gameBoy) {
    // -- a block picked by one of its quarters, and painted whole ----------------------------------
    for (let i = 2; i < ZOOM; i++) await browser.key('=');
    await browser.click(...metatile(18));
    await browser.click(...tile(2, 2));
    await browser.move(...tile(4, 3));
    await shot('painted');
    exported = await download(toolbar.exportEdits);
    const edits = exported && JSON.parse(exported.toString());
    const map = edits && Object.values(edits.maps)[0];
    // (and with it the maps the ROM draws from the same blocks: a town's Pokémon Center is every town's)
    check('the edits file tells the changed map', !!map && edits.format === 'GBMapping/1' && Object.keys(edits.maps)[0] === '0.0', edits && Object.keys(edits.maps).join(' '));
    if (map) {
      const block = [[2, 2], [3, 2], [2, 3], [3, 3]].map(([x, y]) => map.blocks[y * map.width + x]);
      check('the block clicked holds the four quarters of the block picked', block.join(' ') === '18 19 26 27', block.join(' '));
    }
    await browser.key('z', { ctrl: true });
    const undone = await download(toolbar.save);
    check('undone, the ROM saved is the original, byte for byte', !!undone && differing(undone, original) === 0);
    await browser.key('y', { ctrl: true });
    const saved = await download(toolbar.save);
    const changed = saved ? differing(saved, original) : -1;
    check('redone, the ROM saved differs by the block painted and the checksum', changed >= 1 && changed <= 3, `${changed} bytes`);
    // two blocks change places: their tiles and collisions in the ROM, and the maps' blocks that are one or the other
    await browser.click(...moveTool);
    await browser.drag([metatile(18), metatile(0)]);
    await browser.move(...tile(4, 3));
    await shot('blocks-moved');
    const moved = await download(toolbar.save);
    const movedBytes = moved ? differing(moved, original) : -1;
    check('a block dragged onto another changes places with it in the ROM', movedBytes > 8, `${movedBytes} bytes`);
    await browser.key('z', { ctrl: true });
    await browser.click(...moveTool);
    const unmoved = await download(toolbar.save);
    check('undone, the ROM is the one with the block painted', !!unmoved && !!saved && unmoved.equals(saved));
    await browser.click(...tilesetTab);
    await shot('tileset-tab');
    await browser.click(...metatilesTab);
    await browser.click(...searchBox);
    await browser.type('gym');
    await browser.click(...firstMap);
    const found = await browser.evaluate('document.title');
    check('a map found by the search opens', found !== title, found);
    await shot('another-map');
  } else {
    // -- painting -------------------------------------------------------------------------------
    for (let i = 2; i < ZOOM; i++) await browser.key('=');
    await browser.click(...metatile(4));
    await browser.drag([tile(1, 1), tile(2, 1), tile(3, 1)]);
    await browser.move(...tile(4, 3));
    await shot('painted');
    exported = await download(toolbar.exportEdits);
    const edits = exported && JSON.parse(exported.toString());
    const map = edits && Object.values(edits.maps)[0];
    check('the edits file tells one changed map', !!map && edits.format === 'GBMapping/1' && Object.keys(edits.maps).length === 1, edits && Object.keys(edits.maps).join(' '));
    if (map) {
      const painted = [1, 2, 3].map((x) => map.blocks[map.width + x] & 0x3FF);
      check('the three tiles dragged over hold the metatile picked', painted.every((id) => id === 4), painted.join(' '));
    }

    // -- undo, redo, save -----------------------------------------------------------------------
    await browser.key('z', { ctrl: true });
    const undone = await download(toolbar.save);
    check('undone, the ROM saved is the original, byte for byte', !!undone && differing(undone, original) === 0);
    await browser.key('y', { ctrl: true });
    const saved = await download(toolbar.save);
    const changed = saved ? differing(saved, original) : -1;
    check('redone, the ROM saved differs by the painted tiles only', changed >= 1 && changed <= 6, `${changed} bytes`);

    // -- the reference: the ROM as it was opened, next to the map -----------------------------------
    await browser.key('r');
    await browser.move(...besideTile(1, 2));
    await shot('reference');
    // what it shows where the tiles were painted is picked with the right button, and painted back
    await browser.drag([referenceTile(1, 1), referenceTile(2, 1)], 'right');
    await browser.click(...besideTile(1, 1));
    await browser.click(...referenceTile(3, 1, 0.15), 'right');
    await browser.click(...besideTile(3, 1, 0.15));
    await shot('reference-restored');
    const restored = await download(toolbar.save);
    check('tiles picked in the reference and painted back give the original ROM', !!restored && differing(restored, original) === 0, restored ? `${differing(restored, original)} bytes differ` : 'no file');
    await browser.key('z', { ctrl: true });
    await browser.key('z', { ctrl: true });
    await browser.key('r');

    // -- the rest of the window, for the eye -------------------------------------------------------
    await browser.key('c');
    await browser.key('e');
    await browser.key('g');
    await shot('overlays');
    await browser.key('c');
    await browser.key('e');
    await browser.key('g');
    await browser.click(...tilesetTab);
    await shot('tileset-tab');
    await browser.click(...metatilesTab);
    await browser.click(...searchBox);
    await browser.type('gym');
    await shot('search');
    await browser.click(...firstMap);
    const found = await browser.evaluate('document.title');
    check('a map found by the search opens', found !== title, found);
    await shot('another-map');

    // -- the maps' originals, when the profile tells where the ROM keeps them -------------------------
    const told = profile ? JSON.parse(readFileSync(profile, 'utf8')) : {};
    const kept = (told.groups ?? []).flatMap((group) => group.maps).find((map) => map && map.original);
    if (kept) {
      await browser.click(...searchBox);
      await browser.key('a', { ctrl: true });
      await browser.type(kept.name);
      await browser.click(...firstMap);
      const shown = await browser.evaluate('document.title');
      check('a map that comes with its original opens', shown.startsWith(`${kept.name} ·`), shown);
      await browser.key('r');
      // how much of the map's pane differs from the reference's, tile facing tile (the pointer out of the way)
      const apart = async (name) => {
        await browser.move(WIDTH / 2, HEIGHT - 8);
        await browser.wait(700);
        const picture = decodePng(readFileSync(await browser.screenshot(join(out, `${String(++step).padStart(2, '0')}-${name}.png`))));
        return difference(picture, mapCorner, referenceCorner, 380, 640);
      };
      const original = await apart('original');
      check('the reference shows the original, on its own tilesets: not what the map looks like now', original > 0.25, `${(original * 100).toFixed(1)}% of the pixels differ`);
      await browser.click(...referenceSource.menu);
      await browser.click(...referenceSource.opened);
      const opened = await apart('as-opened');
      check('asked for the ROM as it was opened, it shows the map itself', opened < 0.01, `${(opened * 100).toFixed(1)}% of the pixels differ`);
      await browser.click(...referenceSource.menu);
      await browser.click(...referenceSource.originals);
      const again = await apart('original-again');
      check('and the original again when asked', Math.abs(again - original) < 0.01, `${(again * 100).toFixed(1)}%`);
      await browser.key('r');
    }

    // -- the tileset tab: a tile of another tileset goes into a metatile of this map's ----------------
    const edited = await download(toolbar.exportEdits);
    // a tileset of the map's own: a copy of its secondary one. The edits tell it whole, and the map that it has it.
    await browser.click(...ownTileset);
    await shot('tileset-of-its-own');
    const owned = await download(toolbar.exportEdits);
    const withOwn = owned ? JSON.parse(owned.toString()) : {};
    const made = Object.entries(withOwn.tilesets ?? {}).filter(([, tileset]) => tileset.new);
    const given = Object.values(withOwn.maps ?? {}).filter((map) => map.secondary);
    const allOfIt = made.length === 1 && made[0][1].new.secondary === true && Object.keys(made[0][1].palettes ?? {}).length === 16 && Object.keys(made[0][1].metatiles ?? {}).length >= 1;
    check('Duplicate gives the map a copy of its secondary tileset, its own', allOfIt && given.length === 1 && given[0].secondary === made[0][0], allOfIt ? `${made[0][0]}, a copy of ${made[0][1].new.copy_of}: ${Object.keys(made[0][1].metatiles).length} metatiles, ${Object.keys(made[0][1].tiles ?? {}).length} tiles` : `${made.length} tileset(s) made, ${given.length} map(s) given one`);
    await browser.key('z', { ctrl: true });
    const disowned = await download(toolbar.exportEdits);
    check('undone, the map has the tileset it had', !!disowned && !!edited && disowned.equals(edited));

    // two metatiles change places in the picker: the maps are renumbered, and look as they did
    const mapShown = async (name) => {
      await browser.move(WIDTH / 2, HEIGHT - 8);
      await browser.wait(500);
      return decodePng(readFileSync(await browser.screenshot(join(out, `${String(++step).padStart(2, '0')}-${name}.png`))));
    };
    const [left, top, across, down] = [242, 46, 640, 760];
    // (the picker back at its first metatiles: what was picked on the maps since has scrolled it)
    await browser.move(...metatile(20));
    await browser.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: metatile(20)[0], y: metatile(20)[1], deltaX: 0, deltaY: -100000 });
    const beforeMove = await mapShown('before-the-move');
    await browser.click(...moveTool);
    await browser.drag([metatile(5), metatile(2)]);
    const afterMove = await mapShown('metatiles-moved');
    let [mapPixels, pickerPixels] = [0, 0];
    for (let y = top; y < top + down; y++) {
      for (let x = left; x < WIDTH - 12; x++) {
        const o = (y * beforeMove.width + x) * beforeMove.channels;
        const differs = [0, 1, 2].some((c) => beforeMove.data[o + c] !== afterMove.data[o + c]);
        if (x < left + across) mapPixels += differs;
        else if (x > WIDTH - 292 + 9) pickerPixels += differs;
      }
    }
    const moved = await download(toolbar.exportEdits);
    const movedIn = Object.values((moved ? JSON.parse(moved.toString()) : {}).tilesets ?? {}).reduce((n, tileset) => n + Object.keys(tileset.metatiles ?? {}).length, 0);
    check('a metatile dragged onto another changes places with it, and the map looks as it did', mapPixels === 0 && pickerPixels > 0 && movedIn === 2, `${movedIn} metatiles told, ${pickerPixels} pixels of the picker changed, ${mapPixels} of the map`);
    await browser.key('z', { ctrl: true });
    await browser.click(...moveTool);
    const unmoved = await download(toolbar.exportEdits);
    check('undone, the metatiles are where they were', !!unmoved && !!edited && unmoved.equals(edited));
    await browser.click(...tile(1, 1), 'right');
    await browser.click(...tilesetTab);
    // a copy of the metatile in hand, new in the secondary tileset: the edits tell it whole, and no map changes
    await browser.click(...tilesetPanel.duplicate);
    await shot('metatile-duplicated');
    const copied = await download(toolbar.exportEdits);
    const copies = Object.values((copied ? JSON.parse(copied.toString()) : {}).tilesets ?? {});
    const whole = copies.length === 1 && Object.values(copies[0].metatiles ?? {}).length === 1 && Object.values(copies[0].attributes ?? {}).length === 1;
    check('Duplicate gives the secondary tileset a new metatile: the copy of the one in hand', whole && Object.values(copies[0].metatiles)[0].length === 8, whole ? `metatile ${Object.keys(copies[0].metatiles)[0]} of its tileset` : `${copies.length} tileset(s) changed`);
    // drawn on, a pixel at a time: a line in one of its palette's colours, another erased.
    // The quarters drawn on get tiles of their own, which the edits tell.
    await browser.click(...draw.tab);
    await browser.click(...draw.color(13));
    await browser.drag([draw.pixel(1, 1), draw.pixel(6, 6)]);
    await browser.click(...draw.eraser);
    await browser.drag([draw.pixel(14, 1), draw.pixel(9, 6)]);
    await browser.move(WIDTH / 2, HEIGHT / 2);
    await shot('metatile-drawn-on');
    const drawn = await download(toolbar.exportEdits);
    const drawnOn = Object.values((drawn ? JSON.parse(drawn.toString()) : {}).tilesets ?? {});
    const tiles = drawnOn.reduce((n, tileset) => n + Object.keys(tileset.tiles ?? {}).length, 0);
    check('the pencil and the eraser draw on the copy: the edits tell its tiles', drawnOn.length === 1 && tiles >= 1 && tiles <= 2, `${tiles} tile(s) in ${drawnOn.length} tileset(s)`);
    let erased = drawn;
    for (let i = 0; i < 2 && !(erased && copied && erased.equals(copied)); i++) {
      await browser.key('z', { ctrl: true });
      erased = await download(toolbar.exportEdits);
    }
    check('the strokes undone, the edits are the copy alone', !!erased && !!copied && erased.equals(copied));
    await browser.click(...tilesetTab);
    await browser.key('z', { ctrl: true });
    const uncopied = await download(toolbar.exportEdits);
    check('undone, the tileset has the metatiles it had', !!uncopied && !!edited && uncopied.equals(edited));
    // an animation's frame drawn on, the frame before showing through it: the first animation
    // of the ROM (tiles its first tileset keeps changing), its second frame. The edits tell the
    // frame's tiles by where they are, and nothing else.
    await browser.click(...anim.tab);
    await browser.click(...anim.all);
    await browser.click(...anim.list);
    await browser.click(...anim.first);
    await browser.click(...anim.frame(1));
    await browser.click(...anim.ghost);
    await browser.click(...anim.color(13));
    await browser.drag([anim.pixel(1, 1), anim.pixel(6, 6)]);
    await browser.drag([anim.pixel(14, 1), anim.pixel(9, 6)]);
    await browser.move(WIDTH / 2, HEIGHT / 2);
    await shot('frame-drawn-on');
    const framed = await download(toolbar.exportEdits);
    const withFrame = framed ? JSON.parse(framed.toString()) : {};
    const frameTiles = Object.entries(withFrame.graphics ?? {});
    const rest = JSON.stringify({ ...withFrame, graphics: undefined }) === JSON.stringify(edited ? JSON.parse(edited.toString()) : null);
    check('a frame of an animation is drawn on: the edits tell its tiles, where they are in the ROM', frameTiles.length >= 1 && frameTiles.length <= 4 && frameTiles.every(([at, tile]) => /^0x[0-9A-F]{6}$/.test(at) && tile.length === 64) && rest, `${frameTiles.length} tile(s) at ${frameTiles.map(([at]) => at).join(', ')}${rest ? '' : ', and something else changed'}`);
    let unframed = framed;
    for (let i = 0; i < 3 && !(unframed && edited && unframed.equals(edited)); i++) {
      await browser.key('z', { ctrl: true });
      unframed = await download(toolbar.exportEdits);
    }
    check('the strokes undone, the frame is what it was', !!unframed && !!edited && unframed.equals(edited));
    await browser.click(...anim.ghost);
    await browser.click(...anim.all);
    await browser.click(...tilesetTab);
    await browser.click(...tile(1, 1), 'right');
    await browser.click(...tilesetPanel.nextFree);
    await shot('tileset-tab-colours');
    await browser.click(...tilesetPanel.allTilesets);
    await browser.move(...tilesetPanel.list[0]);
    await browser.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: tilesetPanel.list[0][0], y: tilesetPanel.list[0][1], deltaX: 0, deltaY: 6000 });
    await browser.wait(500);
    for (const place of tilesetPanel.list) await browser.click(...place);
    await browser.move(WIDTH / 2, HEIGHT / 2);
    await shot('tile-of-another-tileset');
    const withTile = await download(toolbar.exportEdits);
    const tilesets = Object.values((withTile ? JSON.parse(withTile.toString()) : {}).tilesets ?? {});
    const count = (what) => tilesets.reduce((n, tileset) => n + Object.keys(tileset[what] ?? {}).length, 0);
    check('a tile of another tileset goes into a metatile: the edits carry the tile and the metatile', count('tiles') >= 1 && count('metatiles') >= 1, `${count('tiles')} tile(s), ${count('palettes')} palette(s), ${count('metatiles')} metatile(s)`);
    let back = withTile;
    for (let i = 0; i < 6 && !(back && edited && back.equals(edited)); i++) {
      await browser.key('z', { ctrl: true });
      back = await download(toolbar.exportEdits);
    }
    check('undone, the edits are what they were', !!back && !!edited && back.equals(edited));
    await browser.click(...tilesetPanel.allTilesets);
    await browser.click(...metatilesTab);

    // -- the events: a person added on Littleroot Town, made to say something; Mom moved a tile ----
    // (in this mode the map isn't painted: the edits tell no map's blocks)
    await browser.click(...searchBox);
    await browser.key('a', { ctrl: true });
    await browser.type('LittlerootTown');
    await browser.click(...firstMap);
    for (let i = 2; i < ZOOM; i++) await browser.key('-');
    await browser.key('v');
    await browser.wait(500);
    await browser.click(...events.tile(8, 11));
    await browser.click(...events.addPerson);
    await browser.click(...events.text);
    await browser.type('Hello from GBMapping!');
    await browser.click(...events.write);
    await browser.move(WIDTH / 2, HEIGHT - 8);
    await shot('events-person-added');
    const baseline = edited ? JSON.parse(edited.toString()) : {};
    const editsNow = async () => {
      const file = await download(toolbar.exportEdits);
      return file ? JSON.parse(file.toString()) : {};
    };
    const withPerson = await editsNow();
    const [eventsOf, mapEvents] = Object.entries(withPerson.events ?? {})[0] ?? [];
    const person = mapEvents?.objects?.at(-1);
    const says = Object.values(withPerson.scripts ?? {}).map((script) => script.text);
    const unpainted = JSON.stringify(withPerson.maps ?? {}) === JSON.stringify(baseline.maps ?? {});
    check('a person added in the Events mode, who says a text: the edits tell the map\'s events and the script', !!person && person.x === 8 && person.y === 11 && says.includes('Hello from GBMapping!') && unpainted, `map ${eventsOf}: ${mapEvents?.objects?.length} people, the last at (${person?.x}, ${person?.y}); scripts ${JSON.stringify(says)}${unpainted ? '' : '; the map was painted'}`);
    await browser.drag([events.tile(5, 8), events.tile(6, 8), events.tile(6, 9)]);
    await browser.move(WIDTH / 2, HEIGHT - 8);
    await shot('events-mom-moved');
    const momMoved = (await editsNow()).events?.[eventsOf]?.objects?.[3];
    check('an event dragged on the map moves there', momMoved?.x === 6 && momMoved?.y === 9, `Mom at (${momMoved?.x}, ${momMoved?.y})`);
    let unevented = await download(toolbar.exportEdits);
    for (let i = 0; i < 4 && !(unevented && edited && unevented.equals(edited)); i++) {
      await browser.key('z', { ctrl: true });
      unevented = await download(toolbar.exportEdits);
    }
    check('undone, the map has the events it had', !!unevented && !!edited && unevented.equals(edited));
    await browser.key('p');
    for (let i = 2; i < ZOOM; i++) await browser.key('=');

    // -- another ROM as the reference: a Game Boy Color game next to the Advance one made of it ------
    if (reference) {
      if (beside) {
        await browser.click(...searchBox);
        await browser.key('a', { ctrl: true });
        await browser.type(beside);
        await browser.click(...firstMap);
      }
      const edited = (await browser.evaluate('document.title')).split(' · ')[0];
      await browser.key('r');
      await browser.click(...referenceSource.menu);
      await browser.choose([reference], ...anotherRom(!!kept));
      await browser.wait(1500);
      await browser.move(WIDTH / 2, HEIGHT - 8);
      await browser.wait(700);
      const picture = decodePng(readFileSync(await browser.screenshot(join(out, `${String(++step).padStart(2, '0')}-another-rom.png`))));
      // what the reference's pane shows: a map (many colours), and not the map being edited
      const colors = new Set();
      for (let y = 0; y < 640; y += 4) {
        for (let x = 0; x < 380; x += 4) {
          const o = ((referenceCorner[1] + y) * picture.width + referenceCorner[0] + x) * picture.channels;
          colors.add((picture.data[o] << 16) | (picture.data[o + 1] << 8) | picture.data[o + 2]);
        }
      }
      const apart = difference(picture, mapCorner, referenceCorner, 380, 640);
      check('another ROM is the reference: its map, on its own tiles, next to the one edited', colors.size >= 8 && apart > 0.25, `${edited}: ${colors.size} colours, ${(apart * 100).toFixed(1)}% of the pixels differ from the map's`);
      await browser.key('r');
    }
  }

  // -- the edits are kept from one visit to the next --------------------------------------------
  await browser.wait(500);
  await browser.goto(url);
  await started();
  await browser.drop(profile ? [rom, profile] : [rom], WIDTH / 2, HEIGHT / 2);
  await opened();
  await shot('kept-edits');
  await browser.click(...applyKept);
  await shot('kept-edits-applied');
  const again = await download(toolbar.exportEdits);
  check('after a reload, the kept edits are offered and applied', !!again && !!exported && again.equals(exported));

  // -- launched with a file, as an installed app is ------------------------------------------------
  await browser.evaluate("fetch('/__rom').then((r) => r.blob()).then((blob) => window.gbmappingOpen([new File([blob], 'handed-over.gba')]))");
  await browser.waitFor("document.title.includes('handed-over.gba')", { what: 'the handed-over ROM to open' }).then(() => true, () => false)
    .then((ok) => check('a ROM handed over at launch (the installed app opening a .gba) opens', ok));

  // -- without a network ------------------------------------------------------------------------------
  await browser.wait(1500);
  const before = browser.errors.length + browser.console.length;
  server.closeAllConnections();
  await new Promise((done) => server.close(done));
  await browser.goto(url);
  await browser.wait(500);
  const offline = await started().then(() => true, () => false);
  check('the server gone, the editor still starts (from the service worker)', offline);
  await shot('offline');
  // what the browser says of the missing server isn't the editor's doing
  browser.errors.length = Math.min(browser.errors.length, before);

  const problems = [...browser.errors, ...browser.console.slice(0, before).filter((line) => line.startsWith('error'))];
  check('nothing went wrong in the console', problems.length === 0, problems.join(' | '));
} catch (error) {
  console.error(error);
  failures++;
  await shot('failure').catch(() => {});
} finally {
  await browser.close();
  server.close();
}
console.log(failures ? `\n${failures} check(s) failed: see the screenshots in ${out}` : `\nAll good. Screenshots: ${out}`);
process.exit(failures ? 1 : 0);
