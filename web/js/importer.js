// Turning what the visitor picked or dropped into books.
//
// analyze(): looks at the files WITHOUT keeping anything: lists the pictures,
// asks the engine for the chapters and the book's fingerprint, makes a
// thumbnail of the first page. The result is a draft the person reviews.
// commit(): keeps the book on this device (OPFS, or IndexedDB) and returns the
// record the library shows.
//
// Nothing here talks to a network.
import { openZip, safePath, ZipError } from './zip.js';
import { MIME, extOf, isImageName } from './pagesource.js';
import { saveFile, dbPut, dbGet } from './store.js';

export const engine = {
  // Filled in by the page once its WebAssembly is up:
  //   fingerprint(size, head, tail) -> string
  //   guessChapters(entriesJson) -> string (JSON)
  fingerprint: null,
  guessChapters: null,
};

const collator = new Intl.Collator('en', { numeric: true, sensitivity: 'base' });
export const naturalCompare = (a, b) => collator.compare(a, b) || (a < b ? -1 : a > b ? 1 : 0);

const UNSUPPORTED = {
  rar: 'RAR', cbr: 'RAR (CBR)', '7z': '7z', cb7: '7z (CB7)', pdf: 'PDF', cbt: 'TAR (CBT)', tar: 'TAR',
};
const ARCHIVES = ['cbz', 'zip', 'epub'];
const HEAD_TAIL = 256 * 1024;
// The biggest ComicInfo.xml read: a book of 40,000 pages with a bookmark for every chapter is 3 MB.
const COMICINFO_LIMIT = 16 * 1024 * 1024;

function failure(name, message) {
  return { name, message };
}

function isJunk(path) {
  const p = path.replaceAll('\\', '/');
  return p.startsWith('__MACOSX/') || p.split('/').some((s) => s.startsWith('._') || s === '.DS_Store' || s === 'Thumbs.db');
}

const draftStore = new Map(); // draft id -> private data (the Files, the page table)
let draftCounter = 0;

// ---- reading folders ---------------------------------------------------------

/** Flatten dropped items (files and folders) into [{ file, path }]. */
export async function filesFromDrop(dataTransfer) {
  const out = [];
  const entries = [];
  for (const item of dataTransfer.items || []) {
    if (item.kind !== 'file') continue;
    const entry = item.webkitGetAsEntry ? item.webkitGetAsEntry() : null;
    if (entry) entries.push(entry);
    else {
      const f = item.getAsFile();
      if (f) out.push({ file: f, path: f.name });
    }
  }
  const walk = async (entry, prefix) => {
    if (entry.isFile) {
      const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
      out.push({ file, path: prefix + entry.name });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      for (;;) {
        const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
        if (!batch.length) break;
        for (const child of batch) await walk(child, `${prefix}${entry.name}/`);
      }
    }
  };
  for (const e of entries) await walk(e, '');
  if (!out.length && dataTransfer.files) {
    for (const f of dataTransfer.files) out.push({ file: f, path: f.name });
  }
  return out;
}

/** From an <input type=file>: webkitRelativePath is set for a folder. */
export function filesFromInput(fileList) {
  return [...fileList].map((file) => ({ file, path: file.webkitRelativePath || file.name }));
}

// ---- thumbnails ----------------------------------------------------------------

/** A small JPEG of a picture, and the picture's size. Never throws. */
export async function thumbnail(blob, width = 320) {
  try {
    const bmp = await createImageBitmap(blob);
    const scale = Math.min(1, width / bmp.width);
    const w = Math.max(1, Math.round(bmp.width * scale));
    const h = Math.max(1, Math.round(bmp.height * scale));
    let canvas;
    if (typeof OffscreenCanvas !== 'undefined') canvas = new OffscreenCanvas(w, h);
    else {
      canvas = document.createElement('canvas');
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext('2d');
    ctx.drawImage(bmp, 0, 0, w, h);
    const size = { w: bmp.width, h: bmp.height };
    bmp.close?.();
    const out = canvas.convertToBlob
      ? await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.82 })
      : await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.82));
    return out ? { blob: out, ...size } : null;
  } catch (_) {
    return null;
  }
}

