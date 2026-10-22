// The bridge between the page (Rust, Dioxus) and the browser.
//
// Rust calls `mp.call(name, json)` (a Promise of a JSON string) or
// `mp.send(name, json)` (at once; needed where the browser wants a click to
// start the work, such as opening a file picker). The browser tells Rust what
// happened through a queue of events that Rust waits on: `mp.call('events')`
// resolves as soon as there is something to say.
//
// Nothing here makes a network request for the visitor's data.
import * as store from './store.js';
import * as importer from './importer.js';
import * as pages from './pages.js';
import * as router from './router.js';
import * as fsa from './fsa.js';
import { installReader, commands, setCoverLoader } from './reader.js';

// ---- events to Rust --------------------------------------------------------------------

const queue = [];
let waiter = null;

export function emit(ev) {
  // Position reports replace each other: only the latest matters.
  if (ev.t === 'pos') {
    const i = queue.findIndex((q) => q.t === 'pos');
    if (i >= 0) queue.splice(i, 1);
  }
  queue.push(ev);
  if (waiter) {
    const w = waiter;
    waiter = null;
    w();
  }
}

async function events() {
  if (!queue.length) await new Promise((resolve) => (waiter = resolve));
  await new Promise((resolve) => setTimeout(resolve, 8)); // let a burst arrive together
  return queue.splice(0);
}

// ---- importing --------------------------------------------------------------------------

let analyzing = Promise.resolve();

function startAnalyze(items, only = null, into = null) {
  if (!items.length) return;
  analyzing = analyzing.then(async () => {
    emit({ t: 'analyzing', status: 'Looking at what you picked…' });
    try {
      const r = await importer.analyze(items, (status) => emit({ t: 'analyzing', status }));
      if (only) {
        // The Import button of one card takes that book's file and nothing else.
        const keep = r.drafts.filter((d) => d.fingerprint === only);
        for (const d of r.drafts) if (d.fingerprint !== only) importer.dropDraft(d.draft_id);
        r.drafts = keep;
        if (!keep.length && !r.failures.length) {
          r.failures.push({ name: items[0].file.name, message: 'This is not the file for that book. Pick the same file you imported before.' });
        }
      }
      emit({ t: 'import', drafts: r.drafts, failures: r.failures, into });
    } catch (e) {
      console.error(e);
      emit({ t: 'import', drafts: [], failures: [{ name: items[0].file.name, message: `This could not be read: ${e && e.message ? e.message : e}` }] });
    }
  });
}

function pickFiles(folder, only = null, into = null) {
  const input = document.createElement('input');
  input.type = 'file';
  input.style.display = 'none';
  if (folder) {
    input.webkitdirectory = true;
  } else {
    input.multiple = !only;
    input.accept = '.cbz,.zip,.epub,.cbr,.rar,.7z,.cb7,.pdf,image/*';
  }
  input.addEventListener('change', () => {
    const items = importer.filesFromInput(input.files || []);
    input.remove();
    startAnalyze(items, only, into);
  });
  input.addEventListener('cancel', () => input.remove());
  document.body.appendChild(input);
  input.click();
}

function installDrop() {
  let depth = 0;
  const hasFiles = (e) => e.dataTransfer && [...(e.dataTransfer.types || [])].includes('Files');
  window.addEventListener('dragenter', (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (depth++ === 0) document.body.classList.add('dragging');
  });
  window.addEventListener('dragover', (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  });
  window.addEventListener('dragleave', (e) => {
    if (!hasFiles(e)) return;
    if (--depth <= 0) {
      depth = 0;
      document.body.classList.remove('dragging');
    }
  });
  window.addEventListener('drop', async (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0;
    document.body.classList.remove('dragging');
    const dt = e.dataTransfer;
    // A single .mpstate file dropped anywhere is a reading-state file.
    const only = dt.files && dt.files.length === 1 ? dt.files[0] : null;
    if (only && /\.mpstate$/i.test(only.name)) {
      emit({ t: 'state_file', name: only.name, text: await only.text() });
      return;
    }
    const items = await importer.filesFromDrop(dt);
    startAnalyze(items);
  });
}

