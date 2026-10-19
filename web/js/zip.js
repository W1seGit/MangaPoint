// A ZIP reader that never holds a whole archive in memory.
//
// It reads the central directory (the list of entries at the end of the file)
// with a few Blob.slice() calls and then reads each entry on demand, by slicing
// the bytes of that one entry out of the Blob (a File on disk stays on disk) and
// inflating them with DecompressionStream('deflate-raw').
//
// Handles: stored and deflated entries, ZIP64, archives with bytes in front of
// them (self-extracting stubs), names that are UTF-8 or the old DOS code page.
// Refuses: encrypted entries and other compression methods (they are listed but
// flagged, so the caller can say why they were skipped), entries that claim to
// be huge, and archives with absurd numbers of entries.

export class ZipError extends Error {
  constructor(message) {
    super(message);
    this.name = 'ZipError';
  }
}

export const LIMITS = {
  // More entries than this is not a book.
  maxEntries: 250000,
  // The central directory of that many entries is well below this.
  maxDirectoryBytes: 128 * 1024 * 1024,
  // One picture larger than this (once unpacked) is refused.
  maxEntryBytes: 256 * 1024 * 1024,
};

const SIG_EOCD = 0x06054b50;
const SIG_EOCD64 = 0x06064b50;
const SIG_LOC64 = 0x07064b50;
const SIG_CEN = 0x02014b50;
const SIG_LOC = 0x04034b50;

const strictUtf8 = new TextDecoder('utf-8', { fatal: true });
const lenientUtf8 = new TextDecoder('utf-8');
// CP437 is what ZIP means by "no UTF-8 flag". TextDecoder has no cp437, so map
// the upper half by hand.
const CP437 =
  'ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ';

function decodeName(bytes, utf8Flag) {
  try {
    return strictUtf8.decode(bytes);
  } catch (_) {
    if (utf8Flag) return lenientUtf8.decode(bytes); // damaged, but keep going
    let s = '';
    for (const b of bytes) s += b < 0x80 ? String.fromCharCode(b) : CP437[b - 0x80];
    return s;
  }
}

async function readBytes(blob, start, end) {
  if (start < 0 || end > blob.size || end < start) throw new ZipError('This ZIP file is damaged or cut short.');
  return new DataView(await blob.slice(start, end).arrayBuffer());
}

function u64(dv, at) {
  const lo = dv.getUint32(at, true);
  const hi = dv.getUint32(at + 4, true);
  const v = hi * 4294967296 + lo;
  if (!Number.isSafeInteger(v)) throw new ZipError('This ZIP file is damaged.');
  return v;
}

/** Open a ZIP held in `blob` (a Blob or File). Resolves to a ZipFile. */
export async function openZip(blob) {
  const size = blob.size;
  if (size < 22) throw new ZipError('This file is too small to be a ZIP archive.');
  const tailStart = Math.max(0, size - (22 + 65535));
  const tail = await readBytes(blob, tailStart, size);
  let eocd = -1;
  for (let i = tail.byteLength - 22; i >= 0; i--) {
    if (tail.getUint32(i, true) === SIG_EOCD) {
      const commentLen = tail.getUint16(i + 20, true);
      if (i + 22 + commentLen <= tail.byteLength) {
        eocd = i;
        break;
      }
    }
  }
  if (eocd < 0) throw new ZipError('This is not a ZIP file, or it is damaged or cut short.');
  const eocdPos = tailStart + eocd;
  let count = tail.getUint16(eocd + 10, true);
  let cdSize = tail.getUint32(eocd + 12, true);
  let cdOffset = tail.getUint32(eocd + 16, true);
  let zip64 = false;
  if (count === 0xffff || cdSize === 0xffffffff || cdOffset === 0xffffffff) {
    // ZIP64: the locator sits just before the end record.
    if (eocdPos < 20) throw new ZipError('This ZIP file is damaged.');
    const loc = await readBytes(blob, eocdPos - 20, eocdPos);
    if (loc.getUint32(0, true) !== SIG_LOC64) throw new ZipError('This ZIP file is damaged.');
    const at64 = u64(loc, 8);
    const rec = await readBytes(blob, at64, at64 + 56);
    if (rec.getUint32(0, true) !== SIG_EOCD64) throw new ZipError('This ZIP file is damaged.');
    count = u64(rec, 32);
    cdSize = u64(rec, 40);
    cdOffset = u64(rec, 48);
    zip64 = true;
  }
  if (count > LIMITS.maxEntries) {
    throw new ZipError(`This archive lists ${count.toLocaleString('en')} files, which is more than the website can handle.`);
  }
  if (cdSize > LIMITS.maxDirectoryBytes) throw new ZipError('The file list of this archive is too big.');
  // Bytes in front of the archive (a self-extracting stub): everything is shifted.
  const shift = zip64 ? 0 : Math.max(0, eocdPos - cdSize - cdOffset);
  const cd = await readBytes(blob, cdOffset + shift, cdOffset + shift + cdSize);
  const entries = [];
  let p = 0;
  for (let n = 0; n < count; n++) {
    if (p + 46 > cd.byteLength || cd.getUint32(p, true) !== SIG_CEN) {
      throw new ZipError('The file list of this ZIP is damaged.');
    }
    const flags = cd.getUint16(p + 8, true);
    const method = cd.getUint16(p + 10, true);
    const crc = cd.getUint32(p + 16, true);
    let csize = cd.getUint32(p + 20, true);
    let usize = cd.getUint32(p + 24, true);
    const nlen = cd.getUint16(p + 28, true);
    const elen = cd.getUint16(p + 30, true);
    const clen = cd.getUint16(p + 32, true);
    let offset = cd.getUint32(p + 42, true);
    const end = p + 46 + nlen + elen + clen;
    if (end > cd.byteLength) throw new ZipError('The file list of this ZIP is damaged.');
    const name = decodeName(new Uint8Array(cd.buffer, cd.byteOffset + p + 46, nlen), (flags & 0x800) !== 0);
    // The ZIP64 extra field holds, in this order, only the values that were 0xFFFFFFFF.
    let q = p + 46 + nlen;
    const extraEnd = q + elen;
    while (q + 4 <= extraEnd) {
      const id = cd.getUint16(q, true);
      const len = cd.getUint16(q + 2, true);
      if (q + 4 + len > extraEnd) break;
      if (id === 0x0001) {
        let r = q + 4;
        const stop = q + 4 + len;
        if (usize === 0xffffffff && r + 8 <= stop) { usize = u64(cd, r); r += 8; }
        if (csize === 0xffffffff && r + 8 <= stop) { csize = u64(cd, r); r += 8; }
        if (offset === 0xffffffff && r + 8 <= stop) { offset = u64(cd, r); r += 8; }
      }
      q += 4 + len;
    }
    entries.push({
      name,
      method,
      flags,
      crc,
      csize,
      usize,
      offset: offset + shift,
      isDir: name.endsWith('/') || name.endsWith('\\'),
      encrypted: (flags & 1) !== 0,
    });
    p = end;
  }
  return new ZipFile(blob, entries);
}

