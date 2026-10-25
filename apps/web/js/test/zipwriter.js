// A small ZIP writer for the tests: it makes the archives the reader has to
// cope with (stored, deflated, ZIP64, odd names, damaged) so no binary fixtures
// are kept in the repository.
import { deflateRawSync, deflateSync } from 'node:zlib';

function crc32(buf) {
  let c;
  let crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

const le = (n, bytes) => {
  const b = Buffer.alloc(bytes);
  if (bytes === 8) b.writeBigUInt64LE(BigInt(n));
  else if (bytes === 4) b.writeUInt32LE(n >>> 0);
  else b.writeUInt16LE(n);
  return b;
};

/**
 * files: [{ name | nameBytes, data: Buffer|string, method: 'store'|'deflate', utf8?: bool, zip64?: bool }]
 * options: { zip64: force ZIP64 records, prefix: Buffer put in front, comment: string }
 */
export function makeZip(files, options = {}) {
  const parts = [];
  const central = [];
  let offset = 0;
  const prefix = options.prefix ?? Buffer.alloc(0);
  parts.push(prefix);
  for (const f of files) {
    const data = Buffer.from(f.data ?? '');
    const nameBytes = f.nameBytes ?? Buffer.from(f.name, 'utf8');
    const deflate = (f.method ?? 'store') === 'deflate';
    const packed = deflate ? deflateRawSync(data) : data;
    const flags = f.utf8 === false ? 0 : 0x800;
    const crc = crc32(data);
    const local = Buffer.concat([
      le(0x04034b50, 4), le(20, 2), le(flags, 2), le(deflate ? 8 : 0, 2), le(0, 2), le(0x21, 2),
      le(crc, 4), le(packed.length, 4), le(data.length, 4), le(nameBytes.length, 2), le(0, 2),
      nameBytes,
    ]);
    parts.push(local, packed);
    const z64 = f.zip64 || options.zip64;
    let extra = Buffer.alloc(0);
    let csz = packed.length;
    let usz = data.length;
    let off = offset;
    if (z64) {
      extra = Buffer.concat([le(1, 2), le(24, 2), le(data.length, 8), le(packed.length, 8), le(offset, 8)]);
      csz = usz = off = 0xffffffff;
    }
    central.push(Buffer.concat([
      le(0x02014b50, 4), le(45, 2), le(20, 2), le(flags, 2), le(deflate ? 8 : 0, 2), le(0, 2), le(0x21, 2),
      le(crc, 4), le(csz, 4), le(usz, 4), le(nameBytes.length, 2), le(extra.length, 2), le(0, 2),
      le(0, 2), le(0, 2), le(0, 4), le(off, 4), nameBytes, extra,
    ]));
    offset += local.length + packed.length;
  }
  const cd = Buffer.concat(central);
  const cdOffset = offset;
  parts.push(cd);
  offset += cd.length;
  const comment = Buffer.from(options.comment ?? '');
  if (options.zip64) {
    const rec = Buffer.concat([
      le(0x06064b50, 4), le(44, 8), le(45, 2), le(45, 2), le(0, 4), le(0, 4),
      le(files.length, 8), le(files.length, 8), le(cd.length, 8), le(cdOffset, 8),
    ]);
    const loc = Buffer.concat([le(0x07064b50, 4), le(0, 4), le(offset, 8), le(1, 4)]);
    parts.push(rec, loc);
    parts.push(Buffer.concat([
      le(0x06054b50, 4), le(0, 2), le(0, 2), le(0xffff, 2), le(0xffff, 2), le(0xffffffff, 4), le(0xffffffff, 4),
      le(comment.length, 2), comment,
    ]));
  } else {
    parts.push(Buffer.concat([
      le(0x06054b50, 4), le(0, 2), le(0, 2), le(files.length, 2), le(files.length, 2), le(cd.length, 4),
      le(cdOffset, 4), le(comment.length, 2), comment,
    ]));
  }
  return Buffer.concat(parts);
}

/** A tiny valid PNG of the given size (solid gray), for pictures in tests. */
export function tinyPng(w = 2, h = 3) {
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, sum]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // 8 bits, gray
  const raw = Buffer.alloc((w + 1) * h, 0x80);
  for (let y = 0; y < h; y++) raw[y * (w + 1)] = 0;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}
