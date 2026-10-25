// A series folder from MangaPoint's own library (`solo-leveling-75931860/c2/00001.img`, `cover.img`) is picked
// with "Choose a folder": it is read as a series with its chapters and cover, and reads straight away.
//   node tools/web-e2e/e2e-mangapoint-folder.mjs [desktop|tablet|phone]
import { mkdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { openApp, importBooks, titles, openBook, chapterLabel } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const r = new Report();

// A small grey PNG, written by hand so no fixture file is needed.
const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
const chunk = (type, data) => {
  const head = Buffer.alloc(8); head.writeUInt32BE(data.length); head.write(type, 4);
  const tail = Buffer.alloc(4); tail.writeUInt32BE(crc(Buffer.concat([Buffer.from(type), data])));
  return Buffer.concat([head, data, tail]);
};
const png = (shade, w = 40, h = 60) => {
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 0;
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(w, shade)]);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(Buffer.concat(Array(h).fill(row)))), chunk('IEND', Buffer.alloc(0))]);
};

const base = mkdtempSync(join(tmpdir(), 'mp-own-library-'));
const series = join(base, 'solo-leveling-75931860');
for (const [chapter, pages] of [['c1', 2], ['c2', 3], ['c2_5', 1], ['c10', 2]]) {
  mkdirSync(join(series, chapter), { recursive: true });
  for (let i = 1; i <= pages; i++) writeFileSync(join(series, chapter, `${String(i).padStart(5, '0')}.img`), png(40 * i));
  writeFileSync(join(series, chapter, 'source.txt'), 'mangadex');
}
writeFileSync(join(series, 'cover.img'), png(200));

const server = await startServer();
const page = await launch({ headless: true });
try {
  await openApp(page, server.url, viewport);
  const text = await importBooks(page, [series], { folder: true, keep: false });
  const named = await page.eval("[...document.querySelectorAll('#import-dialog input')].map((i) => i.value).join(' | ')");
  r.check('the review names the series from its folder', /Solo Leveling/.test(named), named);
  await page.click('#import-add');
  await page.waitFor("!document.querySelector('#import-dialog')", 60, 'the import to finish');
  await sleep(300);
  r.check('the review found the four chapters', /4 chapters/.test(text), text.replace(/\n/g, ' | ').slice(0, 160));
  r.check('the review counts eight pages (the cover is not one)', /8 pages/.test(text), text.replace(/\n/g, ' | ').slice(0, 160));
  r.check('it is in the library as Solo Leveling', (await titles(page)).includes('Solo Leveling'), JSON.stringify(await titles(page)));
  await openBook(page, 'Solo Leveling');
  await page.waitFor("(() => { const i = document.querySelector('.reader-tile img'); return i && i.complete && i.naturalWidth > 0; })()", 15, 'the first page picture');
  r.check('the first page is shown', true);
  const label = await chapterLabel(page);
  r.check('it opens at chapter 1', /1/.test(label || ''), String(label));
  await sleep(200);
  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  rmSync(base, { recursive: true, force: true });
  process.exit(failed ? 1 : 0);
}
