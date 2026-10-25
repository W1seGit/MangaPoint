// Showing pages: a picture is unpacked and given a blob: URL only while it is
// on (or near) the screen, and the URL is revoked a moment after the page
// leaves. Memory therefore stays proportional to what is visible, however long
// the book is.
import { BookSource } from './pagesource.js';
import { dbGet, dbPutSoon } from './store.js';
import { sessionFiles } from './importer.js';

const sources = new Map(); // book id -> Promise<BookSource>

export function source(bookId) {
  let p = sources.get(bookId);
  if (!p) {
    p = (async () => {
      const [book, table] = await Promise.all([dbGet('books', bookId), dbGet('pages', bookId)]);
      if (!book || !table) throw new Error('This book is not in the library any more.');
      return new BookSource(book, table, sessionFiles.get(bookId) || null);
    })();
    p.catch(() => sources.delete(bookId));
    sources.set(bookId, p);
    // Keep only a few books open.
    if (sources.size > 3) sources.delete(sources.keys().next().value);
  }
  return p;
}

export function forgetSource(bookId) {
  sources.delete(bookId);
}

// ---- blob URLs with a lifetime ----------------------------------------------------

const urls = new Map(); // `${book}#${i}` -> { url, refs, last, promise }
let sweeper = 0;

const keyOf = (book, i) => `${book}#${i}`;

export function liveUrlCount() {
  let n = 0;
  for (const e of urls.values()) if (e.url) n++;
  return n;
}

/** A URL for the picture of page `i`; call release() when the page goes away. */
export async function acquire(book, i) {
  const k = keyOf(book, i);
  let e = urls.get(k);
  if (!e) {
    e = { url: null, refs: 0, last: Date.now(), promise: null };
    e.promise = (async () => {
      const src = await source(book);
      const blob = await src.blob(i);
      e.url = URL.createObjectURL(blob);
      return e.url;
    })();
    e.promise.catch(() => urls.delete(k));
    urls.set(k, e);
  }
  e.refs++;
  e.last = Date.now();
  try {
    return await e.promise;
  } catch (err) {
    e.refs--;
    throw err;
  }
}

export function release(book, i) {
  const e = urls.get(keyOf(book, i));
  if (e) {
    e.refs = Math.max(0, e.refs - 1);
    e.last = Date.now();
  }
  scheduleSweep();
}

function scheduleSweep() {
  if (sweeper) return;
  sweeper = setTimeout(() => {
    sweeper = 0;
    const now = Date.now();
    let waiting = false;
    for (const [k, e] of urls) {
      if (e.refs > 0 || !e.url) continue;
      if (now - e.last > 1500) {
        URL.revokeObjectURL(e.url);
        urls.delete(k);
      } else waiting = true;
    }
    if (waiting) scheduleSweep();
  }, 1600);
}

export function dropAllUrls() {
  for (const e of urls.values()) if (e.url) URL.revokeObjectURL(e.url);
  urls.clear();
}

// ---- measured sizes ------------------------------------------------------------------

export async function rememberSize(book, i, w, h) {
  try {
    const src = await source(book);
    const p = src.table.pages[i];
    if (p && (p.w !== w || p.h !== h)) {
      // Tables saved before `at` kept where the page is in `w`: keep that before the width takes its place.
      if (typeof p.w === 'string' && p.at === undefined) p.at = p.w;
      p.w = w;
      p.h = h;
      src.table.measured = true;
      dbPutSoon('pages', book, src.table, 2500);
    }
  } catch (_) { /* not worth failing for */ }
}

/** The shape of page `i` if it was measured, else the book's typical shape. */
export function knownRatio(src, i) {
  const p = src.table.pages[i];
  if (p && p.w > 0 && p.h > 0) return { w: p.w, h: p.h };
  if (src.typical === undefined || (src.typical === null && src.table.measured)) {
    const ratios = src.table.pages.filter((q) => q.w > 0 && q.h > 0).map((q) => q.h / q.w).sort((a, b) => a - b);
    src.typical = ratios.length ? ratios[Math.floor(ratios.length / 2)] : null;
  }
  return src.typical ? { w: 1000, h: Math.round(1000 * src.typical), typical: true } : null;
}
