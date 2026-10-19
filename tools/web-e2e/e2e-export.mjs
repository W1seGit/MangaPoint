// Export in the worker: EPUB, CBZ, XTC with a progress bar, cancel, and a look at the downloaded bytes.
//   node tools/web-e2e/e2e-export.mjs [desktop|tablet|phone]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks } from './helpers.mjs';
import { mkdtempSync, readdirSync, readFileSync, existsSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { inflateRawSync } from 'node:zlib';

const viewport = process.argv[2] || 'desktop';
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
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Test.cbz')]);
  await clearToasts(page);
  await nav(page, 'Export');
  await page.waitFor("!!document.querySelector('#export-device')", 10, 'export screen');
  await page.shot(`${viewport}-30-export`);
  const devices = await page.eval("[...document.querySelectorAll('#export-device option')].map(o => o.textContent)");
  r.check('reader list comes from the engine, with the phone entry last', devices.length > 5 && /Phone or tablet/.test(devices[devices.length - 1]), `${devices.length} readers; first ${devices[0]}`);
  r.check('presets offered', (await page.eval("document.querySelectorAll('#export-preset .pill').length")) === 3);

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
  await page.eval("(() => { const s = document.querySelector('#export-device'); const o = [...s.options].find(o => /Kobo Libra Colou?r/i.test(o.textContent)); s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
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
  await page.eval("(() => { const s = document.querySelector('#export-device'); const o = [...s.options].find(o => /Phone or tablet/.test(o.textContent)); s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
  await sleep(400);
  const fm = await page.eval("[...document.querySelectorAll('#export-format .pill')].map(b => b.textContent)");
  r.check('Phone or tablet offers CBZ and PDF', fm.join() === 'CBZ,PDF', fm.join());
  await page.shot(`${viewport}-32-export-phone`);
  await runExport('PDF');
  r.check('PDF export finished', await page.eval("!!document.querySelector('#export-done')"), await page.eval("document.querySelector('#export-error')?.innerText || ''"));
  await page.click('.export-download');
  const pdf = await waitFile('.pdf');
  r.check('PDF downloaded and starts with %PDF', !!pdf && readFileSync(pdf).toString('latin1', 0, 4) === '%PDF');
  await page.click('#export-again');

  // Cancel mid-export
  await page.eval("(() => { const s = document.querySelector('#export-device'); const o = [...s.options].find(o => /Kobo Libra Colou?r/i.test(o.textContent)); s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); })()");
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
