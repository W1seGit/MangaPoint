// The reader's bars and settings: layout of the top and bottom bars, the arrows, the percentage,
// the full-screen reading settings with the one zoom menu, hiding on scroll and showing again,
// and the swipe from the left edge.
//   node tools/web-e2e/e2e-chrome.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, nav, importBooks, openBook, counter, chapterLabel, clearToasts, readerSetting } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const touch = viewport !== 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const press = (sel) => (touch ? page.tap(sel) : page.click(sel));
const barShown = () => page.eval("document.querySelector('#reader').classList.contains('show-bar')");
const reader = (expr) => page.eval(`(() => { const r = document.querySelector('#reader'); ${expr} })()`);
const center = async (sel) => { const [x, y, w, h] = await page.rect(sel); return { x: x + w / 2, y: y + h / 2, w, h, left: x, top: y, right: x + w, bottom: y + h }; };

try {
  await openApp(page, server.url, viewport);
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Webtoon.cbz')]);
  await clearToasts(page);
  const vw = await page.eval('window.innerWidth');
  const vh = await page.eval('window.innerHeight');

  // ---- the bars ----
  await openBook(page, 'Hero Saga');
  await page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'a page');
  // One page at a time is a choice (a book opens as one long page to scroll).
  await readerSetting(page, '#reader-mode-paged', touch);
  await sleep(600);
  await page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'a page');
  const left = await center('#reader-arrow-left');
  const right = await center('#reader-arrow-right');
  const pick = await center('#reader-chapter-picker');
  r.check('the arrows sit right beside the chapter name', left.right <= pick.left + 4 && right.left >= pick.right - 4 && pick.left - left.right < 40 && right.left - pick.right < 40, `${left.right} ${pick.left} ${pick.right} ${right.left}`);
  r.check('arrows are in the top bar', left.top < 40 && right.top < 40, `${left.top}`);
  r.check('chapter name sits in the middle of the top bar', Math.abs(pick.x - vw / 2) < 40 && pick.top < 40, `${pick.x} of ${vw}`);
  const look = await page.eval("(() => { const s = getComputedStyle(document.querySelector('#reader-chapter-picker')); return { border: s.borderTopWidth, bg: s.backgroundColor }; })()");
  r.check('the chapter name is plain text with a dropdown icon, not a button', look.border === '0px' && /rgba\(0, 0, 0, 0\)|transparent/.test(look.bg), JSON.stringify(look));
  const fade = await page.eval("[getComputedStyle(document.querySelector('.reader-top')).backgroundImage, getComputedStyle(document.querySelector('.reader-bottom')).backgroundImage, getComputedStyle(document.querySelector('.reader-top')).borderBottomWidth]");
  r.check('both bars fade into the page (gradient, no border)', /linear-gradient/.test(fade[0]) && /linear-gradient/.test(fade[1]) && fade[2] === '0px', JSON.stringify(fade).slice(0, 120));
  const title = await center('#reader-title');
  const pct = await center('#reader-percent');
  const back = await center('#reader-back');
  const gear = await center('#reader-view-button');
  r.check('title is centred at the bottom', Math.abs(title.x - vw / 2) < 40 && title.bottom > vh - 120, `${title.x} of ${vw}, bottom ${title.bottom} of ${vh}`);
  r.check('percentage is below the title', pct.top >= title.bottom - 2 && pct.bottom <= vh, `${pct.top} vs ${title.bottom}`);
  r.check('library button top left, settings top right', back.left < 40 && back.top < 40 && gear.right > vw - 40 && gear.top < 40, `${back.left},${back.top} ${vw - gear.right},${gear.top}`);
  r.check('percentage of the chapter (page 1 of 8 is 12%)', (await page.eval("document.querySelector('#reader-percent').textContent")) === '12%');
  await page.shot(`${viewport}-40-chrome`);

  // ---- arrows ----
  // The arrows are back and forward between chapters, not pages.
  await press('#reader-arrow-right');
  await sleep(400);
  r.check('the right arrow goes to the next chapter', /Ch\. 3/.test(await chapterLabel(page)) && (await counter(page)) === '1 / 8', `${await chapterLabel(page)} ${await counter(page)}`);
  await press('#reader-arrow-left');
  await sleep(400);
  r.check('the left arrow goes back a chapter', /Ch\. 2/.test(await chapterLabel(page)), await chapterLabel(page));
  await readerSetting(page, '#reader-dir-rtl', touch);
  if (!(await barShown())) { const s = await center('#reader-stage'); touch ? await page.tapAt(s.x, s.y) : await page.clickAt(s.x, s.y); await sleep(300); }
  await press('#reader-arrow-right');
  await sleep(400);
  r.check('right to left: the right arrow still goes to the next chapter', /Ch\. 3/.test(await chapterLabel(page)), await chapterLabel(page));
  await press('#reader-arrow-left');
  await sleep(400);
  await readerSetting(page, '#reader-dir-ltr', touch);

  // ---- reading settings ----
  if (!(await barShown())) { const s = await center('#reader-stage'); touch ? await page.tapAt(s.x, s.y) : await page.clickAt(s.x, s.y); await sleep(300); }
  await press('#reader-view-button');
  await page.waitFor("!!document.querySelector('#reader-settings')", 5, 'reading settings');
  await sleep(300);
  const sp = await center('#reader-settings');
  r.check('the reading settings fill the whole screen', sp.left <= 0 && sp.top <= 0 && sp.w >= vw - 1 && sp.h >= vh - 1, `${sp.w}x${sp.h} of ${vw}x${vh}`);
  r.check('rows have question-mark help', (await page.eval("document.querySelectorAll('#reader-settings .help').length")) >= 3);
  r.check('no separate width/height and zoom controls (one zoom menu)', !(await page.rect('#reader-zoom-in')) && !(await page.rect('#reader-zoom-out')) && !!(await page.rect('#reader-zoom-box')));
  await page.shot(`${viewport}-41-reading-settings`);
  const boxText = () => page.eval("document.querySelector('#reader-zoom-box').value");
  r.check('zoom shows Fit page to begin with', (await boxText()) === 'Fit page', await boxText());
  await press('#reader-zoom-toggle');
  await sleep(300);
  await page.shot(`${viewport}-42-zoom-menu`);
  await press('#reader-fit-width');
  r.check('Fit width: data-fit width', (await reader("return r.dataset.fit")) === 'width');
  r.check('the box says Fit width', (await boxText()) === 'Fit width', await boxText());
  await page.eval("document.querySelector('#reader-zoom-box').focus(); document.querySelector('#reader-zoom-box').select()");
  await page.type('150');
  await page.key('Enter', { code: 'Enter', vk: 13, text: '\r' });
  await sleep(400);
  r.check('typing 150 sets a custom zoom', (await boxText()) === '150%', await boxText());
  r.check('and the page really is zoomed', parseFloat(await reader("return r.style.getPropertyValue('--zoom')")) === 1.5);
  await press('#reader-zoom-toggle');
  await press('#reader-fit-height');
  r.check('Fit page again', (await boxText()) === 'Fit page' && (await reader("return r.dataset.fit")) === 'height');
  await press('#reader-settings-close');
  await sleep(300);
  r.check('closing returns to the page', !(await page.rect('#reader-settings')));

  // ---- hide on scroll, show on tap or scroll up (scroll mode) ----
  await press('#reader-back');
  await page.waitFor("!document.querySelector('#reader')", 10, 'library');
  await sleep(300);
  await openBook(page, 'Color Webtoon');
  await page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'a picture');
  await sleep(1500);
  const mode = await reader('return r.dataset.mode');
  if (mode !== 'scroll') await readerSetting(page, '#reader-mode-scroll', touch);
  await sleep(1500);
  const st = await center('#reader-stage');
  const scrollBy = async (dy) => { await page.eval(`document.querySelector('#reader-stage').scrollBy(0, ${dy})`); await sleep(500); };
  await scrollBy(300);
  await scrollBy(400);
  r.check('scrolling down hides the bars', !(await barShown()));
  await scrollBy(-250);
  r.check('scrolling up brings them back', await barShown());
  await page.shot(`${viewport}-43-scroll-up-shows`);
  await scrollBy(500);
  r.check('hidden again after scrolling down', !(await barShown()));
  touch ? await page.tapAt(st.x, st.y) : await page.clickAt(st.x, st.y);
  await sleep(400);
  r.check('a tap anywhere on the page shows the bars', await barShown());
  await scrollBy(500);
  const gone = await page.eval("getComputedStyle(document.querySelector('.reader-top')).opacity");
  r.check('hidden bars are really invisible', gone === '0', gone);
  r.check('percentage reflects how far down the chapter you are', /^\d+%$/.test(await page.eval("document.querySelector('#reader-percent').textContent")) && (await page.eval("document.querySelector('#reader-percent').textContent")) !== '0%');
  await page.shot(`${viewport}-44-scroll-hidden`);

  // ---- swipe from the left edge leaves the book (a finger) ----
  if (touch) {
    const s = await center('#reader-stage');
    await page.swipe(5, s.y, 230, s.y + 10);
    await sleep(500);
    r.check('swiping in from the left edge goes back to the library', !(await page.rect('#reader')) && !!(await page.rect('.poster')));
  }
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-chrome-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
