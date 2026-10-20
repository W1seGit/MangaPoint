// The export runs here, off the page, so the page never freezes. It loads the
// engine's converter (its own WebAssembly, only fetched when an export starts),
// reads the pages of the chosen chapters from the kept book one chapter at a
// time, and hands back the finished files. Cancelling terminates this worker.
import { BookSource } from './pagesource.js';
import { dbGet } from './store.js';

async function run(job) {
  const { options, book, chapters, session } = job;
  post({ t: 'status', message: 'Starting the converter…' });
  const engine = await import('../engine/web_engine.js');
  await engine.default();
  const exporter = new engine.Exporter(JSON.stringify(options));
  const [record, table] = await Promise.all([dbGet('books', book), dbGet('pages', book)]);
  if (!record || !table) throw new Error('This book is not in the library any more.');
  const source = new BookSource(record, table, session);
  const total = chapters.length;
  let pagesMade = 0;
  const started = Date.now();
  for (const [i, ch] of chapters.entries()) {
    const label = ch.name || `Chapter ${i + 1}`;
    const pages = [];
    for (let k = 0; k < ch.count; k++) {
      pages.push(await source.bytes(ch.first + k));
      if (k % 5 === 0) post({ t: 'progress', chapter: i, total, label, stage: 'reading', fraction: (i + (0.4 * k) / ch.count) / total, elapsed: Date.now() - started });
    }
    post({ t: 'progress', chapter: i, total, label, stage: 'converting', fraction: (i + 0.4) / total, elapsed: Date.now() - started });
    const made = exporter.add_chapter(label, ch.number == null ? NaN : ch.number, pages);
    pagesMade += Number(made) || 0;
    post({ t: 'progress', chapter: i + 1, total, label, stage: 'done', fraction: (i + 1) / total, elapsed: Date.now() - started });
  }
  post({ t: 'status', message: 'Writing the files…' });
  exporter.finish();
  const n = exporter.file_count();
  for (let i = 0; i < n; i++) {
    const f = exporter.take_file(i);
    const bytes = f.bytes;
    self.postMessage({ t: 'file', name: f.name, bytes }, [bytes.buffer]);
  }
  post({ t: 'done', pages: pagesMade });
}

/** Convert a few pages of a chapter to look at (not to save). */
async function preview(job) {
  const { id, options, book, first, count, session } = job;
  const engine = await import('../engine/web_engine.js');
  await engine.default();
  if (book === 'sample') {
    // The made-up pages of the Export screen when no book is imported.
    const made = engine.preview_pages(JSON.stringify(options), engine.sample_pages());
    const images = [];
    for (const m of made) images.push(m);
    self.postMessage({ t: 'preview', id, images }, images.map((b) => b.buffer));
    return;
  }
  const [record, table] = await Promise.all([dbGet('books', book), dbGet('pages', book)]);
  if (!record || !table) throw new Error('This book is not in the library any more.');
  const source = new BookSource(record, table, session);
  const pages = [];
  for (let k = 0; k < count; k++) pages.push(await source.bytes(first + k));
  const made = engine.preview_pages(JSON.stringify(options), pages);
  const images = [];
  for (const m of made) images.push(m);
  self.postMessage({ t: 'preview', id, images }, images.map((b) => b.buffer));
}

function post(m) {
  self.postMessage(m);
}

self.onmessage = (e) => {
  if (e.data.cmd === 'sample') {
    import('../engine/web_engine.js').then(async (engine) => {
      await engine.default();
      const images = [];
      for (const m of engine.sample_pages()) images.push(m);
      self.postMessage({ t: 'sample', images }, images.map((b) => b.buffer));
    }).catch((err) => post({ t: 'sample_error', message: String(err && err.message || err) }));
    return;
  }
  if (e.data.cmd === 'preview') {
    preview(e.data).catch((err) => {
      post({ t: 'preview_error', id: e.data.id, message: err && err.message ? err.message : String(err) });
    });
    return;
  }
  if (e.data.cmd !== 'start') return;
  run(e.data).catch((err) => {
    post({ t: 'error', message: err && err.message ? err.message : String(err) });
  });
};
