// A small Chrome DevTools Protocol toolkit for testing the website in a real
// Microsoft Edge (or Chrome). Zero dependencies: Node 22's built-in WebSocket
// and fetch. It starts the static server, launches the browser with a fresh
// profile, and gives real mouse, touch and keyboard input.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import net from 'node:net';

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const FIXTURES = process.env.MP_FIXTURES || resolve(ROOT, '..', 'import-fixtures');
export const SHOTS = process.env.MP_SHOTS || resolve(ROOT, '..', 'web-shots');
mkdirSync(SHOTS, { recursive: true });

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function findBrowser() {
  const candidates = [
    process.env.MP_BROWSER,
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    '/usr/bin/microsoft-edge',
    '/usr/bin/google-chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  ].filter(Boolean);
  const found = candidates.find((p) => existsSync(p));
  if (!found) throw new Error('No Edge or Chrome found. Set MP_BROWSER to its path.');
  return found;
}

async function freePort() {
  return new Promise((res, rej) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => res(port));
    });
    s.on('error', rej);
  });
}

/** Serve apps/web/dist (or `dir`) with the repository's static server. */
export async function startServer(dir = join(ROOT, 'apps', 'web', 'dist'), port = 0) {
  port = port || (await freePort());
  const py = process.env.MP_PYTHON || 'python';
  const child = spawn(py, [join(ROOT, 'tools', 'serve-web.py'), '--dir', dir, '--port', String(port)], { stdio: 'ignore' });
  for (let i = 0; i < 50; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/index.html`);
      if (r.ok) break;
    } catch (_) { /* not up yet */ }
    await sleep(100);
  }
  return { port, url: `http://127.0.0.1:${port}/`, stop: () => child.kill() };
}

export class Page {
  constructor(ws, browser) {
    this.ws = ws;
    this.browser = browser;
    this.id = 0;
    this.waiting = new Map();
    this.handlers = new Map();
    this.console = [];
    this.errors = [];
    this.downloads = [];
    ws.addEventListener('message', (m) => this.#onMessage(JSON.parse(m.data)));
  }

  #onMessage(msg) {
    if (msg.id) {
      const w = this.waiting.get(msg.id);
      if (w) {
        this.waiting.delete(msg.id);
        if (msg.error) w.reject(new Error(`${w.method}: ${msg.error.message}`));
        else w.resolve(msg.result);
      }
      return;
    }
    for (const h of this.handlers.get(msg.method) || []) h(msg.params, msg.sessionId);
  }

  on(method, fn) {
    if (!this.handlers.has(method)) this.handlers.set(method, []);
    this.handlers.get(method).push(fn);
  }

  send(method, params = {}, timeout = 60000) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => {
        this.waiting.delete(id);
        reject(new Error(`timeout: ${method}`));
      }, timeout);
      this.waiting.set(id, {
        method,
        resolve: (v) => { clearTimeout(t); resolve(v); },
        reject: (e) => { clearTimeout(t); reject(e); },
      });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async eval(expression, timeout = 60000) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, timeout);
    if (r.exceptionDetails) {
      throw new Error(`JS: ${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
    }
    return r.result.value;
  }

  async waitFor(expression, seconds = 30, what = '') {
    const end = Date.now() + seconds * 1000;
    let last;
    while (Date.now() < end) {
      try {
        const v = await this.eval(expression);
        if (v) return v;
      } catch (e) { last = e; }
      await sleep(150);
    }
    throw new Error(`timed out (${seconds}s) waiting for: ${what || expression}${last ? ` (${last.message})` : ''}`);
  }

  async goto(url) {
    await this.send('Page.navigate', { url });
    await this.waitFor("document.readyState === 'complete'", 30, 'page load');
  }

  /** The visible element's box as [x, y, w, h] for a selector (and a text filter). */
  async rect(selector, text = null, index = 0) {
    return this.eval(`(async () => {
      const want = ${JSON.stringify(text)};
      const els = [...document.querySelectorAll(${JSON.stringify(selector)})].filter((e) => {
        const r = e.getBoundingClientRect();
        const cs = getComputedStyle(e);
        return r.width > 0 && r.height > 0 && cs.visibility !== 'hidden' && cs.display !== 'none';
      });
      const pick = want ? els.filter((e) => new RegExp(want, 'i').test((e.innerText || e.value || e.title || e.getAttribute('aria-label') || '').trim())) : els;
      const el = pick[${index}];
      if (!el) return null;
      el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      let r = el.getBoundingClientRect();
      // Something fixed over it (the dock, a toast)? Bring it to the middle of the screen.
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      if (hit && !el.contains(hit) && !hit.contains(el)) {
        el.scrollIntoView({ block: 'center', inline: 'nearest' });
        r = el.getBoundingClientRect();
        // A toast still in the way (on a small screen it is wide): let it go.
        const over = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        if (over && over.closest('.toasts') && !el.closest('.toasts')) {
          document.querySelectorAll('.toast-x').forEach((b) => b.click());
          await new Promise((done) => setTimeout(done, 500));
          r = el.getBoundingClientRect();
        }
      }
      return [r.left, r.top, r.width, r.height];
    })()`);
  }

  async mouseMove(x, y) {
    await this.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
  }

  /** A real mouse click at a point. */
  async clickAt(x, y) {
    await this.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
    await this.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
    await this.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
    await sleep(120);
  }

  /** A real click on the centre of the first visible element matching the selector (and text). */
  async click(selector, text = null, index = 0) {
    let r = await this.rect(selector, text, index);
    if (!r) throw new Error(`nothing visible matches ${selector} ${text || ''}`);
    await sleep(40);
    r = (await this.rect(selector, text, index)) || r;
    await this.clickAt(r[0] + r[2] / 2, r[1] + r[3] / 2);
    return r;
  }

  /** The real mouse wheel at a point. */
  async wheel(x, y, deltaY, steps = 6) {
    for (let i = 0; i < steps; i++) {
      await this.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: deltaY / steps });
      await sleep(30);
    }
    await sleep(250);
  }

  /** A touch tap (touch emulation must be on). */
  async tapAt(x, y) {
    await this.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y, id: 1 }] });
    await sleep(40);
    await this.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await sleep(150);
  }

  async tap(selector, text = null, index = 0) {
    const r = await this.rect(selector, text, index);
    if (!r) throw new Error(`nothing visible matches ${selector} ${text || ''}`);
    await this.tapAt(r[0] + r[2] / 2, r[1] + r[3] / 2);
    return r;
  }

  async swipe(x0, y0, x1, y1, steps = 8) {
    await this.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: x0, y: y0, id: 1 }] });
    for (let i = 1; i <= steps; i++) {
      await this.send('Input.dispatchTouchEvent', {
        type: 'touchMove',
        touchPoints: [{ x: x0 + ((x1 - x0) * i) / steps, y: y0 + ((y1 - y0) * i) / steps, id: 1 }],
      });
      await sleep(12);
    }
    await this.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await sleep(200);
  }

  /** Two fingers moving apart (scale > 1) or together. */
  async pinch(cx, cy, from, to, steps = 10) {
    const pts = (d) => [{ x: cx - d / 2, y: cy, id: 1 }, { x: cx + d / 2, y: cy, id: 2 }];
    await this.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: pts(from) });
    for (let i = 1; i <= steps; i++) {
      await this.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: pts(from + ((to - from) * i) / steps) });
      await sleep(15);
    }
    await this.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await sleep(250);
  }

  async key(key, { code = key, vk = 0, text, modifiers = 0 } = {}) {
    const base = { key, code, windowsVirtualKeyCode: vk, modifiers };
    await this.send('Input.dispatchKeyEvent', { type: 'keyDown', ...base, ...(text ? { text } : {}) });
    await this.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
    await sleep(120);
  }

  async type(text) {
    for (const ch of text) {
      await this.send('Input.dispatchKeyEvent', { type: 'keyDown', key: ch, text: ch });
      await this.send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch });
    }
    await sleep(100);
  }

  async shot(name) {
    const r = await this.send('Page.captureScreenshot', { format: 'png' });
    const path = join(SHOTS, `${name}.png`);
    writeFileSync(path, Buffer.from(r.data, 'base64'));
    return path;
  }

  /** Phone / tablet / desktop viewports; touch emulation for the first two. */
  async viewport(name) {
    const sizes = {
      desktop: { width: 1480, height: 940, mobile: false, scale: 1 },
      tablet: { width: 820, height: 1180, mobile: true, scale: 2 },
      phone: { width: 390, height: 844, mobile: true, scale: 3 },
    };
    const v = sizes[name];
    await this.send('Emulation.setDeviceMetricsOverride', { width: v.width, height: v.height, deviceScaleFactor: v.scale, mobile: v.mobile });
    await this.send('Emulation.setTouchEmulationEnabled', v.mobile ? { enabled: true, maxTouchPoints: 5 } : { enabled: false });
    await this.send('Emulation.setEmitTouchEventsForMouse', { enabled: false });
    this.currentViewport = name;
  }

  /** The files a file chooser should answer with next. */
  answerFileChooser(paths) {
    this.nextChooser = paths;
  }

  close() {
    try { this.ws.close(); } catch (_) { /* gone */ }
  }
}

/** Launch the browser with a fresh profile and attach to a page. */
export async function launch({ headless = true, url = 'about:blank', width = 1480, height = 940, extraArgs = [], profile = null } = {}) {
  const exe = findBrowser();
  const port = await freePort();
  const userData = profile || mkdtempSync(join(tmpdir(), 'mp-e2e-'));
  const args = [
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${userData}`,
    '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-sync',
    '--disable-features=Translate,msEdgeFRE,msEdgeSignIn,OptimizationHints',
    '--disable-background-networking', '--disable-component-update',
    '--allow-insecure-localhost',
    `--window-size=${width},${height}`,
    ...(headless ? ['--headless=new'] : []),
    ...extraArgs,
    url,
  ];
  const proc = spawn(exe, args, { stdio: 'ignore' });
  let targets;
  for (let i = 0; i < 100; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      if (targets.some((t) => t.type === 'page')) break;
    } catch (_) { /* not up yet */ }
    await sleep(150);
  }
  const target = targets.find((t) => t.type === 'page');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.addEventListener('open', res); ws.addEventListener('error', rej); });
  const page = new Page(ws, { proc, port, userData });
  page.port = port;
  page.userData = userData;
  page.proc = proc;
  await page.send('Page.enable');
  await page.send('Runtime.enable');
  await page.send('Log.enable');
  await page.send('Network.enable');
  await page.send('DOM.enable');
  page.on('Runtime.consoleAPICalled', (p) => {
    const text = p.args.map((a) => a.value ?? a.description ?? '').join(' ');
    page.console.push({ type: p.type, text });
    if (p.type === 'error') page.errors.push(text);
  });
  page.on('Runtime.exceptionThrown', (p) => page.errors.push(`exception: ${p.exceptionDetails.exception?.description || p.exceptionDetails.text}`));
  page.on('Log.entryAdded', (p) => {
    if (p.entry.level === 'error') page.errors.push(`${p.entry.source}: ${p.entry.text} ${p.entry.url || ''}`);
  });
  page.on('Page.javascriptDialogOpening', () => page.send('Page.handleJavaScriptDialog', { accept: true }).catch(() => {}));
  // Real file choosers: answer with the files the test named.
  await page.send('Page.setInterceptFileChooserDialog', { enabled: true });
  page.on('Page.fileChooserOpened', async (p) => {
    const files = page.nextChooser;
    page.nextChooser = null;
    if (!files) return;
    try {
      await page.send('DOM.setFileInputFiles', { files, backendNodeId: p.backendNodeId });
    } catch (e) {
      page.errors.push(`file chooser: ${e.message}`);
    }
  });
  return page;
}

export function killBrowser(page) {
  page.close();
  try { page.proc.kill(); } catch (_) { /* gone */ }
  setTimeout(() => {
    try { rmSync(page.userData, { recursive: true, force: true, maxRetries: 3 }); } catch (_) { /* in use */ }
  }, 1500);
}

/** A tiny test runner: named checks with a summary. */
export class Report {
  constructor() {
    this.results = [];
  }
  check(name, ok, detail = '') {
    this.results.push({ name, ok: !!ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  [${detail}]` : ''}`);
    return !!ok;
  }
  summary() {
    const failed = this.results.filter((r) => !r.ok);
    console.log(`\nSUMMARY: ${this.results.length - failed.length} passed, ${failed.length} failed`);
    for (const f of failed) console.log(`  FAILED: ${f.name} ${f.detail}`);
    return failed.length;
  }
}
