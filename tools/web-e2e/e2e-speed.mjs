// How long a page takes to show while scrolling a book of tall pictures.
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, importBooks, openBook } from './helpers.mjs';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
try {
  await openApp(page, server.url, 'desktop');
  await importBooks(page, [fx('Tall Real.cbz')]);
  await openBook(page, 'Tall Real');
  await sleep(1500);
  const st = await page.rect('#reader-stage');
  // sample every frame: how many tiles in view have no picture yet
  await page.eval(`window.__blank = 0; window.__frames = 0; (function f(){ const h = innerHeight;
    for (const t of document.querySelectorAll('.reader-tile')) { const b = t.getBoundingClientRect();
      if (b.bottom > 0 && b.top < h && !t.dataset.loaded) { window.__blank++; (window.__at ??= []).push([Math.round(performance.now()), document.querySelector("#reader-stage").scrollTop, t.dataset.page, !!t.querySelector("img"), !!t.dataset.sized]); break; } }
    window.__frames++; requestAnimationFrame(f); })()`);
  for (let i = 0; i < 40; i++) { await page.wheel(st[0] + st[2] / 2, st[1] + st[3] / 2, 150, 6); await sleep(250); }
  const blank = await page.eval('window.__blank'), frames = await page.eval('window.__frames');
  console.log(JSON.stringify(await page.eval("window.__at")));console.log(`frames with an empty tile in view: ${blank} of ${frames}`);
  r.check('a page is almost never empty in view while scrolling', blank <= 3, `${blank}/${frames}`);
} catch (e) { r.check(`finished (${e.message})`, false); }
finally { const f = r.summary(); killBrowser(page); server.stop(); process.exit(f ? 1 : 0); }
