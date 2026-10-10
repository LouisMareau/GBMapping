// Drives a headless Chrome through the DevTools protocol, with nothing to install:
// Node's own WebSocket and a Chrome (or Edge) that's already on the machine.
// The browser runs with a throwaway profile, apart from the user's own.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CANDIDATES = [
  process.env.CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export class Browser {
  /// `profiles`: where the throwaway profile goes. A disk with room: on a nearly full one,
  /// Chrome fails to hold large files (a ROM made into a Blob) in ways that look like network errors.
  static async launch({ width = 1400, height = 900, port = 9333, headless = true, profiles = tmpdir() } = {}) {
    const binary = CANDIDATES.find((path) => path && existsSync(path));
    if (!binary) throw new Error('No Chrome found: set CHROME to its path');
    mkdirSync(profiles, { recursive: true });
    const profile = mkdtempSync(join(profiles, 'gbmapping-chrome-'));
    const args = [`--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${width},${height}`, '--no-first-run', '--no-default-browser-check',
      '--enable-unsafe-swiftshader', '--force-device-scale-factor=1', 'about:blank'];
    if (headless) args.unshift('--headless=new');
    const child = spawn(binary, args, { stdio: 'ignore' });
    const browser = new Browser(child, profile, port);
    try {
      await browser.connect();
    } catch (error) {
      await browser.close();
      throw error;
    }
    return browser;
  }

  constructor(child, profile, port) {
    Object.assign(this, { child, profile, port, id: 0, waiting: new Map(), listeners: new Map(), console: [], errors: [], mouse: { x: 0, y: 0 } });
  }

  async connect() {
    let target;
    for (let i = 0; i < 100 && !target; i++) {
      await sleep(100);
      try {
        const targets = await (await fetch(`http://127.0.0.1:${this.port}/json/list`)).json();
        target = targets.find((t) => t.type === 'page');
      } catch { /* not up yet */ }
    }
    if (!target) throw new Error('Chrome did not start');
    this.socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { this.socket.onopen = resolve; this.socket.onerror = () => reject(new Error('no connection to Chrome')); });
    this.socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id) {
        const { resolve, reject } = this.waiting.get(message.id);
        this.waiting.delete(message.id);
        message.error ? reject(new Error(message.error.message)) : resolve(message.result);
      } else if (this.listeners.has(message.method)) {
        this.listeners.get(message.method)(message.params);
        this.listeners.delete(message.method);
      } else if (message.method === 'Runtime.consoleAPICalled') {
        this.console.push(`${message.params.type}: ${message.params.args.map((a) => a.value ?? a.description ?? '').join(' ')}`);
      } else if (message.method === 'Runtime.exceptionThrown') {
        this.errors.push(message.params.exceptionDetails.exception?.description ?? message.params.exceptionDetails.text);
      }
    };
    await this.send('Page.enable');
    await this.send('Runtime.enable');
  }

  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  /// The next event of a kind (its parameters)
  once(method, timeout = 15000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`No ${method} event`)), timeout);
      this.listeners.set(method, (params) => { clearTimeout(timer); resolve(params); });
    });
  }

  /// The page's size, whatever the window's
  async viewport(width, height) {
    await this.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  }

  /// Clicks what opens a file chooser, and gives it files instead of showing it
  async choose(files, x, y) {
    await this.send('Page.setInterceptFileChooserDialog', { enabled: true });
    const opened = this.once('Page.fileChooserOpened');
    await this.click(x, y);
    const { backendNodeId } = await opened;
    await this.send('DOM.setFileInputFiles', { files, backendNodeId });
    await this.send('Page.setInterceptFileChooserDialog', { enabled: false });
  }

  async goto(url) {
    await this.send('Page.navigate', { url });
  }

  /// The value of a JavaScript expression in the page (promises are awaited)
  async evaluate(expression) {
    const { result, exceptionDetails } = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  }

  async waitFor(expression, { timeout = 30000, what = expression } = {}) {
    const end = Date.now() + timeout;
    while (Date.now() < end) {
      if (await this.evaluate(expression).catch(() => false)) return;
      await sleep(100);
    }
    throw new Error(`Timed out waiting for ${what}`);
  }

  wait(ms) { return sleep(ms); }

  async screenshot(path) {
    const { data } = await this.send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(path, Buffer.from(data, 'base64'));
    return path;
  }

  // -- the pointer ---------------------------------------------------------------

  async move(x, y, buttons = 0) {
    this.mouse = { x, y };
    await this.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, buttons, button: buttons === 1 ? 'left' : buttons === 2 ? 'right' : 'none' });
  }

  async press(button = 'left') {
    await this.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...this.mouse, button, buttons: button === 'left' ? 1 : 2, clickCount: 1 });
  }

  async release(button = 'left') {
    await this.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...this.mouse, button, buttons: 0, clickCount: 1 });
  }

  async click(x, y, button = 'left') {
    await this.move(x, y);
    await sleep(60);
    await this.press(button);
    await sleep(60);
    await this.release(button);
    await sleep(120);
  }

  /// A drag through the given points, the button held
  async drag(points, button = 'left') {
    const buttons = button === 'left' ? 1 : 2;
    await this.move(...points[0]);
    await sleep(60);
    await this.press(button);
    for (const [x, y] of points.slice(1)) {
      await sleep(40);
      await this.move(x, y, buttons);
    }
    await sleep(60);
    await this.release(button);
    await sleep(120);
  }

  async key(key, { ctrl = false, shift = false } = {}) {
    const modifiers = (ctrl ? 2 : 0) | (shift ? 8 : 0);
    const signs = { '=': ['Equal', 187], '-': ['Minus', 189] };
    const [code, virtualKey] = signs[key] ?? (key.length === 1 ? [`Key${key.toUpperCase()}`, key.toUpperCase().charCodeAt(0)] : [key, 0]);
    const params = { key, code, modifiers, windowsVirtualKeyCode: virtualKey };
    await this.send('Input.dispatchKeyEvent', { type: 'keyDown', ...params });
    await this.send('Input.dispatchKeyEvent', { type: 'keyUp', ...params });
    await sleep(120);
  }

  async type(text) {
    await this.send('Input.insertText', { text });
    await sleep(120);
  }

  /// Files dragged from the system and dropped at a point of the page
  async drop(files, x, y) {
    const data = { items: [], files, dragOperationsMask: 1 };
    for (const type of ['dragEnter', 'dragOver', 'drop']) {
      await this.send('Input.dispatchDragEvent', { type, x, y, data });
      await sleep(80);
    }
  }

  /// Downloads go to a folder instead of asking
  async downloadsTo(downloadPath) {
    await this.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath });
  }

  async close() {
    try { this.socket?.close(); } catch { /* already closed */ }
    this.child.kill();
    await sleep(400);
    try { rmSync(this.profile, { recursive: true, force: true }); } catch { /* Chrome may still hold a file: the system's temp cleanup gets it */ }
  }
}
