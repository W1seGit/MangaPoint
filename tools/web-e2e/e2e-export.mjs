// Export in the worker: EPUB, CBZ, XTC with a progress bar, cancel, and a look at the downloaded bytes.
//   node tools/web-e2e/e2e-export.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks, addReader } from './helpers.mjs';
import { mkdtempSync, readdirSync, readFileSync, existsSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateRawSync } from 'node:zlib';

const viewport = process.argv[2] || 'desktop';
const narrow = viewport !== 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
const dl = mkdtempSync(join(tmpdir(), 'mp-dl-'));

/** Entry names of a ZIP file on disk (central directory). */
function zipNames(buf) {
  let e = buf.length - 22;
  while (e >= 0 && buf.readUInt32LE(e) !== 0x06054b50) e--;
  const n = buf.readUInt16LE(e + 10);
  let p = buf.readUInt32LE(e + 16);
  const out = [];
  for (let i = 0; i < n; i++) {
    const nl = buf.readUInt16LE(p + 28), el = buf.readUInt16LE(p + 30), cl = buf.readUInt16LE(p + 32);
    out.push(buf.toString('utf8', p + 46, p + 46 + nl));
    p += 46 + nl + el + cl;
  }
  return out;
}
const files = () => readdirSync(dl).filter((f) => !f.endsWith('.crdownload'));
const waitFile = async (ext, secs = 30) => {
  const end = Date.now() + secs * 1000;
  while (Date.now() < end) {
    const f = files().find((x) => x.toLowerCase().endsWith(ext));
    if (f) { await sleep(300); return join(dl, f); }
    await sleep(200);
  }
  return null;
};
const runExport = async (format, { group } = {}) => {
  await clearToasts(page);
  await page.click(`#export-format-${format}`);
  if (group) await page.click(group);
  await page.click('#export-start');
  await page.waitFor("!!document.querySelector('#export-progress, #export-done, #export-error')", 20, 'export to start');
  let sawProgress = false;
  const end = Date.now() + 120000;
  while (Date.now() < end) {
    if (await page.eval("!!document.querySelector('#export-progress .progress')")) sawProgress = true;
    if (await page.eval("!!document.querySelector('#export-done, #export-error')")) break;
    await sleep(100);
  }
  return sawProgress;
};