// ---- EPUB ------------------------------------------------------------------------

function dirOf(path) {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i + 1);
}

function resolvePath(base, href) {
  const clean = decodeURIComponent(href.split('#')[0].split('?')[0]);
  const parts = (clean.startsWith('/') ? clean.slice(1) : base + clean).split('/');
  const out = [];
  for (const p of parts) {
    if (p === '..') out.pop();
    else if (p && p !== '.') out.push(p);
  }
  return out.join('/');
}

async function zipText(zip, byName, name, limit = 4 * 1024 * 1024) {
  const e = byName.get(name) || byName.get(name.toLowerCase());
  if (!e || e.usize > limit) return null;
  return new TextDecoder('utf-8').decode(await (await zip.read(e, '', limit)).arrayBuffer());
}

/** Pictures of an EPUB in reading order, and its title. */
async function epubPictures(zip, byName, imageNames) {
  let title = '';
  const ordered = [];
  const seen = new Set();
  try {
    const container = await zipText(zip, byName, 'META-INF/container.xml');
    const opfPath = /full-path\s*=\s*["']([^"']+)["']/i.exec(container || '')?.[1];
    const opf = opfPath ? await zipText(zip, byName, opfPath) : null;
    if (opf) {
      title = /<dc:title[^>]*>([^<]*)</i.exec(opf)?.[1]?.trim() || '';
      const base = dirOf(opfPath);
      const manifest = new Map();
      for (const m of opf.matchAll(/<item\b[^>]*>/gi)) {
        const tag = m[0];
        const id = /\bid\s*=\s*["']([^"']+)["']/i.exec(tag)?.[1];
        const href = /\bhref\s*=\s*["']([^"']+)["']/i.exec(tag)?.[1];
        if (id && href) manifest.set(id, resolvePath(base, href));
      }
      const spine = [...opf.matchAll(/<itemref\b[^>]*\bidref\s*=\s*["']([^"']+)["']/gi)].map((m) => manifest.get(m[1])).filter(Boolean);
      for (const page of spine.slice(0, 5000)) {
        const html = await zipText(zip, byName, page);
        if (!html) continue;
        const refs = [
          ...html.matchAll(/<img\b[^>]*?\ssrc\s*=\s*["']([^"']+)["']/gi),
          ...html.matchAll(/<image\b[^>]*?\s(?:xlink:)?href\s*=\s*["']([^"']+)["']/gi),
        ].sort((a, b) => a.index - b.index);
        for (const r of refs) {
          const name = resolvePath(dirOf(page), r[1]);
          if (byName.has(name) && imageNames.has(name) && !seen.has(name)) {
            seen.add(name);
            ordered.push(name);
          }
        }
      }
    }
  } catch (_) {
    // A damaged EPUB: fall back to the file names below.
  }
  // Pictures the pages did not mention (or an EPUB without a usable spine).
  const rest = [...imageNames].filter((n) => !seen.has(n)).sort(naturalCompare);
  return { names: ordered.length >= Math.max(1, imageNames.size / 2) ? ordered.concat(rest) : [...imageNames].sort(naturalCompare), title };
}

// ---- fingerprints -----------------------------------------------------------------

