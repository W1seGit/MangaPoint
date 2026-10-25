// Everything the website keeps on this device, and nothing anywhere else.
//
//  * IndexedDB `mangapoint`:  books (what the library shows), pages (the page
//    table of each book), state (reading progress by fingerprint), covers
//    (thumbnails), meta (small settings, the saved-file handle), files (the
//    fallback home of a book's bytes when the browser has no OPFS).
//  * OPFS (origin private file system): books/<id>/<name>. The imported files
//    themselves, written as a stream and read back by slices.
//
// Works on the page and in a Web Worker.

const DB_NAME = 'mangapoint';
const DB_VERSION = 1;
const STORES = ['books', 'pages', 'state', 'covers', 'meta', 'files'];

let dbPromise = null;

function openDb() {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('This browser has no IndexedDB.'));
      return;
    }
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      for (const name of STORES) {
        if (!db.objectStoreNames.contains(name)) db.createObjectStore(name);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error('IndexedDB would not open.'));
    req.onblocked = () => reject(new Error('IndexedDB is blocked by another tab.'));
  }).catch((e) => {
    dbPromise = null;
    throw e;
  });
  return dbPromise;
}

function wrap(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function dbGet(store, key) {
  const db = await openDb();
  return wrap(db.transaction(store).objectStore(store).get(key));
}

export async function dbPut(store, key, value) {
  const db = await openDb();
  const tx = db.transaction(store, 'readwrite');
  tx.objectStore(store).put(value, key);
  await new Promise((resolve, reject) => {
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error || new Error('write aborted'));
  });
}

export async function dbDelete(store, key) {
  const db = await openDb();
  const tx = db.transaction(store, 'readwrite');
  tx.objectStore(store).delete(key);
  await new Promise((resolve, reject) => {
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
  });
}

/** Every [key, value] of a store. */
export async function dbAll(store) {
  const db = await openDb();
  const os = db.transaction(store).objectStore(store);
  const [keys, values] = await Promise.all([wrap(os.getAllKeys()), wrap(os.getAll())]);
  return keys.map((k, i) => [k, values[i]]);
}

export async function dbClear() {
  const db = await openDb();
  const tx = db.transaction(STORES, 'readwrite');
  for (const s of STORES) tx.objectStore(s).clear();
  await new Promise((resolve, reject) => {
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
  });
}

// ---- writes that wait a moment (progress changes all the time) ---------------

const pending = new Map(); // `${store}\u0000${key}` -> { store, key, value }
let timer = 0;

/** Remember a value soon; many calls for one key become one write. */
export function dbPutSoon(store, key, value, delay = 800) {
  pending.set(`${store}\u0000${key}`, { store, key, value });
  if (!timer) timer = setTimeout(flushPending, delay);
}

export async function flushPending() {
  clearTimeout(timer);
  timer = 0;
  const batch = [...pending.values()];
  pending.clear();
  for (const { store, key, value } of batch) {
    try {
      await dbPut(store, key, value);
    } catch (e) {
      console.warn('could not save', store, key, e);
    }
  }
}

// ---- files -------------------------------------------------------------------

let forceNoOpfs = false;
/** Test hook: pretend this browser cannot keep files. */
export function simulateNoStorage(on) {
  forceNoOpfs = on;
}

let opfsRoot = null;

async function booksDir(create = true) {
  if (!opfsRoot) {
    if (!navigator.storage || !navigator.storage.getDirectory) throw new Error('no OPFS');
    opfsRoot = await navigator.storage.getDirectory();
  }
  return opfsRoot.getDirectoryHandle('books', { create });
}

export async function opfsAvailable() {
  if (forceNoOpfs) return false;
  try {
    const dir = await booksDir();
    const h = await dir.getFileHandle('.probe', { create: true });
    if (typeof h.createWritable !== 'function') return false;
    const w = await h.createWritable();
    await w.write(new Uint8Array([1]));
    await w.close();
    await dir.removeEntry('.probe');
    return true;
  } catch (_) {
    return false;
  }
}

/**
 * Keep `blob` as file `name` of book `id`. Resolves to 'opfs' or 'idb', or
 * null when the browser would not keep it (quota, private window).
 */
export async function saveFile(id, name, blob) {
  if (!forceNoOpfs) {
    try {
      const dir = await (await booksDir()).getDirectoryHandle(id, { create: true });
      const fh = await dir.getFileHandle(name, { create: true });
      if (typeof fh.createWritable === 'function') {
        const w = await fh.createWritable();
        await blob.stream().pipeTo(w);
        return 'opfs';
      }
    } catch (e) {
      console.warn('OPFS write failed, trying IndexedDB', e);
      try {
        const dir = await (await booksDir()).getDirectoryHandle(id);
        await dir.removeEntry(name);
      } catch (_) { /* nothing to clean */ }
    }
    if (forceNoOpfs) return null;
  }
  if (forceNoOpfs) return null;
  try {
    await dbPut('files', `${id}/${name}`, blob);
    return 'idb';
  } catch (e) {
    console.warn('IndexedDB write failed', e);
    return null;
  }
}

/** The stored file as a File/Blob (read lazily, by slices), or null. */
export async function loadFile(id, name, where) {
  if (where === 'opfs') {
    const dir = await (await booksDir(false)).getDirectoryHandle(id);
    const fh = await dir.getFileHandle(name);
    return fh.getFile();
  }
  if (where === 'idb') {
    return (await dbGet('files', `${id}/${name}`)) || null;
  }
  return null;
}

/** Delete everything kept for a book. */
export async function removeBookFiles(id) {
  try {
    const dir = await booksDir(false);
    await dir.removeEntry(id, { recursive: true });
  } catch (_) { /* not there */ }
  try {
    const all = await dbAll('files');
    for (const [key] of all) {
      if (typeof key === 'string' && key.startsWith(`${id}/`)) await dbDelete('files', key);
    }
  } catch (_) { /* not there */ }
}

export async function removeAllFiles() {
  try {
    const root = await (navigator.storage.getDirectory());
    await root.removeEntry('books', { recursive: true });
  } catch (_) { /* not there */ }
}

export async function usage() {
  try {
    const e = await navigator.storage.estimate();
    return { used: e.usage || 0, quota: e.quota || 0 };
  } catch (_) {
    return { used: 0, quota: 0 };
  }
}
