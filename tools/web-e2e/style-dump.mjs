// Records how every element of the website's screens is styled (the computed style of each), to
// compare two builds when the style sheets are reorganised: the look must not change.
//   node tools/web-e2e/style-dump.mjs <out.json> [desktop|tablet|phone]
//   node tools/web-e2e/style-dump.mjs --compare <before.json> <after.json>
import { readFileSync, writeFileSync } from 'node:fs';
import { launch, startServer, killBrowser, sleep } from './lib.mjs';
import { clearToasts, fx, openApp, nav, importBooks, chooseFiles, waitReview } from './helpers.mjs';

const PROPS = [
  'display', 'position', 'top', 'right', 'bottom', 'left', 'z-index', 'box-sizing', 'width', 'height', 'min-width', 'min-height',
  'max-width', 'max-height', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'padding-top', 'padding-right',
  'padding-bottom', 'padding-left', 'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width',
  'border-top-style', 'border-top-color', 'border-right-color', 'border-bottom-color', 'border-left-color', 'border-top-left-radius',
  'border-bottom-right-radius', 'background-color', 'background-image', 'color', 'opacity', 'visibility', 'overflow-x', 'overflow-y',
  'font-family', 'font-size', 'font-weight', 'font-style', 'line-height', 'letter-spacing', 'text-align', 'text-transform',
  'text-decoration-line', 'white-space', 'text-overflow', 'vertical-align', 'flex-direction', 'flex-wrap', 'flex-grow', 'flex-shrink',
  'flex-basis', 'justify-content', 'align-items', 'align-self', 'gap', 'grid-template-columns', 'grid-template-rows', 'order',
  'transform', 'transition-property', 'transition-duration', 'animation-name', 'animation-duration', 'box-shadow', 'outline-style',
  'outline-color', 'cursor', 'pointer-events', 'object-fit', 'aspect-ratio', 'filter', 'backdrop-filter', 'content', 'fill', 'stroke',
];

if (process.argv[2] === '--compare') {
  const a = JSON.parse(readFileSync(process.argv[3], 'utf8'));
  const b = JSON.parse(readFileSync(process.argv[4], 'utf8'));
  let diffs = 0;
  for (const screen of Object.keys(a)) {
    if (!b[screen]) { console.log(`screen ${screen}: missing in the second`); diffs++; continue; }
    const ea = a[screen], eb = b[screen];
    const keys = new Set([...Object.keys(ea), ...Object.keys(eb)]);
    for (const k of keys) {
      if (!ea[k] || !eb[k]) { console.log(`${screen} ${k}: only in ${ea[k] ? 'the first' : 'the second'}`); diffs++; continue; }
      for (const p of Object.keys(ea[k])) {
        if (ea[k][p] !== eb[k][p]) {
          if (diffs < 400) console.log(`${screen} ${k} ${p}: ${ea[k][p]} -> ${eb[k][p]}`);
          diffs++;
        }
      }
    }
  }
  console.log(`${diffs} differences`);
  process.exit(diffs ? 1 : 0);
}

const out = process.argv[2];
const viewport = process.argv[3] || 'desktop';
const server = await startServer();
const page = await launch({ headless: true });
const result = {};

async function dump(name) {
  await sleep(400);
  result[name] = await page.eval(`(() => {
    for (const a of document.getAnimations()) { try { a.finish(); } catch (e) { a.cancel(); } }
    const props = ${JSON.stringify(PROPS)};
    const path = (el) => {
      const parts = [];
      while (el && el.nodeType === 1 && el !== document.documentElement) {
        const parent = el.parentElement;
        const i = parent ? [...parent.children].indexOf(el) : 0;
        parts.unshift(el.tagName.toLowerCase() + (el.id ? '#' + el.id : '') + ':' + i);
        el = parent;
      }
      return parts.join('>');
    };
    const res = {};
    for (const el of document.querySelectorAll('body *')) {
      if (el.closest('svg') && el.tagName.toLowerCase() !== 'svg') continue;
      const cs = getComputedStyle(el);
      const o = {};
      for (const p of props) o[p] = cs.getPropertyValue(p);
      res[path(el)] = o;
      for (const pseudo of ['::before', '::after']) {
        const ps = getComputedStyle(el, pseudo);
        if (ps.content && ps.content !== 'none' && ps.content !== 'normal') {
          const q = {};
          for (const p of props) q[p] = ps.getPropertyValue(p);
          res[path(el) + pseudo] = q;
        }
      }
    }
    return res;
  })()`);
  console.log(`${name}: ${Object.keys(result[name]).length} elements`);
}

try {
  await openApp(page, server.url, viewport);
  await dump('library-empty');
  await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  await importBooks(page, [fx('Color Webtoon.cbz')]);
  await clearToasts(page);
  await dump('library');
  // Reaching each state is not what is tested here: plain clicks from the page are enough.
  const js = (selector, index = 0) => page.eval(`document.querySelectorAll(${JSON.stringify(selector)})[${index}].click()`);
  await js('.poster-check');
  await dump('library-ticked');
  await js('#bar-more');
  await dump('library-bar-menu');
  await js('.menu-scrim');
  await js('.poster-check');
  await sleep(200);
  await js('.poster-more');
  await dump('card-menu');
  await js('.menu-scrim');
  await sleep(200);
  await js('.poster-art');
  await page.waitFor("!!document.querySelector('.overview')", 10, 'the book page');
  await dump('book');
  await nav(page, 'Library');
  await page.click('.poster-read', null, 0);
  await page.waitFor("!!document.querySelector('#reader .reader-tile img, #reader .reader-end')", 20, 'a page');
  await dump('reader');
  await page.click('#reader-view-button');
  await dump('reader-settings');
  await page.click('#reader-zoom-toggle');
  await dump('reader-zoom-menu');
  await page.eval("document.querySelector('#reader-settings-close').click()");
  await sleep(200);
  await page.click('#reader-chapter-picker');
  await dump('reader-chapter-menu');
  await page.eval("document.querySelector('.menu-scrim')?.click()");
  await page.eval("document.querySelector('#reader-back').click()");
  await sleep(500);
  await nav(page, 'Export');
  await page.waitFor("!!document.querySelector('.editor-row img, .original-tile, .export-screen')", 30, 'the export screen');
  await sleep(1500);
  await dump('export');
  await nav(page, 'Settings');
  await dump('settings');
  for (const id of await page.eval("[...document.querySelectorAll('.settings-nav button, .sx-nav button')].map((b, i) => i)")) {
    await page.eval(`document.querySelectorAll('.settings-nav button, .sx-nav button')[${id}].click()`);
    await dump(`settings-${id}`);
  }
  await nav(page, 'Library');
  await chooseFiles(page, [fx('Picture Book.epub')]);
  await waitReview(page);
  await dump('import-review');
  await page.click('#import-cancel');
  await sleep(500);
  await page.click('.poster-more', null, 0);
  await page.click('.poster-menu .forget-item');
  await dump('modal');
  writeFileSync(out, JSON.stringify(result));
  console.log(`written ${out}`);
} finally {
  killBrowser(page);
  server.stop();
}
