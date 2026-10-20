// The reading-state file round trip, "waiting for file", direct saving through
// the File System Access API (faked: a native dialog cannot be driven), and
// the installed app working offline.
//   node tools/web-e2e/e2e-state.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks, openBook, counter, chapterLabel, closeReader, readerSetting } from './helpers.mjs';
import { mkdtempSync, readdirSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const viewport = process.argv[2] || 'desktop';
const r = new Report();
const server = await startServer();
const dl = mkdtempSync(join(tmpdir(), 'mp-state-'));
let page = await launch({ headless: true });
const pages = [page];
const noPicker = "delete window.showSaveFilePicker; delete window.showOpenFilePicker;";

try {
  await page.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dl });
  await page.send('Page.addScriptToEvaluateOnNewDocument', { source: noPicker });
  await openApp(page, server.url, viewport);
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Test.cbz')]);
  await openBook(page, 'Hero Saga');
  // This test reads one page at a time.
  await readerSetting(page, '#reader-mode-paged', viewport !== 'desktop');
  await sleep(600);
  // Read to chapter 3, page 4.
  await page.eval("window.__mp.emit({ t: 'key', key: ']', shift: false, ctrl: false })");
  await sleep(300);
  for (let i = 0; i < 3; i++) await page.eval("window.__mp.emit({ t: 'key', key: 'ArrowRight', shift: false, ctrl: false })");
  await sleep(300);
  const where = `${await chapterLabel(page)} ${await counter(page)}`;
  r.check('read to Ch. 3 page 4', where === 'Ch. 3 4 / 8', where);
  await closeReader(page);

  // ---- save (a browser without the picker downloads the file) ----
  await nav(page, 'Settings');
  await page.click('#settings-nav-state');
  await page.shot(`${viewport}-40-settings`);
  await clearToasts(page);
  await page.click('#state-save');
  await sleep(2000);
  const saved = readdirSync(dl).filter((f) => f.endsWith('.mpstate'));
  r.check('Save reading state downloads a .mpstate file', saved.length === 1, saved.join());
  const text = readFileSync(join(dl, saved[0]), 'utf8');
  const doc = JSON.parse(text);
  r.check('the file names the format and the book that was read', doc.format === 'mangapoint-state' && doc.books.length === 1, `${doc.books.length} books (only books that were opened have a place to remember)`);
  const hero = doc.books.find((b) => /Hero/.test(b.title));
  r.check('it holds the place: chapter index 1, page 3', hero && hero.position.chapter === 1 && hero.position.page === 3, JSON.stringify(hero && hero.position));
  r.check('it holds chapters read', hero && hero.chaptersRead.includes(0), JSON.stringify(hero && hero.chaptersRead));
  r.check('it holds no pictures, only identity (small)', text.length < 5000, `${text.length} bytes`);

  // ---- a different browser: nothing there, load the file ----
  killBrowser(page);
  page = await launch({ headless: true });
  pages.push(page);
  await page.send('Page.addScriptToEvaluateOnNewDocument', { source: noPicker });
  await openApp(page, server.url, viewport);
  await nav(page, 'Settings');
  await page.click('#settings-nav-state');
  page.answerFileChooser([join(dl, saved[0])]);
  await clearToasts(page);
  await page.click('#state-load');
  await page.waitFor("document.querySelector('.toast-text')?.textContent.includes('Loaded')", 10, 'the loaded toast');
  const toast = await page.eval("document.querySelector('.toast-text').textContent");
  r.check('loading says what happened', /1 book waiting/.test(toast), toast);
  await nav(page, 'Library');
  await page.waitFor("!!document.querySelector('#waiting')", 10, 'waiting list');
  const waiting = await page.eval("[...document.querySelectorAll('.waiting-row')].map(e => e.innerText.replace(/\\s+/g,' '))");
  r.check('the book is listed as waiting for the file', waiting.length === 1 && /Hero Saga/.test(waiting.join()), waiting.join(' || ').slice(0, 200));
  await page.shot(`${viewport}-41-waiting`);

  // ---- import the matching file: it resumes ----
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await sleep(500);
  const left = await page.eval("document.querySelectorAll('.waiting-row').length");
  r.check('the imported book left the waiting list', left === 0, `${left} waiting`);
  const label = await page.eval("document.querySelector('.poster-read')?.textContent.trim()");
  r.check('Library offers Continue at Ch. 3', /Continue.*Ch\. 3/.test(label || ''), label);
  await page.click('.poster-read');
  await page.waitFor("!!document.querySelector('#reader')", 10, 'reader');
  await sleep(600);
  const there = `${await chapterLabel(page)} ${await counter(page)}`;
  r.check('and reopens at the saved page', there === 'Ch. 3 4 / 8', there);
  await closeReader(page);

  // ---- direct saving with the File System Access API (a fake, backed by OPFS) ----
  await page.eval(`(() => {
    window.__pickerCalls = 0;
    window.showSaveFilePicker = async (o) => {
      window.__pickerCalls++;
      const root = await navigator.storage.getDirectory();
      return root.getFileHandle('chosen.mpstate', { create: true });
    };
  })()`);
  await nav(page, 'Settings');
  await page.click('#settings-nav-state');
  await page.waitFor("!!document.querySelector('#state-save')", 5, 'settings');
  await clearToasts(page);
  await page.click('#state-save');
  await page.waitFor("document.querySelector('#state-file')", 10, 'the saved-file note');
  const calls1 = await page.eval('window.__pickerCalls');
  await clearToasts(page);
  await page.click('#state-save');
  await sleep(1200);
  const calls2 = await page.eval('window.__pickerCalls');
  const content = await page.eval(`(async () => { const root = await navigator.storage.getDirectory(); const f = await (await root.getFileHandle('chosen.mpstate')).getFile(); return await f.text(); })()`);
  r.check('first save asks where, the second writes straight to the file', calls1 === 1 && calls2 === 1, `picker calls ${calls1} then ${calls2}`);
  r.check('the chosen file holds valid state', JSON.parse(content).books.length === 1);
  await page.shot(`${viewport}-42-state-saved`);

  // ---- offline: the installed app opens with the server gone ----
  await page.goto(server.url);
  await page.waitFor("!!document.querySelector('.nav')", 20, 'app');
  const ready = await page.waitFor("navigator.serviceWorker.controller !== null || (async () => !!(await navigator.serviceWorker.getRegistration()))()", 20, 'service worker').catch(() => false);
  await page.waitFor("(async () => { const k = await caches.keys(); return k.length > 0; })()", 20, 'the cache');
  await page.eval("navigator.serviceWorker.ready.then(() => true)");
  await page.goto(server.url); // controlled load
  await page.waitFor("navigator.serviceWorker.controller !== null", 15, 'controlled page');
  const cached = await page.eval("(async () => { const c = await caches.open((await caches.keys())[0]); return (await c.keys()).length; })()");
  r.check('the app shell is cached by the service worker', cached >= 25, `${cached} files`);
  server.stop();
  await sleep(600);
  await page.send('Network.emulateNetworkConditions', { offline: true, latency: 0, downloadThroughput: 0, uploadThroughput: 0 });
  await page.send('Page.reload');
  await page.waitFor("!!document.querySelector('.nav')", 30, 'app offline');
  await sleep(800);
  r.check('the app reloads with the server stopped and the network off', true);
  const titlesOff = await page.eval("[...document.querySelectorAll('.poster-title')].map(e => e.textContent)");
  r.check('the library is there offline', titlesOff.length === 1 && /Hero/.test(titlesOff[0]), JSON.stringify(titlesOff));
  await page.click('.poster-read');
  await page.waitFor("!!document.querySelector('#reader')", 10, 'reader offline');
  await page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'a page offline');
  r.check('reading works offline', true);
  await page.shot(`${viewport}-43-offline`);
  r.check('console free of errors', page.errors.filter((e) => !/Failed to load resource|net::ERR/.test(e)).length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-state-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  for (const p of pages) killBrowser(p);
  server.stop();
  process.exit(failed ? 1 : 0);
}
