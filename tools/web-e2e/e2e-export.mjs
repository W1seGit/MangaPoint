// Export in the worker: EPUB, CBZ, XTC with a progress bar, cancel, and a look at the downloaded bytes.
//   node tools/web-e2e/e2e-export.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks } from './helpers.mjs';
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
  // Nothing imported yet: the screen still opens, on the sample pages, and cannot export them.
  await nav(page, 'Export');
  await page.waitFor("!!document.querySelector('.editor-row img')", 30, 'the sample pages');
  r.check('with no book the sample pages are shown, converted', (await page.eval("document.querySelectorAll('.editor-row img').length")) >= 4);
  r.check('the top bar: page jump and zoom, thumbnails, Compare and Overview, as in the app', !!(await page.rect('.editor-toolbar .editor-page-jump input')) && !!(await page.rect('.editor-toolbar .zoom-menu')) && (narrow ? true : !!(await page.rect('.editor-thumbnails .editor-thumb')) && !!(await page.rect('#export-overview'))) && (narrow ? !!(await page.rect('#export-tab-original')) : !!(await page.rect('#export-compare'))));
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
  await page.waitFor("document.querySelectorAll('.original-preview .reader-tile img').length > 1", 30, 'the original pages');
  r.check('phone or tablet: no Compare, no switch, no converted rows', !(await page.eval("!!document.querySelector('#export-compare, .compare-tab, .editor-mobile-switch, .editor-row')")));
  r.check('phone or tablet keeps the top bar: page jump and zoom', await page.eval("!!document.querySelector('.editor-toolbar .editor-page-jump input') && !!document.querySelector('.editor-toolbar .zoom-menu')"));
  if (!narrow) {
    const tileW = () => page.eval("Math.round(document.querySelector('.original-preview .reader-tile').getBoundingClientRect().width)");
    const w100 = await tileW();
    await page.click('.editor-toolbar .zoom-chevron');
    await sleep(300);
    await page.click('.zoom-list .menu-item', '50%');
    await sleep(400);
    r.check('zoom changes the size of the pages', (await tileW()) < w100 - 5, `${w100} -> ${await tileW()}`);
    await page.click('.editor-toolbar .zoom-chevron');
    await sleep(300);
    await page.click('.zoom-list .menu-item', 'Fit page');
    await sleep(300);
  }
  r.check('the pages touch each other, with no gap', await page.eval("(() => { const a = [...document.querySelectorAll('.original-preview .reader-tile')]; return a.length > 1 && a.slice(1).every((im, i) => Math.abs(im.getBoundingClientRect().top - a[i].getBoundingClientRect().bottom) < 1); })()"));
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
