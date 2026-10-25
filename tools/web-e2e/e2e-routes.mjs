// The address bar follows the screen (/, /export, /settings#reader, /read/<book>/<chapter>), the
// update dialog, the reading settings page and a book whose ComicInfo.xml is big.
//   node tools/web-e2e/e2e-routes.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, nav, importBooks, openBook, counter, chapterLabel, closeReader, titles } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const touch = viewport !== 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const where = () => page.eval("location.pathname + location.search + location.hash");
const goTo = async (path) => {
  await page.goto(server.url.replace(/\/$/, '') + path);
  await page.waitFor("!!document.querySelector('.nav')", 30, 'the app to start');
  await sleep(600);
};

try {
  await openApp(page, server.url, viewport);
  r.check('the library is the home: /', (await where()) === '/', await where());

  // ---- a book whose ComicInfo.xml is big still gets its chapters ----
  const text = await importBooks(page, [fx('Big ComicInfo.cbz')]);
  r.check('a big ComicInfo.xml still gives its 3 chapters', /3 chapters/.test(text), text.replace(/\n/g, ' | ').slice(0, 120));
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);

  // ---- the screens ----
  await nav(page, 'Export');
  r.check('Export is /export', (await where()) === '/export', await where());
  await nav(page, 'Settings');
  r.check('Settings is /settings#library', (await where()) === '/settings#library', await where());
  await page.click('#settings-nav-reader');
  await sleep(300);
  r.check('a part of Settings is kept after the #', (await where()) === '/settings#reader', await where());
  await page.click('#settings-nav-app');
  await sleep(300);
  r.check('another part: /settings#app', (await where()) === '/settings#app', await where());
  r.check('there is no Reading state part any more', !(await page.rect('#settings-nav-state')) && !(await page.eval("!!document.querySelector('#settings-nav-state')")));
  await page.eval('history.back()');
  await sleep(500);
  r.check('Back goes to the part before', (await where()) === '/settings#reader' && /Add (device|a reader)/.test(await page.eval("document.querySelector('.settings-content').innerText")), await where());
  await nav(page, 'Library');
  r.check('Library is / again', (await where()) === '/', await where());

  // ---- opening an address shows the screen ----
  await goTo('/export');
  r.check('opening /export shows Export', !!(await page.rect('#export-screen')) || !!(await page.eval("!!document.querySelector('#export-screen')")));
  await goTo('/settings#reader');
  r.check('opening /settings#reader shows that part', /Add (device|a reader)/.test(await page.eval("document.querySelector('.settings-content')?.innerText || ''")), await where());
  await goTo('/settings#library');
  r.check('the reading state file is in the library part, under Data', !!(await page.rect('#state-save')) && !!(await page.rect('#danger-card')), await where());
  await goTo('/nowhere');
  r.check('an unknown address shows the library', (await titles(page)).length >= 1);

  // ---- the book being read ----
  await goTo('/');
  const id = await openBook(page, 'Hero Saga');
  r.check('reading is /read?book=...&chapter=...', (await where()) === `/read?book=${id}&chapter=1`, await where());
  await page.key('End', { code: 'End', vk: 35 });
  await page.waitFor("!!document.querySelector('#reader-chapter-picker')", 10, 'the bar');
  // Go to the second chapter with the picker.
  await page.eval("document.querySelector('#reader').classList.add('show-bar')");
  await page.click('#reader-chapter-picker');
  await sleep(300);
  await page.click('.reader-chapter-item', 'Ch\\. 3');
  await sleep(700);
  r.check('the chapter is in the address', (await where()) === `/read?book=${id}&chapter=2`, `${await where()} ${await chapterLabel(page)}`);
  await page.eval('location.reload()');
  await page.waitFor("!!document.querySelector('#reader')", 30, 'the reader after a reload');
  await sleep(800);
  r.check('reloading the address opens the same chapter', /Ch\. 3/.test(await chapterLabel(page)), `${await chapterLabel(page)} ${await where()}`);
  await page.eval("document.querySelector('#reader').classList.add('show-bar')");
  await page.click('#reader-back');
  await page.waitFor("!document.querySelector('#reader')", 10, 'the reader to close');
  await sleep(400);
  r.check('closing the book goes back to /', (await where()) === '/', await where());
  await openBook(page, 'Hero Saga');
  await page.eval('history.back()');
  await page.waitFor("!document.querySelector('#reader')", 10, 'Back to leave the reader');
  r.check('the Back button leaves the book', (await where()) === '/', await where());

  // ---- the reading settings page ----
  await openBook(page, 'Hero Saga');
  await page.eval("document.querySelector('#reader').classList.add('show-bar')");
  await page.click('#reader-view-button');
  await page.waitFor("!!document.querySelector('#reader-settings')", 10, 'the reading settings');
  const back = await page.rect('#reader-settings-close');
  const title = await page.rect('.reader-settings-head h2');
  const first = await page.rect('.reader-settings-body .sx-row');
  const head = await page.rect('.reader-settings-head');
  r.check('Back is on the left, with the title beside it', back[0] < title[0] && Math.abs(back[1] - title[1]) < 30, `${back} ${title}`);
  r.check('the header is as wide as the settings', Math.abs(head[2] - first[2]) < 4, `${head[2]} vs ${first[2]}`);
  r.check('Back sits above the settings', back[1] + back[3] <= first[1] + 2, `${back} ${first}`);
  r.check('no close X any more', !(await page.eval("!!document.querySelector('#reader-settings .icon-x, #reader-settings [aria-label=Close]')")));
  await page.shot(`${viewport}-40-reading-settings`);
  await page.click('#reader-settings-close');
  await sleep(300);
  r.check('Back returns to the book', !(await page.eval("!!document.querySelector('#reader-settings')")) && !!(await page.eval("!!document.querySelector('#reader')")));
  await closeReader(page);

  // ---- scrolling is scrolling: one long page whose pictures load as you go ----
  await importBooks(page, [fx('Big Book.cbz')]);
  await openBook(page, 'Big Book');
  await sleep(1200);
  r.check('a book opens as one long page to scroll', !!(await page.eval("document.querySelector('#reader-strip')")));
  const tiles = await page.eval("document.querySelectorAll('.reader-tile').length");
  const loaded = () => page.eval("[...document.querySelectorAll('.reader-tile img')].map((i) => Number(i.closest('.reader-tile').dataset.page))");
  const before = await loaded();
  r.check('only the pages near the screen are loaded', tiles >= 100 && before.length > 0 && before.length < tiles / 3, `${before.length} of ${tiles}`);
  const st = await page.rect('#reader-stage');
  await page.wheel(st[0] + st[2] / 2, st[1] + st[3] / 2, 4000);
  await sleep(1200);
  const after = await loaded();
  r.check('the wheel scrolls the page', (await page.eval("document.querySelector('#reader-stage').scrollTop")) > 1500);
  r.check('pages load as it scrolls and the ones left behind are let go', Math.max(...after) > Math.max(...before) && Math.min(...after) > Math.min(...before) && after.length < tiles / 3, `${before[0]}-${before.at(-1)} -> ${after[0]}-${after.at(-1)}`);
  await closeReader(page);

  // ---- a chapter ends where it ends; the next one is opened with a button ----
  await openBook(page, 'Hero Saga');
  await sleep(800);
  await page.eval("document.querySelector('#reader-stage').scrollTop = 1e9");
  await sleep(1200);
  await page.eval("document.querySelector('#reader-stage').scrollTop = 1e9");
  await sleep(800);
  r.check('scrolling to the end does not run into the next chapter', (await page.eval("document.querySelectorAll('.reader-chapter').length")) === 1);
  r.check('the end has a next chapter button', !!(await page.eval("!!document.querySelector('#reader-next-chapter')")));
  const was = await chapterLabel(page);
  await page.eval("document.querySelector('#reader-next-chapter').click()");
  await sleep(900);
  r.check('the next chapter button opens the next chapter', (await chapterLabel(page)) !== was && Number(/\d+/.exec(await chapterLabel(page))) > Number(/\d+/.exec(was)), `${was} -> ${await chapterLabel(page)}`);
  await page.eval("document.querySelector('#reader-stage').scrollTop = 1e9");
  await sleep(1200);
  await page.eval("document.querySelector('#reader-stage').scrollTop = 1e9");
  await sleep(600);
  r.check('and a previous chapter button goes back', !!(await page.eval("!!document.querySelector('#reader-prev-chapter')")));
  await closeReader(page);

  // ---- the update dialog ----
  await page.eval("window.__mark = 1; window.__mp.emit({ t: 'sw_update' })");
  await page.waitFor("!!document.querySelector('#update-dialog')", 10, 'the update dialog');
  await page.shot(`${viewport}-41-update`);
  await page.click('#update-later');
  await sleep(300);
  r.check('Later closes the update dialog', !(await page.eval("!!document.querySelector('#update-dialog')")));
  await page.eval("window.__mp.emit({ t: 'sw_update' })");
  await sleep(300);
  r.check('Later is kept for this visit', !(await page.eval("!!document.querySelector('#update-dialog')")));
  await page.eval('location.reload()');
  await page.waitFor("!!document.querySelector('.nav')", 30, 'the app to restart');
  await sleep(600);
  await page.eval("window.__mark = 1; window.__mp.emit({ t: 'sw_update' })");
  await page.waitFor("!!document.querySelector('#update-dialog')", 10, 'the update dialog again');
  await page.click('#update-now');
  await page.waitFor("window.__mark === undefined", 20, 'Update and reload to reload the page');
  r.check('Update and reload reloads the page', true);

  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-routes-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
