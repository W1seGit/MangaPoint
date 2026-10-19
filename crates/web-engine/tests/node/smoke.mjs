// Proves the WebAssembly build makes the same files as the native engine.
//
//   node smoke.mjs <wasm-bindgen --target nodejs output dir> <fixture dir>
//
// The fixture comes from `cargo run -p web-engine --example make_fixture -- <dir>`
// (native Rust): generated pages, export options, and the files the native
// Exporter made. This script feeds the same pages to the wasm Exporter and
// compares every file byte for byte, then repeats a few of the small functions.
// Exit code 0 means everything matched.

import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';

const [, , pkgArg, fixtureArg] = process.argv;
if (!pkgArg || !fixtureArg) {
  console.error('usage: node smoke.mjs <nodejs wasm-bindgen output dir> <fixture dir>');
  process.exit(2);
}
const require = createRequire(import.meta.url);
const engine = require(resolve(pkgArg, 'web_engine.js'));
const dir = resolve(fixtureArg);
const read = (rel) => new Uint8Array(readFileSync(join(dir, rel)));
const manifest = JSON.parse(readFileSync(join(dir, 'manifest.json'), 'utf8'));

// A zip records which operating system made it ("version made by", the high
// byte of each central-directory entry): the zip library writes 0 (MS-DOS) when it
// is compiled for Windows and 3 (Unix) elsewhere, including WebAssembly. That byte
// says nothing about the content, so it is set to 0 in both before comparing.
function normalizeZip(bytes) {
  const out = Buffer.from(bytes);
  if (out.length < 22 || out.readUInt32LE(0) !== 0x04034b50) return out;
  let eocd = -1;
  for (let i = out.length - 22; i >= Math.max(0, out.length - 22 - 65535); i--) {
    if (out.readUInt32LE(i) === 0x06054b50) { eocd = i; break; }
  }
  if (eocd < 0) return out;
  const count = out.readUInt16LE(eocd + 10);
  let at = out.readUInt32LE(eocd + 16);
  for (let n = 0; n < count && at + 46 <= out.length; n++) {
    if (out.readUInt32LE(at) !== 0x02014b50) break;
    out[at + 5] = 0;
    at += 46 + out.readUInt16LE(at + 28) + out.readUInt16LE(at + 30) + out.readUInt16LE(at + 32);
  }
  return out;
}

let checks = 0;
const same = (a, b, what) => {
  assert.equal(a.length, b.length, `${what}: ${a.length} bytes vs ${b.length} bytes`);
  assert.ok(Buffer.compare(normalizeZip(a), normalizeZip(b)) === 0, `${what}: the bytes differ`);
  checks++;
};

assert.match(engine.engine_version(), /^\d+\.\d+\.\d+/);

for (const c of manifest.cases) {
  const exporter = new engine.Exporter(c.options);
  for (const ch of c.chapters) {
    const made = exporter.add_chapter(ch.name, ch.number, ch.pages.map(read));
    assert.ok(made >= 1, `${c.name}: ${ch.name} made pages`);
  }
  exporter.finish();
  assert.equal(exporter.file_count(), c.expected.length, `${c.name}: number of files`);
  for (let i = 0; i < c.expected.length; i++) {
    const file = exporter.take_file(i);
    assert.equal(file.name, c.expected[i].name, `${c.name}: file name`);
    same(file.bytes, read(c.expected[i].file), `${c.name}: ${file.name}`);
  }
  assert.throws(() => exporter.take_file(0), /already taken/, 'a file is taken once');
  exporter.free();
  console.log(`ok  ${c.name}`);
}

// Errors arrive as Error objects with a sentence.
assert.throws(() => new engine.Exporter('{"device":"nowhere"}'), (e) => e instanceof Error && /nowhere/.test(e.message));
const bad = new engine.Exporter('{"device":"xteink-x4","format":"XTC"}');
assert.throws(() => bad.add_chapter('x', NaN, [new Uint8Array([1, 2, 3])]), /page 1 of x could not be read/);
bad.free();

// Small functions give the native answers.
const f = manifest.fingerprint;
assert.equal(engine.book_fingerprint_head_tail(f.size, read(f.head), read(f.tail)), f.quick);
const hasher = new engine.Fingerprinter();
hasher.update(read(f.head));
hasher.update(read(f.tail));
assert.equal(hasher.finish(), f.full);
assert.throws(() => hasher.finish(), /already finished/);
hasher.free();
assert.equal(engine.guess_chapters_json(manifest.chapters.entries), manifest.chapters.expected);
assert.equal(engine.state_parse_json(manifest.state.input), manifest.state.expected);
assert.equal(engine.state_parse_json(new TextEncoder().encode(manifest.state.input)), manifest.state.expected);
assert.ok(engine.state_merge_json(manifest.state.input, manifest.state.input).length > 0);
assert.throws(() => engine.state_parse_json('{"format":"mangapoint-state","version":2}'), /newer MangaPoint/);
assert.equal(engine.devices_json(), manifest.devices);
assert.equal(engine.default_options_json('kobo-libra-colour', 'manhwa'), manifest.options.expected);
assert.match(engine.parse_comicinfo_json('<ComicInfo><Series>S</Series></ComicInfo>'), /"series":"S"/);
checks += 12;

console.log(`\nall ${checks} checks passed: the wasm build matches the native engine`);