try {
  await page.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dl });
  await openApp(page, server.url, viewport);
  // A first visit has no readers of its own: none is chosen for the visitor.
  await page.eval("document.querySelector('#nav-Settings').click()");
  await sleep(650);
  await page.click('#settings-nav-reader');
  await sleep(300);
  r.check('a first visit has no readers', (await page.eval("document.querySelectorAll('.settings-content .pills .pill:not(.add)').length")) === 0);
  // The readers this test uses, added as a visitor would.
  await addReader(page, 'xteink x3', 'Xteink X3');
  await addReader(page, 'xteink x4', 'Xteink X4(?! Pro)');
  await addReader(page, 'xteink x4 pro', 'Xteink X4 Pro');
  // Nothing imported yet: the screen still opens, on the sample pages, and cannot export them.
  await nav(page, 'Export');
  await page.waitFor("!!document.querySelector('.editor-row img')", 30, 'the sample pages');
  r.check('with no book the sample pages are shown, converted', (await page.eval("document.querySelectorAll('.editor-row img').length")) >= 4);
  r.check('the top bar: page jump, thumbnails, Compare and Overview, as in the app', !!(await page.rect('.editor-toolbar .editor-page-jump input')) && (narrow ? true : !!(await page.rect('.editor-thumbnails .editor-thumb')) && !!(await page.rect('#export-overview'))) && (narrow ? !!(await page.rect('#export-tab-original')) : !!(await page.rect('#export-compare'))));
  r.check('Compare shows the original beside each page (wide screens)', narrow || (await page.eval("document.querySelectorAll('.source-cell img').length")) >= 3);
  r.check('the sample cannot be exported', await page.eval("document.querySelector('#export-start').disabled"));
  await page.shot(`${viewport}-28-sample`);
  await nav(page, 'Library');
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Test.cbz')]);
  await clearToasts(page);
  await nav(page, 'Export');
  await page.waitFor(narrow ? "!!document.querySelector('#export-settings-tab')" : "!!document.querySelector('#export-format')", 10, 'export screen');
  // On a phone the panels are in a sheet that opens from the Settings tab; open every panel.
  const openPanels = async () => {
    if (narrow) {
      if (!(await page.eval("!!document.querySelector('#export-sheet')"))) await page.tap('#export-settings-tab');
      await sleep(400);
      await page.eval("document.querySelectorAll('.px-panel.collapsed .px-panel-head').forEach((h) => h.click())");
      await sleep(300);
    }
  };
  await openPanels();
  // A reader in the list of the Send to panel: scrolled into view, then a real click.
  const pickReader = async (pattern) => {
    const at = await page.eval(`(() => { const el = [...document.querySelectorAll('#panel-send .px-preset')].find((e) => ${pattern}.test(e.textContent)); if (!el) return null; el.scrollIntoView({ block: 'center' }); const b = el.getBoundingClientRect(); return [b.x + b.width / 2, b.y + b.height / 2]; })()`);
    if (!at) throw new Error('no reader like ' + pattern);
    await sleep(150);
    await page.clickAt(at[0], at[1]);
    await sleep(500);
  };
  await page.shot(`${viewport}-30-export`);
  if (!narrow) {
    // The bar above Series moves only the line between Edit and Series: Edit keeps 160px, and nothing above moves.
    const panel = (id) => page.eval(`(() => { const b = document.querySelector('[data-panel="${id}"]').getBoundingClientRect(); return [Math.round(b.top), Math.round(b.height)]; })()`);
    const before = [await panel('presets'), await panel('edit'), await panel('series')];
    const bar = await page.eval("(() => { const b = document.querySelector('.export-side.left .px-split[data-target=series]').getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()");
    await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: bar[0], y: bar[1] });
    await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: bar[0], y: bar[1], button: 'left', clickCount: 1 });
    for (const dy of [-60, -200, -400, -700]) {
      await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: bar[0], y: bar[1] + dy, buttons: 1 });
      await sleep(150);
    }
    await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: bar[0], y: bar[1] - 700, button: 'left', clickCount: 1 });
    await sleep(300);
    const after = [await panel('presets'), await panel('edit'), await panel('series')];
    r.check('dragging Series up makes it taller', after[2][1] > before[2][1] + 100, `${before[2][1]} -> ${after[2][1]}`);
    r.check('Edit keeps room (at least 160px) and Presets does not move', after[1][1] >= 159 && after[0][0] === before[0][0] && after[0][1] === before[0][1], JSON.stringify([before, after]));
    // Back to where it was, so the rest of the run sees the usual layout.
    await page.eval("localStorage.removeItem('mp-export-layout'); document.querySelector('[data-panel=series]').style.setProperty('--h', '150px')");
    await sleep(300);
    // A group of the Edit panel eases open: part way through it is between shut and open.
    await page.eval("window.__g = []; const g = document.querySelectorAll('.mp-group')[2]; const t0 = performance.now(); const iv = setInterval(() => { window.__g.push(Math.round(g.getBoundingClientRect().height)); if (performance.now() - t0 > 600) clearInterval(iv); }, 20)");
    await page.click('.mp-group-head', 'Crop');
    await sleep(800);
    const grow = await page.eval('window.__g');
    r.check('a group of Edit eases open instead of jumping', grow.some((h) => h > grow[0] + 10 && h < grow[grow.length - 1] - 10), JSON.stringify(grow.filter((_, i) => i % 3 === 0)));
    await page.click('.mp-group-head', 'Crop');
    await sleep(500);
  }
  const devices = await page.eval("[...document.querySelectorAll('#panel-send .px-preset .px-item-name')].map(o => o.textContent)");
  r.check('the reader list holds the readers kept in Settings (three Xteink) and the phone entry first', devices.length === 4 && /Phone or tablet/.test(devices[0]) && devices.slice(1).every((d) => /Xteink/.test(d)), devices.join(' | '));
  r.check('the panel is the app\'s Send to panel', /Send to/i.test(await page.eval("document.querySelector('#panel-send .px-panel-head').innerText")) && !!(await page.rect('#panel-send .px-new')));

  // Readers are added and edited in Settings, as in the app.
  await page.eval("document.querySelector('#panel-send .px-new').scrollIntoView({ block: 'center' })");
  await page.click('#panel-send .px-new');
  await page.waitFor("!!document.querySelector('.settings .pills')", 10, 'the readers in Settings');
  const pills = () => page.eval("[...document.querySelectorAll('.settings-content .pills .pill:not(.add)')].map((p) => p.innerText)");
  r.check('Settings lists the readers as pills, with Add device', (await pills()).length === 3 && !!(await page.rect('.settings-content .pill.add')));
  await page.click('.settings-content .pill.add');
  await sleep(500);
  await page.click('.adder-box input');
  await page.type('libra colour');
  await page.waitFor("!!document.querySelector('.adder-opt')", 10, 'a reader found');
  await page.click('.adder-opt');
  await sleep(400);
  r.check('the reader was added and is in use', (await pills()).length === 4 && /Libra/.test(await page.eval("document.querySelector('.settings-content .pill.on')?.innerText || ''")));
  r.check('the reader form is there: name, size, colour, formats, firmware, Save', !!(await page.rect('.reader-line .title-input')) && !!(await page.rect('.size-boxes')) && /Color screen/.test(await page.eval("document.querySelector('.settings-content').innerText")) && /File formats/.test(await page.eval("document.querySelector('.settings-content').innerText")) && /Firmware/.test(await page.eval("document.querySelector('.settings-content').innerText")) && !!(await page.rect('.form-footer .btn.primary')));
  r.check('no USB or Wi-Fi in the reader form', !/USB|Wi-?Fi/.test(await page.eval("document.querySelector('.settings-content').innerText")));
  // Edit: rename it, save, and see it in the list.
  await page.eval("(() => { const i = document.querySelector('.reader-line .title-input'); i.focus(); i.select(); })()");
  await page.type('Libra Test');
  r.check('Save is enabled after a change', !(await page.eval("document.querySelector('.form-footer .btn.primary').disabled")));
  // Leaving with the change not saved asks first, as the app does.
  await nav(page, 'Library');
  await page.waitFor("!!document.querySelector('#unsaved-prompt')", 5, 'the unsaved-settings question');
  r.check('leaving with unsaved reader settings asks first', /You have unsaved settings/.test(await page.eval("document.querySelector('#unsaved-prompt').innerText")));
  await page.click('#unsaved-keep');
  await sleep(200);
  r.check('Keep editing stays in the form with the change', (await page.eval("document.querySelector('.reader-line .title-input')?.value")) === 'Libra Test');
  await page.click('#settings-nav-about');
  await page.waitFor("!!document.querySelector('#unsaved-prompt')", 5, 'the question for another part of Settings');
  r.check('another part of Settings asks too', true);
  await page.click('#unsaved-keep');
  await sleep(200);
  await page.click('.form-footer .btn.primary');
  await sleep(400);
  r.check('the renamed reader is saved', (await pills()).some((n) => /Libra Test/.test(n)), (await pills()).join(' | '));
  await page.shot(`${viewport}-29-readers`);
  await nav(page, 'Export');
  await page.waitFor(narrow ? "!!document.querySelector('#export-settings-tab')" : "!!document.querySelector('#export-format')", 10, 'export screen again');
  await clearToasts(page);
  await openPanels();
  r.check('the new reader is in the Export list', (await page.eval("[...document.querySelectorAll('#panel-send .px-preset .px-item-name')].some((o) => /Libra Test/.test(o.textContent))")));
  // The first export below is on an Xteink reader.
  await pickReader('/Xteink X4 Pro/');
  r.check('the three built-in presets are offered', (await page.eval("document.querySelectorAll('#export-preset .px-preset').length")) === 3);
  r.check('the edit groups of the app are there (Layout, Spreads and zoom, Crop, Look)', (await page.eval("[...document.querySelectorAll('.mp-group-title')].map((e) => e.textContent).join()")) === 'Layout,Spreads and zoom,Crop,Look');
  r.check('no way to send to a reader, only downloads', !(await page.eval("/USB|Wi-?Fi|Folder on/i.test(document.querySelector('#panel-send').innerText)")));
  await page.waitFor("!!document.querySelector('.editor-row img')", 30, 'the converted pages');
  r.check('the pages are shown as they will be', (await page.eval("document.querySelectorAll('.editor-row img').length")) > 0);

  // The book picker: choose Hero Saga.
  await page.eval("(() => { const s = document.querySelector('#export-book'); const o = [...s.options].find(o => /Hero/.test(o.textContent)); s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await sleep(400);

  // XTC on the default Xteink reader
  let saw = await runExport('XTC');
  r.check('XTC export finished', await page.eval("!!document.querySelector('#export-done')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  r.check('a progress bar was shown while it ran', saw);
  await page.shot(`${viewport}-31-export-done`);
  await page.click('.export-download');
  const xtc = await waitFile('.xtc');
  r.check('XTC file downloaded', !!xtc, files().join(','));
  if (xtc) {
    const b = readFileSync(xtc);
    r.check('XTC header magic', b.toString('latin1', 0, 4).startsWith('XTC'), JSON.stringify(b.subarray(0, 8).toString('latin1')));
    console.log('xtc bytes', b.length, 'header', b.subarray(0, 16).toString('hex'));
  }
  await page.click('#export-again');

  // EPUB / CBZ on a colour reader
  await pickReader('/Libra Test/');
  await sleep(400);
  r.check('colour reader offers the colour choice', !!(await page.rect('#export-color')));
  // Its screen marked as gray in Settings: the choice goes; back to colour, it comes back.
  const setColour = async (yes) => {
    // (On a phone the settings sheet of Export covers the tabs; the tab is pressed all the same.)
    if (narrow) await page.eval("document.querySelector('#nav-Settings').click()"); else await nav(page, 'Settings');
    await page.waitFor("!!document.querySelector('#settings-nav-reader')", 10, 'Settings');
    await page.click('#settings-nav-reader');
    await page.waitFor("!!document.querySelector('#reader-color')", 10, 'the reader form');
    await page.click(yes ? '#reader-color-yes' : '#reader-color-no');
    await page.click('.form-footer .btn.primary');
    await sleep(400);
    await nav(page, 'Export');
    await page.waitFor(narrow ? "!!document.querySelector('#export-settings-tab')" : "!!document.querySelector('#export-format')", 10, 'export screen');
    await clearToasts(page);
    await openPanels();
    await sleep(300);
  };
  await setColour(false);
  r.check('a reader saved as having no colour screen offers no colour choice', !(await page.eval("!!document.querySelector('#export-color')")));
  await setColour(true);
  r.check('saved as a colour screen again, Black & white | Color is back in Send to', !!(await page.rect('#export-color')) && !!(await page.rect('#export-bw')));
  await runExport('EPUB');
  r.check('EPUB export finished', await page.eval("!!document.querySelector('#export-done')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  await page.click('.export-download');
  const epub = await waitFile('.epub');
  r.check('EPUB downloaded', !!epub);
  if (epub) {
    const names = zipNames(readFileSync(epub));
    const imgs = names.filter((n) => /\.(png|jpe?g|webp)$/i.test(n));
    r.check('EPUB is a zip with 24 page pictures', names.includes('mimetype') && imgs.length === 24, `${names.length} entries, ${imgs.length} pictures`);
  }
  await page.click('#export-again');
  await runExport('CBZ', { group: '#export-group-chapter' });
  r.check('CBZ per chapter finished', await page.eval("!!document.querySelector('#export-done')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  const nfiles = await page.eval("document.querySelectorAll('.export-file').length");
  r.check('one CBZ per chapter (3 files)', nfiles === 3, `${nfiles}`);
  await page.click('#export-download-all');
  await sleep(3000);
  const cbzs = files().filter((f) => f.endsWith('.cbz'));
  r.check('all three downloaded', cbzs.length === 3, cbzs.join(', '));
  if (cbzs[0]) {
    const names = zipNames(readFileSync(join(dl, cbzs[0]))).filter((n) => /\.(png|jpe?g|webp)$/i.test(n));
    r.check('a CBZ holds its chapter\'s 8 pages', names.length === 8, `${names.length}`);
  }
  await page.click('#export-again');

  // Phone or tablet: original quality
  await pickReader('/Phone or tablet/');
  await sleep(400);
  const fm = await page.eval("[...document.querySelectorAll('#export-format button')].map(b => b.id.replace('export-format-', ''))");
  r.check('Phone or tablet offers CBZ and PDF', fm.join() === 'CBZ,PDF', fm.join());
  await page.shot(`${viewport}-32-export-phone`);
  // A phone or tablet gets the pages as they are: one long page, nothing between them, no Compare.
  // The pages load as they come near what is shown; a wide column shows less than two of them at once.
  await page.eval("document.querySelector('#export-pages').scrollTop = 400");
  await page.waitFor("document.querySelectorAll('.original-preview .reader-tile img').length > 1", 30, 'the original pages');
  r.check('phone or tablet: no Compare, no switch, no converted rows', !(await page.eval("!!document.querySelector('#export-compare, .compare-tab, .editor-mobile-switch, .editor-row')")));
  r.check('phone or tablet keeps the top bar: page jump, and no zoom', await page.eval("!!document.querySelector('.editor-toolbar .editor-page-jump input') && !document.querySelector('.editor-toolbar .zoom-menu')"));
  r.check('Fit page is no wider than 800px', await page.eval("Math.round(document.querySelector('.original-preview .reader-tile').getBoundingClientRect().width) <= 801"));
  r.check('the pages touch each other, with no gap', await page.eval("(() => { const a = [...document.querySelectorAll('.original-preview .reader-tile')]; return a.length > 1 && a.slice(1).every((im, i) => Math.abs(im.getBoundingClientRect().top - a[i].getBoundingClientRect().bottom) < 1); })()"));
  // Back to a reader: the page list is a new element and must be sized to the window again (no gap under it).
  if (viewport === 'desktop') {
    await pickReader('/Xteink X4 Pro/');
    await sleep(1500);
    r.check('after going back to a reader the page list fills the window', await page.eval("(() => { const e = document.querySelector('#page-editor'); return !!e && e.style.getPropertyValue('--canvas-height') !== '' && !e.classList.contains('original-editor'); })()"), await page.eval("(() => { const e = document.querySelector('#page-editor'); return e ? e.className + ' / ' + e.getAttribute('style') : 'none'; })()"));
    await pickReader('/Phone or tablet/');
    await sleep(800);
  }
  await runExport('PDF');
  r.check('PDF export finished', await page.eval("!!document.querySelector('#export-done')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  await page.click('.export-download');
  const pdf = await waitFile('.pdf');
  r.check('PDF downloaded and starts with %PDF', !!pdf && readFileSync(pdf).toString('latin1', 0, 4) === '%PDF');
  await page.click('#export-again');

  // Send to the library instead of a folder: CBZ and EPUB only.
  r.check('the library is not offered for a PDF', await page.eval("document.querySelector('#export-target-library').disabled"));
  await page.click('#export-format-CBZ');
  r.check('the library is offered for a CBZ', !(await page.eval("document.querySelector('#export-target-library').disabled")));
  await page.click('#export-target-library');
  await clearToasts(page);
  await page.click('#export-start');
  await page.waitFor("!!document.querySelector('#export-added, #export-error')", 60, 'the files to reach the library');
  r.check('the files were added to the library', await page.eval("!!document.querySelector('#export-added')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  r.check('no files were offered for download', !(await page.eval("!!document.querySelector('.export-download')")));
  await clearToasts(page); // a toast can sit over the button on a phone
  await page.click('#export-open-library');
  await sleep(600);
  r.check('Open the library goes to the library', !(await page.eval("!!document.querySelector('#export-screen')")));
  r.check('the new book is in the library', (await page.eval("document.body.innerText.match(/Hero Saga/g) || []")).length >= 2);
  await nav(page, 'Export');
  await page.waitFor(narrow ? "!!document.querySelector('#export-settings-tab')" : "!!document.querySelector('#export-format')", 10, 'export screen again');
  await clearToasts(page);
  await openPanels();
  await page.click('#export-target-folder');

  // Cancel mid-export
  await pickReader('/Libra Test/');
  await sleep(400);
  await page.click('#export-format-EPUB');
  await page.eval("document.querySelector('#export-start').click()");
  await page.waitFor("!!document.querySelector('#export-cancel')", 10, 'the Cancel button');
  await page.click('#export-cancel');
  await sleep(600);
  r.check('cancel returns to the form, nothing made', await page.eval("!!document.querySelector('#export-start') && !document.querySelector('#export-done')"));
  const after = await page.eval("(async () => (await window.mp.call('export_status', '{}')))()");
  r.check('worker is gone after cancel', after.includes('"running":false'), after);
  r.check('page still responds', (await page.eval('1+1')) === 2);
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} catch (e) {
  r.check(`script finished (${e.message})`, false);
  try { await page.shot(`${viewport}-export-failure`); } catch (_) { /* gone */ }
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
