// Run with: node --test web/js/test
import test from 'node:test';
import assert from 'node:assert/strict';
import { openZip, ZipError, LIMITS, safePath } from '../zip.js';
import { makeZip } from './zipwriter.js';

const blobOf = (buf) => new Blob([buf]);
const text = async (blob) => Buffer.from(await blob.arrayBuffer()).toString('utf8');

test('reads stored and deflated entries', async () => {
  const body = 'hello '.repeat(5000);
  const zip = await openZip(blobOf(makeZip([
    { name: 'a.txt', data: 'stored one', method: 'store' },
    { name: 'dir/b.txt', data: body, method: 'deflate' },
  ])));
  assert.deepEqual(zip.entries.map((e) => e.name), ['a.txt', 'dir/b.txt']);
  assert.equal(await text(await zip.read(zip.entries[0])), 'stored one');
  assert.equal(await text(await zip.read(zip.entries[1])), body);
  assert.equal(zip.entries[1].usize, body.length);
  assert.ok(zip.entries[1].csize < body.length / 10);
});

test('reads ZIP64 archives', async () => {
  const zip = await openZip(blobOf(makeZip([
    { name: 'one.bin', data: 'abc', method: 'store' },
    { name: 'two.bin', data: 'x'.repeat(1000), method: 'deflate' },
  ], { zip64: true })));
  assert.equal(zip.entries.length, 2);
  assert.equal(await text(await zip.read(zip.entries[0])), 'abc');
  assert.equal((await zip.read(zip.entries[1])).size, 1000);
});

test('reads an archive with bytes in front of it', async () => {
  const zip = await openZip(blobOf(makeZip([{ name: 'x.txt', data: 'payload' }], { prefix: Buffer.from('MZ'.repeat(300)) })));
  assert.equal(await text(await zip.read(zip.entries[0])), 'payload');
});

test('names: UTF-8 flag, and the DOS code page without it', async () => {
  const zip = await openZip(blobOf(makeZip([
    { name: '第1話/001.jpg', data: 'x' },
    { nameBytes: Buffer.from([0x82, 0x2f, 0x41, 0x2e, 0x6a, 0x70, 0x67]), data: 'y', utf8: false },
    { nameBytes: Buffer.from([0xff, 0xfe, 0x2e, 0x6a, 0x70, 0x67]), data: 'z', utf8: true },
  ])));
  assert.equal(zip.entries[0].name, '第1話/001.jpg');
  assert.equal(zip.entries[1].name, 'é/A.jpg');
  assert.equal(zip.entries.length, 3); // a damaged UTF-8 name is still listed
});

test('a long comment after the end record is fine', async () => {
  const zip = await openZip(blobOf(makeZip([{ name: 'a', data: 'b' }], { comment: 'c'.repeat(60000) })));
  assert.equal(zip.entries.length, 1);
});

test('lists 20,000 entries and reads one from the middle', async () => {
  const files = [];
  for (let i = 0; i < 20000; i++) files.push({ name: `p/${String(i).padStart(5, '0')}.jpg`, data: `page ${i}`, method: i % 2 ? 'deflate' : 'store' });
  const zip = await openZip(blobOf(makeZip(files)));
  assert.equal(zip.entries.length, 20000);
  assert.equal(await text(await zip.read(zip.entries[12345])), 'page 12345');
  assert.equal(await text(await zip.read(zip.entries[12346])), 'page 12346');
});

test('refuses an absurd number of entries before reading them', async () => {
  const buf = makeZip([{ name: 'a', data: 'b' }]);
  // Claim 300,000 entries in the end record.
  const at = buf.length - 22;
  buf.writeUInt16LE(60000, at + 8);
  buf.writeUInt16LE(60000, at + 10);
  const old = LIMITS.maxEntries;
  LIMITS.maxEntries = 1000;
  try {
    await assert.rejects(openZip(blobOf(buf)), (e) => e instanceof ZipError && /more than the website can handle/.test(e.message));
  } finally {
    LIMITS.maxEntries = old;
  }
});

test('garbage, empty and truncated files give a ZipError, not a crash', async () => {
  await assert.rejects(openZip(blobOf(Buffer.from('this is not a zip file at all, just some text'))), ZipError);
  await assert.rejects(openZip(blobOf(Buffer.alloc(0))), ZipError);
  await assert.rejects(openZip(blobOf(Buffer.alloc(100000, 7))), ZipError);
  const good = makeZip([{ name: 'a.jpg', data: 'x'.repeat(5000), method: 'deflate' }, { name: 'b.jpg', data: 'y' }]);
  await assert.rejects(openZip(blobOf(good.subarray(0, good.length - 30))), ZipError);
  await assert.rejects(openZip(blobOf(good.subarray(0, 40))), ZipError);
});

test('a damaged central directory is refused', async () => {
  const good = makeZip([{ name: 'a.jpg', data: 'x' }, { name: 'b.jpg', data: 'y' }]);
  const bad = Buffer.from(good);
  const cd = bad.readUInt32LE(bad.length - 22 + 16);
  bad[cd] = 0; // break the first entry's signature
  await assert.rejects(openZip(blobOf(bad)), (e) => e instanceof ZipError && /damaged/.test(e.message));
});

test('a damaged deflate stream and a lying size are errors when read', async () => {
  const buf = makeZip([{ name: 'a.bin', data: 'abcdefgh'.repeat(2000), method: 'deflate' }]);
  const zip0 = await openZip(blobOf(buf));
  const start = await zip0.dataStart(zip0.entries[0]);
  const bad = Buffer.from(buf);
  for (let i = start + 4; i < start + 24; i++) bad[i] ^= 0x5a;
  const zip = await openZip(blobOf(bad));
  await assert.rejects(zip.read(zip.entries[0]), ZipError);
  // An entry that declares 10 bytes but unpacks to 100,000 (a bomb).
  const bomb = makeZip([{ name: 'bomb.bin', data: Buffer.alloc(100000), method: 'deflate' }]);
  const b = await openZip(blobOf(bomb));
  b.entries[0].usize = 10;
  await assert.rejects(b.read(b.entries[0]), ZipError);
  // An entry that declares more than the cap is refused without reading.
  b.entries[0].usize = LIMITS.maxEntryBytes + 1;
  await assert.rejects(b.read(b.entries[0]), /too big/);
});

test('head() returns only the first bytes of a deflated entry', async () => {
  const zip = await openZip(blobOf(makeZip([{ name: 'a', data: Buffer.alloc(1 << 20, 65), method: 'deflate' }])));
  const head = await zip.head(zip.entries[0], 100);
  assert.equal(head.length, 100);
  assert.equal(head[0], 65);
});

test('encrypted and unknown-method entries are flagged, not read', async () => {
  const buf = makeZip([{ name: 'a.jpg', data: 'x' }]);
  const zip = await openZip(blobOf(buf));
  zip.entries[0].encrypted = true;
  await assert.rejects(zip.read(zip.entries[0]), /password/);
  zip.entries[0].encrypted = false;
  zip.entries[0].method = 99;
  await assert.rejects(zip.read(zip.entries[0]), /packed in a way/);
});

test('safePath refuses traversal and absolute paths', () => {
  assert.ok(safePath('ch1/001.jpg'));
  assert.ok(!safePath('../evil.jpg'));
  assert.ok(!safePath('a/../../evil.jpg'));
  assert.ok(!safePath('/abs/x.jpg'));
  assert.ok(!safePath('C:\\x.jpg'));
  assert.ok(!safePath('a\\..\\b.jpg'));
});
