// A big library: several multi-GB books imported into the browser, read, exported from, reloaded.
//   TEMP=D:\x MP_FIXTURES=D:\fx MP_SHOTS=D:\shots node tools/web-e2e/e2e-shelf10.mjs [viewport]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, nav, importBooks, openBook, titles, counter, closeReader, clearToasts } from './helpers.mjs';
import { readdirSync } from 'node:fs';

const viewport = process.argv[2] || 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const heap = async () => (await page.send('Performance.getMetrics')).metrics.find((x) => x.name === 'JSHeapUsedSize').value / 1048576;
const usage = () => page.eval("navigator.storage.estimate().then(e => ({ used: e.usage / 1073741824, quota: e.quota / 1073741824 }))");
const books = ['Tower_of_Dawn', 'Moonlit_Duel', 'Iron_Garden', 'Glass_Harbor'].map((n) => fx(n + '.cbz'));
try {
  await page.send('Performance.enable');
  await openApp(page, server.url, viewport);
  await page.shot(`${viewport}-01-empty-library`);
  const u0 = await usage();
  console.log(`browser allows ${u0.quota.toFixed(0)} GB for this site`);
  let n = 0;
  for (const f of books) {
    const t0 = Date.now(), h0 = await heap();
    const text = await importBooks(page, [f], { keep: false });
    if (n === 0) await page.shot(`${viewport}-02-import-review`);
    await page.click('#import-add');
    await page.waitFor("!document.querySelector('#import-dialog')", 900, 'import to finish');
    await sleep(500);
    n++;
    const secs = (Date.now() - t0) / 1000, h1 = await heap(), u = await usage();
    console.log(`#${n} ${f.split(/[\/]/).pop()}: ${secs.toFixed(0)} s, heap ${h0.toFixed(0)}->${h1.toFixed(0)} MB, browser storage ${u.used.toFixed(1)} GB`);
    r.check(`imported book ${n}`, (await titles(page)).length === n);
    r.check(`import ${n} kept the JS heap small`, h1 - h0 < 200, `${h0.toFixed(0)} -> ${h1.toFixed(0)} MB`);
  }
  const u1 = await usage();
  r.check('browser really holds about 10 GB', u1.used > 9, `${u1.used.toFixed(1)} GB used of ${u1.quota.toFixed(0)}`);
  await clearToasts(page);
  await page.shot(`${viewport}-03-library-10gb`);

  // Reading the first book
  await openBook(page, 'Tower');
  await page.shot(`${viewport}-04-reader-paged`);
  for (let i = 0; i < 6; i++) { await page.eval("document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}))"); await sleep(250); }
  await page.shot(`${viewport}-05-reader-page7`);
  const c1 = await counter(page);
  console.log('counter', c1);
  // show the bar, then jump to the middle through the chapter picker
  const [sx, sy, sw, sh] = await page.rect('#reader-stage');
  await page.clickAt(sx + sw / 2, sy + sh * 0.55);
  await sleep(400);
  await page.shot(`${viewport}-05b-reader-bar`);
  await page.click('#reader-chapter-picker');
  await sleep(300);
  await page.shot(`${viewport}-06-chapter-picker`);
  await page.eval("[...document.querySelectorAll('.reader-chapter-item')].find((x) => /^Ch. 40$/.test(x.textContent))?.scrollIntoView({ block: 'center' })");
  await sleep(300);
  await page.shot(`${viewport}-06b-chapter-picker-scrolled`);
  await page.click('.reader-chapter-item', 'Ch. 40');
  await sleep(1500);
  const c2 = await counter(page);
  r.check('jumped to a middle chapter instantly', c2 !== c1, `${c1} -> ${c2}`);
  await page.shot(`${viewport}-07-reader-chapter40`);
  // scroll mode for a stretch
  await page.eval("document.querySelector('#reader-view-button')?.click()"); await sleep(300);
  await page.eval("document.querySelector('#reader-mode-scroll')?.click()"); await sleep(300);
  await page.eval("document.querySelector('.menu-scrim')?.click()"); await sleep(1500);
  for (let i = 0; i < 30; i++) { await page.eval("document.querySelector('#reader-stage').scrollBy(0, 1500)"); await sleep(150); }
  await sleep(800);
  await page.shot(`${viewport}-08-reader-scroll`);
  const during = JSON.parse(await page.eval("window.mp.call('debug','{}')")).urls;
  await sleep(2500);
  const urls = JSON.parse(await page.eval("window.mp.call('debug','{}')")).urls;
  r.check('live blob URLs settle to a handful once scrolling stops', urls <= 12, `${during} while flinging, ${urls} after`);
  r.check('heap flat while reading', (await heap()) < 400, `${(await heap()).toFixed(0)} MB`);
  await sleep(2500); // let the debounced progress save
  await closeReader(page);

  // Reload: the library and the place must still be there; then Continue
  await page.goto(server.url);
  await page.waitFor("!!document.querySelector('.nav')", 30, 'restart');
  await sleep(1200);
  r.check('all four books are still there after a reload', (await titles(page)).length === 4);
  await page.shot(`${viewport}-09-library-after-reload`);
  const u2 = await usage();
  console.log(`after reload storage ${u2.used.toFixed(1)} GB`);

  // Settings storage card
  await nav(page, 'Settings');
  await sleep(800);
  await page.shot(`${viewport}-10-settings-storage`);

  // Export a 3-chapter book of the last one (the smallest)
  await nav(page, 'Export');
  await page.waitFor("!!document.querySelector('#export-device')", 10, 'export');
  await page.eval("(() => { const s = document.querySelector('#export-book'); const o = [...s.options].find(o => /Glass/.test(o.textContent)); s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await sleep(600);
  // leave only the first two chapters ticked
  await page.eval("(() => { const ls=[...document.querySelectorAll('#export-chapters input[type=checkbox]')]; ls.forEach((c,i)=>{ if (c.checked !== (i<2)) c.click(); }); })()");
  await sleep(300);
  await page.shot(`${viewport}-11-export-setup`);
  await page.click('#export-start');
  const t1 = Date.now();
  await page.waitFor("!!document.querySelector('#export-done, #export-error')", 600, 'export');
  const err = await page.eval("document.querySelector('#export-error')?.innerText || ''");
  r.check('export of two chapters from a 1.4 GB book finished', !err && await page.eval("!!document.querySelector('#export-done')"), err);
  console.log(`export took ${((Date.now() - t1) / 1000).toFixed(0)} s, heap ${(await heap()).toFixed(0)} MB`);
  await page.shot(`${viewport}-12-export-done`);
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