export class ZipFile {
  constructor(blob, entries) {
    this.blob = blob;
    this.entries = entries;
  }

  /** Where the bytes of an entry start in the archive. */
  async dataStart(e) {
    const h = await readBytes(this.blob, e.offset, e.offset + 30);
    if (h.getUint32(0, true) !== SIG_LOC) throw new ZipError(`"${e.name}" is damaged in this ZIP.`);
    const start = e.offset + 30 + h.getUint16(26, true) + h.getUint16(28, true);
    if (start + e.csize > this.blob.size) throw new ZipError(`"${e.name}" is cut short in this ZIP.`);
    return start;
  }

  /**
   * The bytes of an entry, as a Blob typed `type`. Unpacking stops with an
   * error if the entry grows past what it declared (a "zip bomb") or past
   * `maxBytes`.
   */
  async read(e, type = '', maxBytes = LIMITS.maxEntryBytes) {
    if (e.encrypted) throw new ZipError(`"${e.name}" is password protected.`);
    if (e.usize > maxBytes) throw new ZipError(`"${e.name}" is too big to open here.`);
    const start = await this.dataStart(e);
    const raw = this.blob.slice(start, start + e.csize, type);
    if (e.method === 0) return raw;
    if (e.method !== 8) throw new ZipError(`"${e.name}" is packed in a way the website cannot open.`);
    // Allow a little over what was declared, never more than the cap.
    const limit = Math.min(maxBytes, Math.max(e.usize, 0) * 1.05 + 4096);
    let seen = 0;
    const guard = new TransformStream({
      transform(chunk, controller) {
        seen += chunk.byteLength;
        if (seen > limit) controller.error(new ZipError('An entry unpacked to more than it declared.'));
        else controller.enqueue(chunk);
      },
    });
    try {
      const out = raw.stream().pipeThrough(new DecompressionStream('deflate-raw')).pipeThrough(guard);
      const blob = await new Response(out).blob();
      return type ? blob.slice(0, blob.size, type) : blob;
    } catch (err) {
      if (err instanceof ZipError) throw err;
      throw new ZipError(`"${e.name}" is damaged in this ZIP.`);
    }
  }

  /** At most the first `n` bytes of an entry (for reading a picture's size). */
  async head(e, n = 65536) {
    if (e.encrypted) throw new ZipError(`"${e.name}" is password protected.`);
    const start = await this.dataStart(e);
    if (e.method === 0) return new Uint8Array(await this.blob.slice(start, start + Math.min(n, e.csize)).arrayBuffer());
    if (e.method !== 8) throw new ZipError(`"${e.name}" is packed in a way the website cannot open.`);
    const reader = this.blob
      .slice(start, start + e.csize)
      .stream()
      .pipeThrough(new DecompressionStream('deflate-raw'))
      .getReader();
    const parts = [];
    let have = 0;
    try {
      while (have < n) {
        const { done, value } = await reader.read();
        if (done) break;
        parts.push(value);
        have += value.byteLength;
      }
    } catch (_) {
      // A damaged stream: use what came out of it.
    } finally {
      reader.cancel().catch(() => {});
    }
    const out = new Uint8Array(Math.min(have, n));
    let at = 0;
    for (const part of parts) {
      const take = Math.min(part.byteLength, out.length - at);
      out.set(part.subarray(0, take), at);
      at += take;
    }
    return out;
  }
}

/** Is this entry name safe to treat as a path inside a book? */
export function safePath(name) {
  const p = name.replaceAll('\\', '/');
  if (p.startsWith('/') || /^[A-Za-z]:/.test(p)) return false;
  return !p.split('/').some((seg) => seg === '..');
}
