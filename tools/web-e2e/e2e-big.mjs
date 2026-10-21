// A 160-page, 1.7 GB book: imported without reading it into memory, then
// scrolled end to end while the JS heap and the number of live pictures are
// sampled. node tools/web-e2e/e2e-big.mjs [viewport]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, importBooks, openBook, titles } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const heap = async () => {
  const m = await page.send('Performance.getMetrics');
  return m.metrics.find((x) => x.name === 'JSHeapUsedSize').value / 1048576;
};
try {
  await page.send('Performance.enable');
  await openApp(page, server.url, viewport);
  const h0 = await heap();
  const t0 = Date.now();
  const text = await importBooks(page, [fx('Big Book.cbz')]);
  const secs = (Date.now() - t0) / 1000;
  console.log(`import took ${secs.toFixed(1)} s; review said: ${text.replace(/\n/g, ' | ').slice(0, 120)}`);
  const h1 = await heap();
  r.check('imported the 1.7 GB book', (await titles(page)).length === 1);
  r.check('importing did not load the book into the JS heap', h1 - h0 < 150, `heap ${h0.toFixed(0)} -> ${h1.toFixed(0)} MB`);
  const stored = await page.eval("(async () => { const b = (await window.__mp.store.dbAll('books'))[0][1]; return b.stored + ' ' + b.page_count; })()");
  r.check('it is kept in OPFS with 160 pages', /^opfs 160$/.test(stored), stored);

  await openBook(page, 'Big Book');
  await sleep(1500);
  const samples = [];
  let maxImgs = 0;
  const stage = await page.rect('#reader-stage');
  const total = await page.eval("document.querySelector('#reader-stage').scrollHeight");
  let guard = 0;
  while (guard++ < 400) {
    const at = await page.eval("(() => { const s = document.querySelector('#reader-stage'); s.scrollBy(0, s.clientHeight * 2); return s.scrollTop + s.clientHeight >= s.scrollHeight - 5; })()");
    await sleep(350);
    const info = await page.eval("({ imgs: document.querySelectorAll('.reader-tile img').length, urls: 0 })");
    const d = await page.eval("window.mp.call('debug', '{}')");
    const live = JSON.parse(d).urls;
    maxImgs = Math.max(maxImgs, info.imgs);
    if (guard % 8 === 0) samples.push({ heap: await heap(), imgs: info.imgs, urls: live });
    if (at) break;
  }
  console.log('samples', JSON.stringify(samples.map((s) => [Math.round(s.heap), s.imgs, s.urls])));
  const heaps = samples.map((s) => s.heap);
  const first = heaps.slice(0, 3).reduce((a, b) => a + b, 0) / Math.max(1, Math.min(3, heaps.length));
  const peak = Math.max(...heaps);
  r.check('reached the end of the strip', guard < 400, `${guard} steps`);
  r.check('never more than a handful of pictures in the page at once', maxImgs <= 8, `max ${maxImgs}`);
  const maxUrls = Math.max(...samples.map((s) => s.urls));
  // Pages are drawn at a comfortable width, so more of them fit in one screen than when they filled it.
  r.check('blob URLs stay few (revoked after pages leave)', maxUrls <= 20, `max ${maxUrls}`);
  r.check('JS heap stays flat while scrolling 160 pages (< 80 MB above the start)', peak - first < 80, `start ${first.toFixed(0)} MB, peak ${peak.toFixed(0)} MB`);
  await page.shot(`${viewport}-50-big-end`);
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-big-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
