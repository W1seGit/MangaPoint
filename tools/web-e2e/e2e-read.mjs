// Reading: keys, taps (mouse or touch), swipes, scroll mode, progress that
// survives a reload, Continue, and removing a book.
//   node tools/web-e2e/e2e-read.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, nav, importBooks, titles, openBook, counter, chapterLabel, pageNo, hScroll, closeReader, readerSetting } from './helpers.mjs';

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
// The next page, the way the book reads: a key, or a swipe along the page.
const turnNext = async () => {
  const rtl = (await page.eval("document.querySelector('#reader').dataset.dir")) === 'rtl';
  if (touch) {
    const s = await stage();
    const [a, b] = rtl ? [0.2, 0.8] : [0.8, 0.2];
    await page.swipe(s.x + s.w * a, s.y + s.h * 0.5, s.x + s.w * b, s.y + s.h * 0.5);
  } else {
    await page.key(rtl ? 'ArrowLeft' : 'ArrowRight', { code: rtl ? 'ArrowLeft' : 'ArrowRight', vk: rtl ? 37 : 39 });
  }
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

  // ---- open and page through ----
  const id = await openBook(page, 'Hero Saga');
  await imgLoaded();
  r.check('reader opened on page 1', (await counter(page)) === '1 / 8', await counter(page));
  r.check('first chapter is Ch. 2', /Ch\. 2/.test(await chapterLabel(page)), await chapterLabel(page));
  await page.shot(`${viewport}-11-reader-bar`);
  const hs = await hScroll(page);
  r.check('no sideways scrolling in the reader', hs.doc === 0 && hs.body === 0, JSON.stringify(hs));

  if (!touch) {
    await page.key('ArrowRight', { code: 'ArrowRight', vk: 39 });
    r.check('ArrowRight turns to page 2 (left to right)', (await counter(page)) === '2 / 8', await counter(page));
    await page.key('ArrowLeft', { code: 'ArrowLeft', vk: 37 });
    r.check('ArrowLeft turns back', (await counter(page)) === '1 / 8', await counter(page));
    await page.key('End', { code: 'End', vk: 35 });
    r.check('End goes to the last page', (await counter(page)) === '8 / 8', await counter(page));
    await page.key('Home', { code: 'Home', vk: 36 });
    r.check('Home goes to the first page', (await counter(page)) === '1 / 8', await counter(page));
  }
  // A tap anywhere, even beside the page, shows or hides the bars; it never turns a page.
  for (const zone of ['right', 'left', 'middle']) {
    const was = await barShown();
    await tapZone(zone);
    r.check(`a tap in the ${zone} third toggles the bar`, (await barShown()) !== was, `${was} -> ${await barShown()}`);
  }
  r.check('taps did not turn a page', (await counter(page)) === '1 / 8', await counter(page));
  // Scrolling the page itself (wheel, or dragging it up) turns it; scrolling back brings the bars.
  const sg = await stage();
  const scrollOn = async (down) => {
    if (touch) await page.swipe(sg.x + sg.w / 2, sg.y + sg.h * (down ? 0.75 : 0.25), sg.x + sg.w / 2, sg.y + sg.h * (down ? 0.25 : 0.75));
    else await page.wheel(sg.x + sg.w / 2, sg.y + sg.h / 2, down ? 300 : -300);
    await sleep(700);
  };
  await scrollOn(true);
  r.check('scrolling down the page turns to page 2', (await counter(page)) === '2 / 8', await counter(page));
  r.check('scrolling down hides the bars', !(await barShown()));
  await scrollOn(false);
  r.check('scrolling up turns back', (await counter(page)) === '1 / 8', await counter(page));
  r.check('scrolling up shows the bars again', await barShown());
  await turnNext();
  await showBar();
  await page.shot(`${viewport}-12-reader-page2`);

  // ---- swipes (touch) ----
  if (touch) {
    const s = await stage();
    await page.swipe(s.x + s.w * 0.8, s.y + s.h * 0.5, s.x + s.w * 0.2, s.y + s.h * 0.5);
    r.check('swipe left turns to the next page', (await counter(page)) === '3 / 8', await counter(page));
    await page.swipe(s.x + s.w * 0.2, s.y + s.h * 0.5, s.x + s.w * 0.8, s.y + s.h * 0.5);
    r.check('swipe right goes back', (await counter(page)) === '2 / 8', await counter(page));
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

  // ---- direction: right to left ----
  await showBar();
  await readerSetting(page, '#reader-dir-rtl', touch);
  await sleep(250);
  r.check('right to left is on', (await page.eval("document.querySelector('#reader').dataset.dir")) === 'rtl');
  await hideBar();
  await turnNext();
  r.check('RTL: the next page is the other way round', (await counter(page)) === '3 / 8', await counter(page));
  if (!touch) {
    await page.key('ArrowLeft', { code: 'ArrowLeft', vk: 37 });
    r.check('RTL: ArrowLeft goes forward', (await counter(page)) === '4 / 8', await counter(page));
  }

  // ---- chapter picker and the end of a chapter ----
  await showBar();
  await page.click('#reader-chapter-picker');
  await sleep(300);
  await page.shot(`${viewport}-14-chapter-menu`);
  await page.click('.reader-chapter-item', 'Ch\\. 4');
  await sleep(500);
  r.check('chapter picker jumps to Ch. 4', /Ch\. 4/.test(await chapterLabel(page)) && (await counter(page)) === '1 / 8', `${await chapterLabel(page)} ${await counter(page)}`);
  await hideBar();
  for (let i = 0; i < 9; i++) await turnNext();
  const endCard = await page.waitFor("!!document.querySelector('#reader-end-back, #reader-next-chapter, .reader-end')", 10, 'the end card');
  r.check('the end of the last chapter shows an end card', endCard);
  await page.shot(`${viewport}-15-end-card`);
  await showBar();

  // ---- scroll mode ----
  await readerSetting(page, '#reader-mode-scroll', touch);
  await sleep(1200);
  r.check('scroll mode shows a strip', !!(await page.rect('#reader-strip')) || (await page.eval("!!document.querySelector('#reader-strip')")));
  const live0 = await page.eval("document.querySelectorAll('.reader-tile img').length");
  const s2 = await stage();
  if (touch) await page.swipe(s2.x + s2.w / 2, s2.y + s2.h * 0.8, s2.x + s2.w / 2, s2.y + s2.h * 0.2);
  else await page.wheel(s2.x + s2.w / 2, s2.y + s2.h / 2, 900);
  await sleep(600);
  const top = await page.eval("document.querySelector('#reader-stage').scrollTop");
  r.check('the strip scrolls', top > 100, `scrollTop ${top}`);
  r.check('pages are loaded only near the screen', live0 >= 1 && live0 <= 12, `${live0} images`);
  await page.shot(`${viewport}-16-scroll`);
  const c1 = await counter(page);
  r.check('scroll mode keeps a page counter', /\d+ \/ 8/.test(c1), c1);

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
