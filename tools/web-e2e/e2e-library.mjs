// The Library: picking several books (the bar at the bottom), Forget, and the strip source in Export.
//   node tools/web-e2e/e2e-library.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks, titles } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const touch = viewport !== 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const press = async (selector, text = null, index = 0) => (touch ? page.tap(selector, text, index) : page.click(selector, text, index));
const text = (selector) => page.eval(`document.querySelector(${JSON.stringify(selector)})?.textContent.trim() ?? null`);

try {
  await openApp(page, server.url, viewport);
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Webtoon.cbz')]);
  await clearToasts(page);
  r.check('two books in the library', (await titles(page)).length === 2, JSON.stringify(await titles(page)));

  // ---- picking several books: the app's tick on each card, and its action bar ----
  r.check('no Select button: ticking a card is how a selection starts, as in the app', !(await page.eval("!!document.querySelector('#select-button')")));
  r.check('no bar until a card is ticked', !(await page.eval("!!document.querySelector('.action-bar')")));
  r.check('the cards in a row line up at the top', await page.eval("(() => { const b = [...document.querySelectorAll('.poster')].map((p) => p.getBoundingClientRect()); return b.length > 1 && (b[1].left <= b[0].left || Math.round(b[1].top) === Math.round(b[0].top)); })()"));
  await press('.poster-check', null, 0);
  await page.waitFor("!!document.querySelector('.action-bar')", 5, 'the bar');
  r.check('ticking a card shows the bar with it counted', /^1 selected/.test(await text('#bar-count')), await text('#bar-count'));
  const bar = await page.rect('.action-bar');
  const vh = await page.eval('innerHeight');
  const navBox = await page.rect('.nav');
  const tabBar = navBox[2] >= (await page.eval('innerWidth')) - 2; // a row of tabs along the bottom, not a side menu
  const aboveNav = tabBar ? bar[1] + bar[3] <= navBox[1] + 2 : true;
  r.check('the bar sits at the bottom of the screen' + (tabBar ? ', above the tab bar' : ''), bar[1] + bar[3] >= vh - (tabBar ? navBox[3] + 6 : 6) && aboveNav, `bar ${Math.round(bar[1])}-${Math.round(bar[1] + bar[3])} of ${vh}`);
  r.check('the bar does not run past the sides of the screen', bar[0] >= -1 && bar[0] + bar[2] <= (await page.eval('innerWidth')) + 1, JSON.stringify(bar));
  await page.shot(`${viewport}-50-select`);
  r.check('one book can be exported', !!(await page.eval("document.querySelector('#bar-export')")));
  await page.eval("document.querySelectorAll('.poster-art')[1].scrollIntoView({ block: 'center' })");
  await sleep(200);
  // While something is ticked, a click on a card ticks it instead of opening it.
  await press('.poster-art', null, 1);
  await sleep(300);
  r.check('a click on another card adds it', /^2 selected/.test(await text('#bar-count')), await text('#bar-count'));
  r.check('Export is for one book at a time', !(await page.eval("!!document.querySelector('#bar-export')")));
  r.check('with every card ticked the button says Select none', /Select none/.test(await text('#bar-select-all')), await text('#bar-select-all'));
  await press('#bar-select-all');
  await sleep(300);
  r.check('Select none empties the pick and the bar goes', !(await page.eval("!!document.querySelector('.action-bar')")));
  await press('.poster-check', null, 0);
  await sleep(200);
  await press('#bar-select-all');
  await sleep(200);
  r.check('Select all ticks every card', /^2 selected/.test(await text('#bar-count')), await text('#bar-count'));
  await press('#bar-more');
  await press('#bar-forget');
  await page.waitFor("!!document.querySelector('#modal-confirm')", 5, 'the confirmation');
  await page.shot(`${viewport}-51-forget-many`);
  await press('#modal-confirm');
  await page.waitFor("!document.querySelector('.poster')", 15, 'the books to go');
  r.check('Forget took both books out of the library', (await titles(page)).length === 0);
  r.check('and left no waiting card behind', !(await page.eval("!!document.querySelector('.poster.not-installed')")));
  r.check('the bar is gone', !(await page.eval("!!document.querySelector('.action-bar')")));

  // ---- Remove download keeps the book, Forget takes it away ----
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await clearToasts(page);
  await press('.poster-more');
  r.check('the card menu has Remove download and Forget, with a question mark by Forget',
    !!(await page.eval("document.querySelector('#remove-download')")) && !!(await page.eval("document.querySelector('.poster-menu .forget-item .help')")));
  const helpText = await page.eval("document.querySelector('.poster-menu .forget-item .help')?.dataset.tip || ''");
  r.check('the question mark says what Forget does', /takes the book out of your library/i.test(helpText) && /remove the download/i.test(helpText), helpText);
  await page.click('.poster-menu .forget-item .help');
  await sleep(200);
  r.check('the question mark does not forget the book', !(await page.eval("!!document.querySelector('#modal-confirm')")) && !!(await page.eval("document.querySelector('.poster-menu')")));
  await press('#remove-download');
  await page.waitFor("!!document.querySelector('#modal-confirm')", 5, 'the confirmation');
  await press('#modal-confirm');
  await page.waitFor("!!document.querySelector('.poster.not-installed')", 15, 'the card to wait for its file');
  r.check('Remove download leaves the book in the library, not installed, even though it was never read',
    (await page.eval("document.querySelectorAll('.poster').length")) === 1 && /Not installed yet/.test(await page.eval("document.querySelector('.poster').innerText")));
  r.check('its file is gone from storage', (await page.eval("(async () => { const root = await navigator.storage.getDirectory(); try { const d = await root.getDirectoryHandle('books'); let n = 0; for await (const _ of d.keys()) n++; return n; } catch (_) { return 0; } })()")) === 0);
  await page.click('.poster-more');
  await page.click('.poster-menu .menu-item', 'Forget');
  await page.waitFor("!!document.querySelector('#modal-confirm')", 5, 'the confirmation');
  await page.click('#modal-confirm');
  await page.waitFor("!document.querySelector('.poster')", 15, 'the card to go');
  r.check('Forget on that card takes it away', !(await page.eval("!!document.querySelector('.poster')")));

  // ---- Forget from one card ----
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await clearToasts(page);
  await press('.poster-more');
  await press('.poster-menu .menu-item', 'Forget');
  await page.waitFor("!!document.querySelector('#modal-confirm')", 5, 'the confirmation');
  await press('#modal-confirm');
  await page.waitFor("!document.querySelector('.poster')", 15, 'the book to go');
  r.check('Forget in a card menu removes the book, and its place, completely', !(await page.eval("!!document.querySelector('.poster, #waiting')")));
  await nav(page, 'Settings');
  r.check('Settings has no Reading state part', !(await page.eval("!!document.querySelector('#settings-nav-state')")));
  r.check('the reading state file is under Data, in Your library', !!(await page.rect('#state-save')));

  // ---- the App part of Settings ----
  await page.click('#settings-nav-app');
  await sleep(300);
  const links = await page.eval("JSON.stringify(['macos', 'windows', 'linux'].map((s) => { const a = document.querySelector('#download-' + s); return a && [a.textContent.trim().replace(/\\s+/g, ' '), a.href]; }))");
  const found = JSON.parse(links);
  r.check('downloads for macOS, Windows and Linux', found.every((l) => l && /github\.com\/W1seGit\/MangaPoint\/releases/.test(l[1])), links);
  r.check('the GitHub link is under them', /github\.com\/W1seGit\/MangaPoint$/.test(await page.eval("document.querySelector('#github-link')?.href || ''")));
  const head = await page.eval("(() => { const h = document.querySelector('.desk-title'); return h ? [h.textContent, getComputedStyle(h).textAlign] : null; })()");
  r.check('a centred heading: Get the desktop app', head && /Get the desktop app/i.test(head[0]) && head[1] === 'center', JSON.stringify(head));
  r.check('no paragraph of explanation above it', !(await page.eval("!!document.querySelector('.desk-intro')")));
  r.check('the features are a slider with a dot each', (await page.eval("document.querySelectorAll('.desk-slide').length")) === 1 && (await page.eval("document.querySelectorAll('.desk-dot').length")) === 6);
  const slideTitle = () => page.eval("document.querySelector('#desk-slide-title')?.textContent");
  const t0 = await slideTitle();
  await page.eval("document.querySelectorAll('.desk-dot')[3].scrollIntoView({ block: 'center' })");
  await press('.desk-dot', null, 3);
  await sleep(300);
  const t3 = await slideTitle();
  r.check('a dot shows its feature', t3 && t3 !== t0 && (await page.eval("document.querySelectorAll('.desk-dot')[3].classList.contains('on')")), `${t0} -> ${t3}`);
  if (touch) {
    const sr = await page.rect('#desk-slide');
    await page.swipe(sr[0] + sr[2] * 0.8, sr[1] + sr[3] * 0.4, sr[0] + sr[2] * 0.2, sr[1] + sr[3] * 0.4);
    await sleep(300);
  } else {
    await page.click('#desk-next');
    await sleep(300);
  }
  r.check(touch ? 'a swipe goes to the next feature' : 'the arrow goes to the next feature', (await page.eval("document.querySelectorAll('.desk-dot')[4].classList.contains('on')")), await slideTitle());
  await page.shot(`${viewport}-52-app`);

  // ---- the strip source in Export ----
  await nav(page, 'Library');
  await importBooks(page, [fx('Color Webtoon.cbz')]);
  await nav(page, 'Export');
  await page.waitFor("document.querySelectorAll('.editor-row img').length > 1", 60, 'the converted pages');
  if (!touch) {
    await page.waitFor("document.querySelectorAll('.source-cell img').length > 1", 60, 'the source of the strip');
    r.check('a long strip shows its source beside the pages (not "Preparing source" for ever)', !(await page.eval("document.querySelector('.source-loading')")), await page.eval("document.querySelectorAll('.source-cell img').length + ' pictures'"));
  }
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-library-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