// ---- covers -----------------------------------------------------------------------------

async function coverUrl(id) {
  const known = importer.coverUrls.get(id);
  if (known) return known;
  try {
    const blob = await store.dbGet('covers', id);
    if (!blob) return null;
    const url = URL.createObjectURL(blob);
    importer.coverUrls.set(id, url);
    return url;
  } catch (_) {
    return null;
  }
}

// ---- exporting ----------------------------------------------------------------------------

let worker = null;
let exportResult = []; // [{ name, url, size }]

function freeExport() {
  for (const f of exportResult) URL.revokeObjectURL(f.url);
  exportResult = [];
}

function exportStart(job) {
  exportCancel();
  freeExport();
  worker = new Worker(new URL('./export-worker.js', import.meta.url), { type: 'module' });
  const w = worker;
  w.onmessage = (e) => {
    const m = e.data;
    if (m.t === 'file') {
      const blob = new Blob([m.bytes], { type: 'application/octet-stream' });
      exportResult.push({ name: m.name, url: URL.createObjectURL(blob), size: blob.size });
    } else if (m.t === 'done') {
      emit({ t: 'export_done', files: exportResult.map((f, i) => ({ index: i, name: f.name, size: f.size })), pages: m.pages });
      w.terminate();
      if (worker === w) worker = null;
    } else {
      emit({ ...m, t: `export_${m.t}` });
      if (m.t === 'error') {
        w.terminate();
        if (worker === w) worker = null;
      }
    }
  };
  w.onerror = (e) => {
    emit({ t: 'export_error', message: e.message || 'The export worker stopped.' });
    w.terminate();
    if (worker === w) worker = null;
  };
  const session = importer.sessionFiles.get(job.book) || null;
  w.postMessage({ cmd: 'start', ...job, session });
}

// A second worker only for looking at pages: a newer request replaces an older one still running.
let previewWorker = null;
let previewId = 0;
let previewUrls = [];

function freePreview() {
  for (const u of previewUrls) URL.revokeObjectURL(u);
  previewUrls = [];
}

/** The converted pictures of a few pages of a chapter, as blob: URLs (the last ones are freed). */
function preview({ options, book, first, count }) {
  previewWorker?.terminate();
  const id = ++previewId;
  const w = new Worker(new URL('./export-worker.js', import.meta.url), { type: 'module' });
  previewWorker = w;
  return new Promise((resolve, reject) => {
    w.onmessage = (e) => {
      const m = e.data;
      if (m.id !== id) return;
      w.terminate();
      if (previewWorker === w) previewWorker = null;
      if (m.t === 'preview') {
        freePreview();
        previewUrls = m.images.map((bytes) => URL.createObjectURL(new Blob([bytes], { type: sniff(bytes) })));
        const urls = previewUrls.slice();
        // A strip page comes with the downloaded rows it is made from ('' for the others).
        const strip_urls = (m.strips || []).map((bytes) => {
          if (!bytes) return '';
          const url = URL.createObjectURL(new Blob([bytes], { type: sniff(bytes) }));
          previewUrls.push(url);
          return url;
        });
        resolve({ id, urls, strip_urls, sources: JSON.parse(m.sources || '[]') });
      } else {
        reject(new Error(m.message));
      }
    };
    w.onerror = (e) => {
      w.terminate();
      if (previewWorker === w) previewWorker = null;
      reject(new Error(e.message || 'The preview worker stopped.'));
    };
    const session = importer.sessionFiles.get(book) || null;
    w.postMessage({ cmd: 'preview', id, options, book, first, count, session });
  });
}

// The made-up pages shown on Export before a book is imported: made once, by the engine.
let samplePromise = null;
function sampleUrls() {
  samplePromise ??= new Promise((resolve, reject) => {
    const w = new Worker(new URL('./export-worker.js', import.meta.url), { type: 'module' });
    w.onmessage = (e) => {
      w.terminate();
      if (e.data.t === 'sample') resolve(e.data.images.map((b) => URL.createObjectURL(new Blob([b], { type: 'image/png' }))));
      else reject(new Error(e.data.message));
    };
    w.onerror = (e) => { w.terminate(); reject(new Error(e.message || 'The sample worker stopped.')); };
    w.postMessage({ cmd: 'sample' });
  }).catch((err) => { samplePromise = null; throw err; });
  return samplePromise;
}

