// Steps the tests share: real clicks and real file choosers, nothing scripted.
import { join } from 'node:path';
import { sleep, FIXTURES } from './lib.mjs';

export const fx = (...parts) => join(FIXTURES, ...parts);

export async function openApp(page, url, viewport = 'desktop') {
  await page.viewport(viewport);
  await page.goto(url);
  await page.waitFor("!!document.querySelector('.nav')", 30, 'the app to start');
  await sleep(300);
}

/** Go to a screen by its nav button (the sidebar on desktop, the tab bar on phones). */
export async function nav(page, label) {
  await page.click(`#nav-${label}`);
  await sleep(250);
}

/** Open the Import menu and choose files with the real file chooser. */
export async function chooseFiles(page, files, folder = false) {
  page.answerFileChooser(files);
  const empty = await page.rect('#empty-import-files');
  if (!empty) {
    await page.click('#import-button');
    await page.click(folder ? '#import-folder' : '#import-files');
  } else {
    await page.click(folder ? '#empty-import-folder' : '#empty-import-files');
  }
}

export async function waitReview(page, seconds = 120) {
  await page.waitFor("!!document.querySelector('#import-add, #import-cancel') && !document.querySelector('#import-wait')", seconds, 'the import review');
  await sleep(200);
}

/** Import files, check them in the review, add them. Resolves to the review text. */
export async function importBooks(page, files, { folder = false, keep = true } = {}) {
  await chooseFiles(page, files, folder);
  await waitReview(page);
  const text = await page.eval("document.querySelector('#import-dialog').innerText");
  if (keep) {
    await page.click('#import-add');
    await page.waitFor("!document.querySelector('#import-dialog')", 300, 'the import to finish');
    await sleep(300);
  }
  return text;
}

export async function closeImport(page) {
  await page.click('#import-cancel');
  await page.waitFor("!document.querySelector('#import-dialog')", 10, 'the dialog to close');
}

export const titles = (page) => page.eval("[...document.querySelectorAll('.poster-title')].map((e) => e.textContent)");

export async function openBook(page, title) {
  const id = await page.eval(`(() => { const p = [...document.querySelectorAll('.poster')].find((p) => p.querySelector('.poster-title').textContent.includes(${JSON.stringify(title)})); return p ? p.dataset.book : null; })()`);
  if (!id) throw new Error(`no book called ${title}`);
  await page.click(`#read-${id}`);
  await page.waitFor("!!document.querySelector('#reader')", 20, 'the reader');
  await page.waitFor("!!document.querySelector('.reader-tile img, .reader-end')", 20, 'a page');
  await sleep(200);
  return id;
}

export const counter = (page) => page.eval("document.querySelector('#reader-page-counter')?.textContent.trim()");
export const chapterLabel = (page) => page.eval("document.querySelector('#reader-chapter-picker')?.textContent.trim()");
export const pageNo = (page) => page.eval("document.querySelector('.reader-tile')?.dataset.page");

/** Is there sideways scrolling anywhere? */
export const hScroll = (page) => page.eval("({ doc: document.documentElement.scrollWidth - document.documentElement.clientWidth, body: document.body.scrollWidth - document.body.clientWidth })");

export async function closeReader(page) {
  if (!(await page.eval("document.querySelector('#reader')?.classList.contains('show-bar')"))) {
    const [x, y, w, h] = await page.rect('#reader-stage');
    await page.clickAt(x + w / 2, y + h * 0.55); // the middle third shows the bar
    await sleep(250);
    // A tap can land on the end card (which takes no taps): scrolling up shows the bars too.
    if (!(await page.eval("document.querySelector('#reader')?.classList.contains('show-bar')"))) {
      await page.eval("document.querySelector('#reader-stage')?.scrollBy(0, -160)");
      await sleep(400);
    }
  }
  await page.click('#reader-back');
  await page.waitFor("!document.querySelector('#reader')", 10, 'the reader to close');
  await sleep(150);
}

/** Dismiss every toast (they float over the bottom of a phone screen). */
export async function clearToasts(page) {
  await page.eval("document.querySelectorAll('.toast-x').forEach((b) => b.click())");
  await sleep(600);
}

/** Open the reading settings page, press one thing in it and close it again. */
export async function readerSetting(page, selector, touch = false) {
  const press = (sel) => (touch ? page.tap(sel) : page.click(sel));
  await press('#reader-view-button');
  await page.waitFor("!!document.querySelector('#reader-settings')", 5, 'the reading settings');
  await sleep(250);
  await press(selector);
  await sleep(250);
  await press('#reader-settings-close');
  await sleep(300);
}
