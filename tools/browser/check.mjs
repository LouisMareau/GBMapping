// The editor checked in a real browser, end to end: the built site (dist/) is served,
// a headless Chrome opens it, a ROM goes in through the file chooser, tiles are painted
// with the mouse, and what comes out (the saved ROM, the exported edits) is compared
// with what should. Screenshots of each step land in target/browser-check/.
//
//     trunk build --release
//     node tools/browser/check.mjs <rom.gba> [profile.json]
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
const [rom, profile] = process.argv.slice(2).map((path) => resolve(path));
if (!rom || !existsSync(join(dist, 'index.html'))) {
  console.error('usage: node tools/browser/check.mjs <rom.gba> [profile.json]   (after: trunk build --release)');
  process.exit(2);
}

// -- where things are -------------------------------------------------------------
const [WIDTH, HEIGHT] = [1400, 900];
const ZOOM = 8;
const toolbar = { open: [124, 15], save: [191, 15], exportEdits: [272, 15] };
const firstMap = [100, 83];
const searchBox = [110, 49];
const tilesetTab = [WIDTH - 292 + 101, 49];
const metatilesTab = [WIDTH - 292 + 46, 49];
/// The picker's metatile `id` (8 to a row, 32 pixels each)
const metatile = (id) => [WIDTH - 292 + 9 + (id % 8) * 32 + 16, 160 + Math.floor(id / 8) * 32 + 16];
/// The map's tile (x, y) at zoom 8, the map scrolled to its top left
const tile = (x, y) => [242 + x * 16 * ZOOM + 8 * ZOOM, 46 + y * 16 * ZOOM + 8 * ZOOM];
/// With the reference shown, the map and the reference share the middle of the page,
/// each under a strip of controls: their tiles (x, y), at zoom 8 too. Only the first
/// three columns are whole in each; `across` is how far into the tile to aim.
const besideTile = (x, y, across = 0.5) => [242 + (x + across) * 16 * ZOOM, 97 + (y + 0.5) * 16 * ZOOM];
const referenceTile = (x, y, across = 0.5) => [682 + (x + across) * 16 * ZOOM, 97 + (y + 0.5) * 16 * ZOOM];
/// In the tileset tab: the button that goes to a metatile no map shows, the "All
/// tilesets" box, and three places in the list of tiles under them
const tilesetPanel = { nextFree: [WIDTH - 292 + 40, 110], allTilesets: [WIDTH - 292 + 119, 330], list: [[WIDTH - 220, 500], [WIDTH - 100, 620], [WIDTH - 180, 740]] };
/// Where the two maps' top left corners are then
const [mapCorner, referenceCorner] = [[242, 97], [682, 97]];
/// The reference's source menu, and its entries when the ROM keeps its maps' originals
const referenceSource = { menu: [WIDTH - 292 - 193, 47], originals: [WIDTH - 292 - 273, 72], opened: [WIDTH - 292 - 273, 93] };
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
const downloaded = () => readdirSync(downloads).filter((name) => /\.(gba|json)$/i.test(name));

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

  // -- painting -------------------------------------------------------------------------------
  for (let i = 2; i < ZOOM; i++) await browser.key('=');
  await browser.click(...metatile(4));
  await browser.drag([tile(1, 1), tile(2, 1), tile(3, 1)]);
  await browser.move(...tile(4, 3));
  await shot('painted');
  const exported = await download(toolbar.exportEdits);
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
  await browser.click(...tile(1, 1), 'right');
  await browser.click(...tilesetTab);
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