function sniff(bytes) {
  return bytes[0] === 0xff && bytes[1] === 0xd8 ? 'image/jpeg' : 'image/png';
}

function exportCancel() {
  if (worker) {
    worker.terminate();
    worker = null;
    emit({ t: 'export_cancelled' });
  }
}

function downloadResult(index) {
  const f = exportResult[index];
  if (!f) return false;
  const a = document.createElement('a');
  a.href = f.url;
  a.download = f.name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  return true;
}

// ---- service worker ---------------------------------------------------------------------------

let swRegistration = null;
let installPrompt = null;

async function registerServiceWorker() {
  if (!('serviceWorker' in navigator)) return;
  const secure = location.protocol === 'https:' || location.hostname === 'localhost' || location.hostname === '127.0.0.1';
  if (!secure) return;
  try {
    swRegistration = await navigator.serviceWorker.register(new URL('../sw.js', import.meta.url).href, { scope: new URL('../', import.meta.url).pathname });
    const waitingCheck = (reg) => {
      if (reg.waiting && navigator.serviceWorker.controller) emit({ t: 'sw_update' });
    };
    waitingCheck(swRegistration);
    swRegistration.addEventListener('updatefound', () => {
      const nw = swRegistration.installing;
      nw?.addEventListener('statechange', () => {
        if (nw.state === 'installed' && navigator.serviceWorker.controller) emit({ t: 'sw_update' });
      });
    });
    navigator.serviceWorker.ready.then(() => emit({ t: 'offline_ready' }));
  } catch (e) {
    console.warn('service worker not registered', e);
  }
}

function applyUpdate() {
  const reg = swRegistration;
  if (reg && reg.waiting) {
    navigator.serviceWorker.addEventListener('controllerchange', () => location.reload(), { once: true });
    reg.waiting.postMessage({ type: 'skip-waiting' });
  } else {
    location.reload();
  }
}

// ---- the calls --------------------------------------------------------------------------------------