async function sha256Hex(text) {
  const d = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function fileFingerprint(file) {
  const head = new Uint8Array(await file.slice(0, HEAD_TAIL).arrayBuffer());
  const tail = new Uint8Array(await file.slice(Math.max(0, file.size - HEAD_TAIL)).arrayBuffer());
  if (engine.fingerprint) return engine.fingerprint(file.size, head, tail);
  throw new Error("The page engine has not started yet.");
}

// ---- chapters ---------------------------------------------------------------------

/**
 * Ask the engine. `entries`: [{ path, size }] in any order; the engine decides
 * which are pages and in which order. Returns { title, chapters, pages, cover,
 * comicinfo, warnings }; `pages` are indices into `entries`.
 */
function ask(input) {
  if (!engine.guessChapters) throw new Error('The page engine has not started yet.');
  return JSON.parse(engine.guessChapters(JSON.stringify(input)));
}

function titleFromFileName(name) {
  return name.replace(/\.[^.]+$/, '').replace(/[_]+/g, ' ').trim() || 'Untitled';
}

//** Does this start like a picture the browser can show (or the converter can read)? */
export function looksLikeImage(b) {
  if (!b || b.length < 4) return false;
  const at = (i, ...v) => v.every((x, k) => b[i + k] === x);
  return (
    at(0, 0xff, 0xd8, 0xff) || // JPEG
    at(0, 0x89, 0x50, 0x4e, 0x47) || // PNG
    at(0, 0x47, 0x49, 0x46, 0x38) || // GIF
    (at(0, 0x52, 0x49, 0x46, 0x46) && b.length >= 12 && at(8, 0x57, 0x45, 0x42, 0x50)) || // WebP
    at(0, 0x49, 0x49, 0x2a, 0x00) || at(0, 0x4d, 0x4d, 0x00, 0x2a) || // TIFF
    at(0, 0x42, 0x4d) || // BMP
    (b.length >= 12 && at(4, 0x66, 0x74, 0x79, 0x70)) // AVIF/HEIF
  );
}

// ---- analysis -----------------------------------------------------------------------

/** Run the engine on a list of entries, reading the book's ComicInfo.xml if it names one. */
async function guessWith(label, kind, entries, readText) {
  let g = ask({ label, kind, entries });
  if (g.comicinfo != null && readText) {
    try {
      const xml = await readText(g.comicinfo);
      if (xml) g = ask({ label, kind, entries, comicinfo: xml });
    } catch (_) { /* the book's own notes are a bonus */ }
  }
  return g;
}

async function analyzeArchive({ file }, ext) {
  let zip;
  try {
    zip = await openZip(file);
  } catch (e) {
    if (e instanceof ZipError) return { failure: failure(file.name, e.message) };
    return { failure: failure(file.name, 'This file could not be opened.') };
  }
  const byName = new Map();
  const imageNames = new Set();
  let unsafe = 0;
  let locked = 0;
  for (const e of zip.entries) {
    byName.set(e.name, e);
    if (e.isDir || isJunk(e.name) || !isImageName(e.name)) continue;
    if (!safePath(e.name)) { unsafe++; continue; }
    if (e.encrypted) { locked++; continue; }
    if (e.usize === 0) continue;
    imageNames.add(e.name);
  }
  if (!imageNames.size) {
    if (locked) return { failure: failure(file.name, 'The pictures in this file are password protected.') };
    if (unsafe) return { failure: failure(file.name, 'The only pictures in this file have unsafe names (they try to leave the folder), so none was opened.') };
    return { failure: failure(file.name, 'No pictures were found in this file.') };
  }
  let title = titleFromFileName(file.name);
  let names;
  let entries;
  if (ext === 'epub') {
    const r = await epubPictures(zip, byName, imageNames);
    names = r.names;
    if (r.title) title = r.title;
    // The reading order of an EPUB is the order of its pages, not of its file
    // names: hand the engine names that sort the way the pages read.
    entries = names.map((n, i) => ({ path: `${String(i + 1).padStart(6, '0')}${n.slice(n.lastIndexOf('.'))}`, size: byName.get(n).usize }));
  } else {
    names = [...imageNames];
    entries = names.map((n) => ({ path: n, size: byName.get(n).usize }));
    // ComicInfo.xml sits beside the pictures; let the engine see it.
    for (const e of zip.entries) {
      if (!e.isDir && /(^|\/)comicinfo\.xml$/i.test(e.name) && !e.encrypted && e.usize < COMICINFO_LIMIT && safePath(e.name)) {
        names.push(e.name);
        entries.push({ path: e.name, size: e.usize });
        break;
      }
    }
  }
  let g;
  try {
    g = await guessWith(title, 'archive', entries, async (i) => {
      const e = byName.get(names[i]);
      return e ? new TextDecoder().decode(await (await zip.read(e, '', COMICINFO_LIMIT)).arrayBuffer()) : null;
    });
  } catch (e) {
    return { failure: failure(file.name, String(e && e.message ? e.message : e)) };
  }
  if (!g.pages || !g.pages.length) return { failure: failure(file.name, 'No pictures were found in this file.') };
  // Files named like pictures that are not: look at the first bytes of a few pages.
  {
    const sample = [...new Set([0, 1, 2, g.pages.length - 1])].filter((i) => i < g.pages.length);
    let good = 0;
    for (const i of sample) {
      try {
        const e = byName.get(names[g.pages[i]]);
        if (looksLikeImage(await zip.head(e, 16))) good++;
      } catch (_) { /* counts as not readable */ }
    }
    if (!good) return { failure: failure(file.name, 'The files in this archive that are named like pictures are not pictures (or are damaged), so there is nothing to read.') };
  }
  if (ext !== 'epub' && g.title) title = g.title;
  const pages = g.pages.map((idx) => {
    const e = byName.get(names[idx]);
    return { n: names[idx], o: e.offset, c: e.csize, u: e.usize, m: e.method };
  });
  const bad = pages.filter((p) => p.m !== 0 && p.m !== 8);
  const warnings = [...(g.warnings || [])];
  if (unsafe) warnings.push(`${unsafe} picture${unsafe === 1 ? '' : 's'} with unsafe names ${unsafe === 1 ? 'was' : 'were'} skipped.`);
  if (locked) warnings.push(`${locked} password-protected picture${locked === 1 ? '' : 's'} ${locked === 1 ? 'was' : 'were'} skipped.`);
  if (bad.length) warnings.push(`${bad.length} picture${bad.length === 1 ? ' is' : 's are'} packed in a way the website cannot open.`);
  let thumb = null;
  try {
    const coverName = g.cover != null ? names[g.cover] : names[g.pages[0]];
    thumb = await thumbnail(await zip.read(byName.get(coverName), MIME[extOf(coverName)] || ''));
  } catch (_) { /* no cover */ }
  return {
    draft: {
      mode: 'archive',
      kind: ext === 'epub' ? 'epub' : 'zip',
      title,
      file_name: file.name,
      size: file.size,
      fingerprint: await fileFingerprint(file),
      pages,
      chapters: g.chapters,
      warnings,
      thumb,
      files: [file],
      rtl: g.meta && typeof g.meta.rightToLeft === 'boolean' ? g.meta.rightToLeft : null,
    },
  };
}

/** Pictures from one folder (kind 'folder': paths are relative to it) or loose ones ('images'). */
async function analyzePictures(items, label, kind) {
  const entries = items.map((i) => ({ path: i.rel, size: i.file.size }));
  let g;
  try {
    g = await guessWith(label, kind, entries, async (i) => (items[i] ? items[i].file.text() : null));
  } catch (e) {
    return { failure: failure(label, String(e && e.message ? e.message : e)) };
  }
  if (!g.pages || !g.pages.length) return { failure: failure(label, 'No pictures were found in what you picked.') };
  const ordered = g.pages.map((i) => items[i]);
  {
    const sample = [...new Set([0, 1, 2, ordered.length - 1])].filter((i) => i < ordered.length);
    let good = 0;
    for (const i of sample) {
      try {
        if (looksLikeImage(new Uint8Array(await ordered[i].file.slice(0, 16).arrayBuffer()))) good++;
      } catch (_) { /* not readable */ }
    }
    if (!good) return { failure: failure(label, 'The files you picked are not readable pictures.') };
  }
  const manifest = ordered.map((i) => `${i.rel}\u0000${i.file.size}`).join('\n');
  let thumb = null;
  try {
    const cover = g.cover != null ? items[g.cover] : ordered[0];
    thumb = await thumbnail(cover.file.slice(0, cover.file.size, MIME[extOf(cover.rel)] || ''));
  } catch (_) { /* no cover */ }
  return {
    draft: {
      mode: 'pictures',
      kind: 'images',
      title: g.title || label,
      file_name: label,
      size: ordered.reduce((s, i) => s + i.file.size, 0),
      fingerprint: await sha256Hex(`mp1-pictures|${manifest}`),
      pages: ordered.map((i, k) => ({ n: i.rel, u: i.file.size, s: k, f: `p${String(k).padStart(5, '0')}.${extOf(i.rel)}` })),
      chapters: g.chapters,
      warnings: g.warnings || [],
      thumb,
      files: ordered.map((i) => i.file),
      rtl: g.meta && typeof g.meta.rightToLeft === 'boolean' ? g.meta.rightToLeft : null,
    },
  };
}

/** Look at what was picked. Returns { drafts: [...], failures: [...] } and keeps the private parts. */
export async function analyze(items, onStatus = () => {}) {
  const failures = [];
  const drafts = [];
  let ignored = 0;
  const archives = [];
  const groups = new Map(); // top folder ('' for loose pictures) -> items
  for (const it of items) {
    const name = it.file.name;
    const ext = extOf(name);
    if (isJunk(it.path)) continue;
    if (UNSUPPORTED[ext]) {
      failures.push(failure(name, `${UNSUPPORTED[ext]} files are not supported on the web yet. Use the MangaPoint desktop app, or save the book as CBZ.`));
    } else if (ARCHIVES.includes(ext)) {
      archives.push(it);
    } else if (isImageName(name)) {
      const slash = it.path.indexOf('/');
      const top = slash > 0 ? it.path.slice(0, slash) : '';
      if (!groups.has(top)) groups.set(top, []);
      groups.get(top).push({ file: it.file, rel: slash > 0 ? it.path.slice(slash + 1) : it.path });
    } else {
      ignored++;
    }
  }
  const tick = () => new Promise((res) => setTimeout(res, 0)); // let the page breathe
  for (const [i, it] of archives.entries()) {
    onStatus(`Reading ${it.file.name} (${i + 1} of ${archives.length})`);
    const r = await analyzeArchive(it, extOf(it.file.name));
    if (r.failure) failures.push(r.failure);
    else drafts.push(r.draft);
    await tick();
  }
  for (const [top, list] of groups) {
    onStatus(`Looking at ${list.length} pictures`);
    const label = top || (list.length === 1 ? titleFromFileName(list[0].file.name) : 'Pictures');
    const r = await analyzePictures(list, label, top ? 'folder' : 'images');
    if (r.failure) failures.push(r.failure);
    else drafts.push(r.draft);
    await tick();
  }
  if (!drafts.length && !failures.length) {
    failures.push(failure(items[0]?.file?.name || 'Nothing', ignored ? 'No books or pictures were found in what you picked.' : 'Nothing was picked.'));
  }
  const out = [];
  for (const d of drafts) {
    const id = `d${++draftCounter}`;
    draftStore.set(id, d);
    const existing = await dbGet('books', d.fingerprint).catch(() => null);
    out.push({
      draft_id: id,
      title: d.title,
      file_name: d.file_name,
      kind: d.kind,
      size: d.size,
      fingerprint: d.fingerprint,
      page_count: d.pages.length,
      chapters: d.chapters,
      warnings: d.warnings,
      cover_url: d.thumb ? URL.createObjectURL(d.thumb.blob) : null,
      aspect: d.thumb && d.thumb.w ? Math.round((d.thumb.h / d.thumb.w) * 1000) / 1000 : null,
      rtl: d.rtl ?? null,
      existing: existing ? { stored: existing.stored } : null,
    });
  }
  return { drafts: out, failures };
}

export function dropDraft(id) {
  const d = draftStore.get(id);
  draftStore.delete(id);
  return d;
}

// ---- keeping the book ---------------------------------------------------------------

export const sessionFiles = new Map(); // book id -> File | File[] when the browser would not keep it

/**
 * Keep a reviewed draft. `edit`: { title, chapters } as the person left them.
 * `onProgress(done, total)`. Resolves to the library record.
 */
export async function commit(draftId, edit, onProgress = () => {}) {
  const d = draftStore.get(draftId);
  if (!d) throw new Error('That import was cancelled.');
  const id = d.fingerprint;
  let stored = 'none';
  if (d.mode === 'archive') {
    const where = await saveFile(id, 'book.bin', d.files[0]);
    if (where) stored = where;
    onProgress(1, 1);
  } else {
    let ok = true;
    let where = null;
    for (const [i, p] of d.pages.entries()) {
      where = await saveFile(id, p.f, d.files[p.s]);
      if (!where) { ok = false; break; }
      if (i % 4 === 0) onProgress(i, d.pages.length);
    }
    stored = ok && where ? where : 'none';
    onProgress(d.pages.length, d.pages.length);
  }
  if (stored === 'none') sessionFiles.set(id, d.mode === 'archive' ? d.files[0] : d.files);
  else sessionFiles.delete(id);
  const chapters = (edit?.chapters || d.chapters).map((c) => ({
    name: String(c.name ?? ''),
    number: typeof c.number === 'number' && Number.isFinite(c.number) ? c.number : null,
    first: c.first,
    count: c.count,
  }));
  const record = {
    id,
    title: (edit?.title || d.title).trim() || d.title,
    file_name: d.file_name,
    kind: d.kind,
    size: d.size,
    added_at: Date.now(),
    stored,
    page_count: d.pages.length,
    has_cover: !!d.thumb,
    aspect: d.thumb && d.thumb.w ? Math.round((d.thumb.h / d.thumb.w) * 1000) / 1000 : null,
    rtl: d.rtl ?? null,
    chapters,
  };
  const table = { mode: d.mode, pages: d.pages.map(({ s, ...rest }) => (d.mode === 'pictures' ? { ...rest, s } : rest)) };
  await dbPut('pages', id, table);
  if (d.thumb) await dbPut('covers', id, d.thumb.blob).catch(() => {});
  await dbPut('books', id, record);
  if (d.thumb) coverUrls.delete(id);
  draftStore.delete(draftId);
  return record;
}

/**
 * Add the chapters of reviewed drafts to a book already in the library, after its own chapters.
 * An archive is kept beside the book as `part-<n>.bin`; loose pictures as files of their own. The
 * book keeps its identity (its place, the chapters read). Resolves to the new library record and
 * how many chapters were added.
 */
export async function appendTo(bookId, draftIds, onProgress = () => {}) {
  const record = await dbGet('books', bookId);
  const table = await dbGet('pages', bookId);
  if (!record || !table) throw new Error('This book is not in the library any more.');
  if (record.stored === 'none') throw new Error('The browser did not keep this book\'s file, so chapters cannot be added to it. Import the book again first.');
  let parts = record.parts || 0;
  let size = record.size || 0;
  const chapters = record.chapters.slice();
  let added = 0;
  for (const [k, draftId] of draftIds.entries()) {
    const d = draftStore.get(draftId);
    if (!d) continue;
    parts += 1;
    const first = table.pages.length;
    if (d.mode === 'archive') {
      const where = await saveFile(bookId, `part-${parts}.bin`, d.files[0]);
      if (!where) throw new Error(`${d.file_name} could not be kept on this device.`);
      for (const { s, ...rest } of d.pages) table.pages.push({ ...rest, z: parts, w: where });
    } else {
      for (const p of d.pages) {
        const name = `p${parts}-${p.f}`;
        const where = await saveFile(bookId, name, d.files[p.s]);
        if (!where) throw new Error(`${d.file_name} could not be kept on this device.`);
        const { s, ...rest } = p;
        table.pages.push({ ...rest, f: name, w: where });
      }
    }
    for (const c of d.chapters) {
      chapters.push({
        name: String(c.name ?? ''),
        number: typeof c.number === 'number' && Number.isFinite(c.number) ? c.number : null,
        first: first + c.first,
        count: c.count,
      });
      added += 1;
    }
    size += d.size || 0;
    draftStore.delete(draftId);
    onProgress(k + 1, draftIds.length);
  }
  const updated = { ...record, chapters, size, parts, page_count: table.pages.length };
  await dbPut('pages', bookId, table);
  await dbPut('books', bookId, updated);
  return { record: updated, added };
}

export const coverUrls = new Map(); // book id -> object URL
