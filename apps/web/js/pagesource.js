// Reading one page of a kept book. Used by the reader on the page and by the
// export worker, so it only needs IndexedDB / OPFS and the ZIP reader.
import { ZipFile } from './zip.js';
import { dbGet, loadFile } from './store.js';

export const MIME = {
  jpg: 'image/jpeg', jpeg: 'image/jpeg', jpe: 'image/jpeg', png: 'image/png', webp: 'image/webp',
  gif: 'image/gif', avif: 'image/avif', bmp: 'image/bmp',
};
export const IMAGE_EXT = Object.keys(MIME);

export function extOf(name) {
  const m = /\.([A-Za-z0-9]+)$/.exec(name);
  return m ? m[1].toLowerCase() : '';
}

export function isImageName(name) {
  return IMAGE_EXT.includes(extOf(name));
}

/**
 * Where a page added to a book later is kept ('opfs' or 'idb'), or undefined for the book's own pages.
 * Tables saved before this was called `at` kept it in `w`, which is also the measured width: a string
 * there is the old place, a number a width.
 */
function keptAt(p) {
  return p.at ?? (typeof p.w === 'string' ? p.w : undefined);
}

export class BookSource {
  /**
   * `book`: the library record (id, kind, stored). `table`: its page table.
   * `session`: the File itself when the browser would not keep it.
   */
  constructor(book, table, session = null) {
    this.book = book;
    this.table = table;
    this.session = session;
    this.zip = null;
    this.opening = null;
    this.parts = new Map(); // part number -> Promise<ZipFile>, for chapters added later
  }

  get count() {
    return this.table.pages.length;
  }

  async #archive() {
    if (!this.zip) {
      this.opening ??= (async () => {
        const blob = this.session && !Array.isArray(this.session)
          ? this.session
          : await loadFile(this.book.id, 'book.bin', this.book.stored);
        if (!blob) throw new Error('The file of this book is not on this device any more.');
        this.zip = new ZipFile(blob, []);
      })();
      try {
        await this.opening;
      } catch (e) {
        this.opening = null;
        throw e;
      }
    }
    return this.zip;
  }

  /** The archive a page added later lives in: `part-<z>.bin`, kept where the page says (`at`). */
  async #part(z, where) {
    let opening = this.parts.get(z);
    if (!opening) {
      opening = (async () => {
        const blob = await loadFile(this.book.id, `part-${z}.bin`, where || this.book.stored);
        if (!blob) throw new Error('A part of this book is not on this device any more.');
        return new ZipFile(blob, []);
      })();
      opening.catch(() => this.parts.delete(z));
      this.parts.set(z, opening);
    }
    return opening;
  }

  /** Whether page `p` is a picture inside an archive (the book's own, or a part added later). */
  #inArchive(p) {
    return p.z !== undefined || (this.table.mode === 'archive' && p.f === undefined);
  }

  async #zipOf(p) {
    return p.z !== undefined ? this.#part(p.z, keptAt(p)) : this.#archive();
  }

  /** The picture of page `i` as a typed Blob. */
  async blob(i) {
    const p = this.table.pages[i];
    if (!p) throw new Error('No such page.');
    const type = MIME[extOf(p.n || p.f || '')] || '';
    if (this.#inArchive(p)) {
      const zip = await this.#zipOf(p);
      return zip.read({ name: p.n, method: p.m, csize: p.c, usize: p.u, offset: p.o, encrypted: false }, type);
    }
    // Loose pictures: one stored file per page, or the Files of this session.
    if (Array.isArray(this.session) && p.s !== undefined && !keptAt(p)) return this.session[p.s].slice(0, this.session[p.s].size, type);
    const f = await loadFile(this.book.id, p.f, keptAt(p) || this.book.stored);
    if (!f) throw new Error('A page of this book is not on this device any more.');
    return f.slice(0, f.size, type);
  }

  /** The start of the picture's file (enough to read its size from), unpacked. */
  async start(i, n = 131072) {
    const p = this.table.pages[i];
    if (!p) throw new Error('No such page.');
    if (this.#inArchive(p)) {
      const zip = await this.#zipOf(p);
      return zip.head({ name: p.n, method: p.m, csize: p.c, usize: p.u, offset: p.o, encrypted: false }, n);
    }
    const b = await this.blob(i);
    return new Uint8Array(await b.slice(0, n).arrayBuffer());
  }

  async bytes(i) {
    return new Uint8Array(await (await this.blob(i)).arrayBuffer());
  }
}

/** Read the page table of a book from IndexedDB. */
export async function loadTable(id) {
  return dbGet('pages', id);
}