const handlers = {
  events,

  async boot() {
    const params = new URLSearchParams(location.search);
    if (params.has('nostore')) store.simulateNoStorage(true);
    const caps = {
      opfs: await store.opfsAvailable(),
      fsa: fsa.canPickSave(),
      persist: !!(navigator.storage && navigator.storage.persist),
      standalone: matchMedia('(display-mode: standalone)').matches || navigator.standalone === true,
      sw: 'serviceWorker' in navigator,
    };
    let books = [];
    let states = [];
    let meta = {};
    try {
      books = (await store.dbAll('books')).map(([, v]) => v);
      states = (await store.dbAll('state')).map(([, v]) => v);
      // The small records the page keeps: its settings, the readers, the presets and the last exports.
      for (const [k, v] of await store.dbAll('meta')) if (['settings', 'readers', 'presets', 'last_exports'].includes(k)) meta[k] = v;
    } catch (e) {
      return { error: `This browser would not open its local storage (${e && e.message ? e.message : e}). Books can be read in this visit but will not be kept.`, books, states, meta, caps };
    }
    // Books whose file the browser could not keep and that are not in this visit are only a name.
    for (const b of books) {
      if (b.stored === 'none' && !importer.sessionFiles.has(b.id)) b.needs_file = true;
    }
    return { books, states, meta, caps, persisted: await (navigator.storage?.persisted?.() ?? false) };
  },

  /** Keep the files of the last export as books in the library, as if they had been imported. */
  async export_to_library() {
    const books = [];
    const failures = [];
    for (const f of exportResult) {
      const blob = await (await fetch(f.url)).blob();
      const file = new File([blob], f.name);
      const r = await importer.analyze([{ file, path: f.name }]);
      failures.push(...r.failures);
      for (const d of r.drafts) books.push(await importer.commit(d.draft_id, {}));
    }
    return { books, failures };
  },

  /** Add a reviewed draft's chapters (as the visitor left them) to the book `book`. */
  async append({ book, draft_id, chapters }) {
    const done = await importer.appendTo(book, [draft_id], { [draft_id]: chapters });
    pages.forgetSource(book);
    return done.record;
  },

  async commit({ draft_id, title, chapters }) {
    const rec = await importer.commit(draft_id, { title, chapters }, (done, total) => emit({ t: 'commit_progress', done, total }));
    return rec;
  },

  discard({ draft_id }) {
    importer.dropDraft(draft_id);
    return true;
  },

  async remove_book({ id }) {
    pages.forgetSource(id);
    await store.removeBookFiles(id);
    await store.dbDelete('books', id);
    await store.dbDelete('pages', id);
    await store.dbDelete('covers', id);
    const u = importer.coverUrls.get(id);
    if (u) URL.revokeObjectURL(u);
    importer.coverUrls.delete(id);
    importer.sessionFiles.delete(id);
    return true;
  },

  async db_delete({ store: s, key }) {
    await store.dbDelete(s, key);
    return true;
  },

  async flush() {
    await store.flushPending();
    return true;
  },

  async storage() {
    const u = await store.usage();
    return { ...u, persisted: await (navigator.storage?.persisted?.() ?? false), opfs: await store.opfsAvailable() };
  },

  async persist() {
    try {
      return await navigator.storage.persist();
    } catch (_) {
      return false;
    }
  },

  async clear_all() {
    exportCancel();
    freeExport();
    pages.dropAllUrls();
    await store.flushPending().catch(() => {});
    await store.removeAllFiles();
    await store.dbClear();
    for (const u of importer.coverUrls.values()) URL.revokeObjectURL(u);
    importer.coverUrls.clear();
    importer.sessionFiles.clear();
    return true;
  },

  async save_state({ text, name, choose }) {
    return fsa.saveState(text, name, !!choose);
  },

  async state_file_name() {
    return fsa.savedName();
  },

  async forget_state_file() {
    await fsa.forgetHandle();
    return true;
  },

  async preview(args) {
    return preview(args);
  },

  async preview_free() {
    previewWorker?.terminate();
    previewWorker = null;
    freePreview();
    return true;
  },

  /** Blob URL of the picture of page `i` as it is stored (acquired until preview_free_original). */
  async original_url({ book, index }) {
    if (book === 'sample') return (await sampleUrls())[index];
    return pages.acquire(book, index);
  },

  async original_release({ book, index }) {
    if (book === 'sample') return true;
    pages.release(book, index);
    return true;
  },

  async export_status() {
    return { files: exportResult.map((f, i) => ({ index: i, name: f.name, size: f.size })), running: !!worker };
  },

  /** Size the page list of the Export screen and report the pages in view (the shared editor). */
  async editor_start() {
    await import('./ui/preview-editor.js');
    window.MP.preview_editor.start((m) => emit({ t: 'editor', first: m[0], last: m[1], current: m[2] }));
    return true;
  },

  /** Scroll the list (and the thumbnails) to page `index`. */
  async editor_scroll(args) {
    await import('./ui/preview-editor.js');
    window.MP.preview_editor.scroll(args);
    return true;
  },

  async editor_thumb(args) {
    await import('./ui/preview-editor.js');
    window.MP.preview_editor.thumb(args);
    return true;
  },

  async editor_top() {
    await import('./ui/preview-editor.js');
    window.MP.preview_editor.top();
    return true;
  },

  /** The size in pixels of each picture at these URLs. */
  async image_sizes({ urls }) {
    return Promise.all(urls.map((u) => new Promise((resolve) => {
      if (!u) return resolve([0, 0]);
      const img = new Image();
      img.onload = () => resolve([img.naturalWidth, img.naturalHeight]);
      img.onerror = () => resolve([0, 0]);
      img.src = u;
    })));
  },

  export_cancel() {
    exportCancel();
    return true;
  },

  async download_all() {
    for (let i = 0; i < exportResult.length; i++) {
      downloadResult(i);
      await new Promise((r) => setTimeout(r, 400));
    }
    return true;
  },

  free_export() {
    freeExport();
    return true;
  },

  apply_update() {
    applyUpdate();
    return true;
  },

  async install() {
    if (!installPrompt) return 'unavailable';
    installPrompt.prompt();
    const r = await installPrompt.userChoice;
    installPrompt = null;
    return r.outcome;
  },

  reader({ cmd, args }) {
    const f = commands[cmd];
    if (f) f(...(args || []));
    return true;
  },

  sleep({ ms }) {
    return new Promise((r) => setTimeout(() => r(true), ms));
  },

  async debug() {
    return { urls: pages.liveUrlCount(), heap: performance.memory ? performance.memory.usedJSHeapSize : 0 };
  },
};

