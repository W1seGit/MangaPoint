// Reading: keys, taps (mouse or touch), the strip, progress that
// survives a reload, Continue, and removing a book.
//   node tools/web-e2e/e2e-read.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, nav, importBooks, titles, openBook, counter, chapterLabel, pageNo, hScroll, closeReader } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const touch = viewport !== 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });

const stage = async () => {
  const [x, y, w, h] = await page.rect('#reader-stage');
  return { x, y, w, h };
};
// A tap in the left / middle / right third of the stage, by mouse or finger.
const tapZone = async (zone) => {
  const s = await stage();
  const fx_ = zone === 'left' ? 0.12 : zone === 'right' ? 0.88 : 0.5;
  const px = s.x + s.w * fx_;
  const py = s.y + s.h * 0.55;
  if (touch) await page.tapAt(px, py);
  else await page.clickAt(px, py);
  await sleep(250);
};
const barShown = () => page.eval("document.querySelector('#reader').classList.contains('show-bar')");
const showBar = async () => { if (!(await barShown())) await tapZone('middle'); };
const hideBar = async () => { if (await barShown()) await tapZone('middle'); };
const imgLoaded = () => page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'the page picture');

try {
  await openApp(page, server.url, viewport);
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Webtoon.cbz')]);
  r.check('two books in the library', (await titles(page)).length === 2, JSON.stringify(await titles(page)));
  await page.shot(`${viewport}-10-library`);

  // ---- open and scroll through ----
  const id = await openBook(page, 'Hero Saga');
  await imgLoaded();
  r.check('a book opens as one long page to scroll', !!(await page.eval("document.querySelector('#reader-strip')")));
  r.check('there is no page-by-page mode to switch to', !(await page.eval("!!document.querySelector('#reader-mode-paged, #reader-mode-scroll, #reader-dir-rtl')")));
  await showBar();
  r.check('reader opened on page 1', (await counter(page)) === '1 / 8', await counter(page));
  r.check('first chapter is Ch. 2', /Ch\. 2/.test(await chapterLabel(page)), await chapterLabel(page));
  await page.shot(`${viewport}-11-reader-bar`);
  const hs = await hScroll(page);
  r.check('no sideways scrolling in the reader', hs.doc === 0 && hs.body === 0, JSON.stringify(hs));
  const scrollTop = () => page.eval("document.querySelector('#reader-stage').scrollTop");

  if (!touch) {
    await sleep(1500); // the strip is still being put at the top
    await page.key('ArrowDown', { code: 'ArrowDown', vk: 40 });
    await sleep(500);
    r.check('ArrowDown scrolls the strip', (await scrollTop()) > 20, `scrollTop ${await scrollTop()}`);
    await page.key('End', { code: 'End', vk: 35 });
    await sleep(1500);
    r.check('End goes to the last page of the chapter', (await counter(page)) === '8 / 8', await counter(page));
    await page.key('Home', { code: 'Home', vk: 36 });
    await sleep(1500);
    r.check('Home goes back to the top', (await scrollTop()) < 5, `scrollTop ${await scrollTop()}`);
  }
  // A tap anywhere, even beside the page, shows or hides the bars.
  for (const zone of ['right', 'left', 'middle']) {
    const was = await barShown();
    await tapZone(zone);
    r.check(`a tap in the ${zone} third toggles the bar`, (await barShown()) !== was, `${was} -> ${await barShown()}`);
  }
  await showBar();

  // ---- pinch (touch) ----
  if (touch) {
    const s = await stage();
    // Pinch to zoom in.
    const z0 = await page.eval("document.querySelector('#reader').style.getPropertyValue('--zoom')");
    await page.pinch(s.x + s.w / 2, s.y + s.h / 2, 80, 220);
    const z1 = await page.eval("document.querySelector('#reader').style.getPropertyValue('--zoom')");
    r.check('pinch changes the zoom', parseFloat(z1) > parseFloat(z0) + 0.2, `${z0} -> ${z1}`);
    await page.shot(`${viewport}-13-pinched`);
    // Back to 100%.
    await page.eval("window.__mp.emit({ t: 'zoom', pct: 100 })");
    await sleep(300);
  }

  // ---- chapter picker and the end of a chapter ----
  await showBar();
  await page.click('#reader-chapter-picker');
  await sleep(300);
  await page.shot(`${viewport}-14-chapter-menu`);
  await page.click('.reader-chapter-item', 'Ch\\. 4');
  await sleep(700);
  r.check('chapter picker jumps to Ch. 4', /Ch\. 4/.test(await chapterLabel(page)) && (await counter(page)) === '1 / 8', `${await chapterLabel(page)} ${await counter(page)}`);
  const s0 = await stage();
  const live0 = await page.eval("document.querySelectorAll('.reader-tile img').length");
  if (touch) await page.swipe(s0.x + s0.w / 2, s0.y + s0.h * 0.8, s0.x + s0.w / 2, s0.y + s0.h * 0.2);
  else await page.wheel(s0.x + s0.w / 2, s0.y + s0.h / 2, 900);
  await sleep(700);
  r.check('the strip scrolls', (await scrollTop()) > 100, `scrollTop ${await scrollTop()}`);
  r.check('pages are loaded only near the screen', live0 >= 1 && live0 <= 12, `${live0} images`);
  await page.shot(`${viewport}-16-scroll`);
  r.check('the page counter follows the scroll', /\d+ \/ 8/.test(await counter(page)), await counter(page));
  // To the end of the last chapter: an end card.
  await page.eval("document.querySelector('#reader-stage').scrollTo(0, document.querySelector('#reader-stage').scrollHeight)");
  const endCard = await page.waitFor("!!document.querySelector('#reader-end-back, #reader-next-chapter, .reader-end')", 10, 'the end card');
  r.check('the end of a chapter shows an end card', endCard);
  await page.shot(`${viewport}-15-end-card`);
  // Leave the strip part way down, for Continue.
  await page.eval("document.querySelector('#reader-stage').scrollTo(0, 400)");
  await sleep(800);

  // ---- back to the library: Continue, and persistence across a reload ----
  await closeReader(page);
  await page.waitFor("!!document.querySelector('.poster-read')", 10, 'poster');
  const label = await page.eval(`document.getElementById('read-${id}')?.textContent.trim()`);
  r.check('Library offers Continue with the chapter', /Continue/.test(label || ''), label);
  const barWidth = await page.eval(`document.querySelector('#read-${id} + .poster-read-progress > div')?.style.width`);
  r.check('Library shows a progress bar', !!barWidth && barWidth !== '0%', barWidth);
  await page.shot(`${viewport}-17-continue`);
  await sleep(1200); // the debounced write
  await page.send('Page.reload');
  await page.waitFor("!!document.querySelector('.poster')", 30, 'library after reload');
  await sleep(500);
  const label2 = await page.eval(`document.getElementById('read-${id}')?.textContent.trim()`);
  r.check('progress survives a reload', label2 === label, `${label} -> ${label2}`);
  await page.click(`#read-${id}`);
  await page.waitFor("!!document.querySelector('#reader')", 10, 'reader after reload');
  await imgLoaded().catch(() => {});
  await sleep(700);
  r.check('Continue reopens the same chapter', /Ch\./.test(await chapterLabel(page)), `${await chapterLabel(page)} ${await counter(page)}`);
  r.check('the scroll position was restored', (await page.eval("document.querySelector('#reader-stage').scrollTop")) > 50, `scrollTop ${await page.eval("document.querySelector('#reader-stage').scrollTop")}`);
  await page.shot(`${viewport}-18-after-reload`);
  await closeReader(page);

  // ---- remove a book ----
  await page.click(`.poster[data-book="${id}"] .poster-more`);
  await sleep(200);
  await page.click('.poster-menu .menu-item', 'Remove');
  await page.click('#modal-confirm');
  await page.waitFor(`!document.querySelector('.poster[data-book="${id}"]')`, 10, 'the book to go');
  await sleep(300);
  r.check('removing a book takes it off the library', (await titles(page)).length === 1, JSON.stringify(await titles(page)));
  await sleep(300);
  await page.send('Page.reload');
  await page.waitFor("!!document.querySelector('.poster')", 30, 'library');
  r.check('and it stays removed after a reload', (await titles(page)).length === 1);
  const opfs = await page.eval(`(async () => { const root = await navigator.storage.getDirectory(); const d = await root.getDirectoryHandle('books'); const n = []; for await (const [k] of d.entries()) n.push(k); return n.length; })()`);
  r.check('its file is gone from storage too', opfs === 1, `${opfs} folders left in OPFS`);
  await page.shot(`${viewport}-19-after-remove`);
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-read-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
