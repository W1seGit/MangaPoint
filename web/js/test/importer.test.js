// Run with: node --test web/js/test
// The importer with the real engine WebAssembly (skipped when web/dist is not built).
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { engine, analyze, looksLikeImage, naturalCompare } from '../importer.js';
import { makeZip, tinyPng } from './zipwriter.js';

const dist = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'dist', 'engine');
const built = existsSync(join(dist, 'web_engine_bg.wasm'));

test('picture signatures', () => {
  assert.ok(looksLikeImage(tinyPng()));
  assert.ok(looksLikeImage(Uint8Array.from([0xff, 0xd8, 0xff, 0xe0, 0, 0])));
  assert.ok(!looksLikeImage(Buffer.from('plain text file')));
  assert.ok(!looksLikeImage(null));
  assert.ok(!looksLikeImage(Uint8Array.from([1, 2])));
});

test('natural order', () => {
  const names = ['10.jpg', '2.jpg', '1.jpg', 'a/3.jpg'].sort(naturalCompare);
  assert.deepEqual(names, ['1.jpg', '2.jpg', '10.jpg', 'a/3.jpg']);
});

test('analyze: chapters, bad files and unsupported types, with the real engine', { skip: !built && 'build web/dist first' }, async () => {
  const wasm = await import(`file:///${join(dist, 'web_engine.js').replaceAll('\\', '/')}`);
  wasm.initSync({ module: readFileSync(join(dist, 'web_engine_bg.wasm')) });
  engine.fingerprint = (size, head, tail) => wasm.book_fingerprint_head_tail(size, head, tail);
  engine.guessChapters = (json) => wasm.guess_chapters_json(json);
  const png = tinyPng();
  const file = (name, buf) => ({ file: new File([buf], name), path: name });
  const book = makeZip([
    { name: 'Ch 1/001.png', data: png }, { name: 'Ch 1/002.png', data: png },
    { name: 'Ch 2/001.png', data: png, method: 'deflate' },
    { name: '../evil.png', data: png },
  ]);
  const r = await analyze([
    file('Series.cbz', book),
    file('x.rar', Buffer.from('Rar!')),
    file('y.pdf', Buffer.from('%PDF')),
    file('broken.cbz', Buffer.from('not a zip at all, sorry')),
    file('fake.zip', makeZip([{ name: '1.jpg', data: 'not a picture' }])),
  ]);
  assert.equal(r.drafts.length, 1);
  const d = r.drafts[0];
  assert.equal(d.page_count, 3);
  assert.equal(d.chapters.length, 2);
  assert.deepEqual(d.chapters.map((c) => c.count), [2, 1]);
  assert.match(d.fingerprint, /^[0-9a-f]{64}$/);
  assert.ok(d.warnings.some((w) => /unsafe/.test(w)));
  const msgs = r.failures.map((f) => `${f.name}: ${f.message}`).join('\n');
  assert.match(msgs, /x\.rar: RAR.*not supported on the web yet/);
  assert.match(msgs, /y\.pdf: PDF.*not supported on the web yet/);
  assert.match(msgs, /broken\.cbz: .*not a ZIP/);
  assert.match(msgs, /fake\.zip: .*not pictures/);
});