// Calls that must start inside the click that asked for them.
const immediate = {
  pick_files: () => pickFiles(false),
  pick_folder: () => pickFiles(true),
  pick_for: ({ fingerprint }) => pickFiles(false, fingerprint),
  pick_more: ({ book }) => pickFiles(false, null, book),
  pick_state: () => {
    fsa.pickStateFile().then((f) => {
      if (f) emit({ t: 'state_file', name: f.name, text: f.text });
    });
  },
  export_start: (job) => exportStart(job),
  db_put_soon: ({ store: s, key, value }) => store.dbPutSoon(s, key, value),
  reader: ({ cmd, args }) => {
    const f = commands[cmd];
    if (f) f(...(args || []));
  },
  download_result: ({ index }) => downloadResult(index),
  blur: () => document.activeElement?.blur?.(),
  reload: () => location.reload(),
};

// Run at once (`send`): the page asks the browser to size or scroll something it just drew.
immediate.route = ({ path, hash }) => router.go(path, hash);
immediate.route_fix = ({ path, hash }) => router.fix(path, hash);
// The page is ready for an address: say which one it was opened at, and follow the history after that.
immediate.route_ready = () => {
  router.watch((r) => emit({ t: 'route', path: r.path, hash: r.hash }));
  const r = router.current();
  emit({ t: 'route', path: r.path, hash: r.hash });
};
for (const name of ['apply_update', 'editor_start', 'editor_scroll', 'editor_thumb', 'editor_top']) immediate[name] = handlers[name];
// One of the scripts shared with the desktop app (crates/ui/js, copied to js/ui/): loaded the first
// time it is asked for, then started. Each registers window.MP[name].
immediate.run_script = ({ name, file, args }) => {
  import(`./ui/${file}`)
    .then(() => window.MP[name].start(args || {}))
    .catch((e) => console.error(`script ${name}:`, e));
};

const mp = {
  async call(name, json) {
    const h = handlers[name];
    if (!h) throw new Error(`unknown call ${name}`);
    const args = json ? JSON.parse(json) : {};
    return JSON.stringify((await h(args)) ?? null);
  },
  send(name, json) {
    const h = immediate[name];
    if (!h) return 'unknown';
    h(json ? JSON.parse(json) : {});
    return 'ok';
  },
  /** The page's WebAssembly gives the engine's functions to the importer. */
  setEngine(wasm) {
    importer.engine.fingerprint = (size, head, tail) => wasm.mp_fingerprint(size, head, tail);
    importer.engine.guessChapters = (json) => wasm.mp_guess_chapters(json);
  },
};

window.mp = mp;
window.__mp = { store, importer, pages, emit };

installReader(emit);
installDrop();
setCoverLoader(coverUrl);

window.addEventListener('pagehide', () => store.flushPending());
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'hidden') store.flushPending();
});
{
  const narrow = matchMedia('(max-width: 900px)');
  emit({ t: 'narrow', narrow: narrow.matches });
  narrow.addEventListener('change', (e) => emit({ t: 'narrow', narrow: e.matches }));
}
window.addEventListener('online', () => emit({ t: 'online', online: true }));
window.addEventListener('offline', () => emit({ t: 'online', online: false }));
window.addEventListener('beforeinstallprompt', (e) => {
  e.preventDefault();
  installPrompt = e;
  emit({ t: 'can_install' });
});
window.addEventListener('appinstalled', () => emit({ t: 'installed' }));
registerServiceWorker();

export default mp;
