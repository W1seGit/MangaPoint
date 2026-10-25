// A picture's width and height from the first bytes of its file, without decoding it,
// so a page can be given its true shape long before it is opened.

/** `bytes`: the start of a PNG, JPEG, GIF or WebP file. Returns [w, h] or null. */
export function imageSize(bytes) {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const n = bytes.length;
  if (n > 24 && v.getUint32(0) === 0x89504e47) return [v.getUint32(16), v.getUint32(20)];
  if (n > 10 && v.getUint32(0) === 0x47494638) return [v.getUint16(6, true), v.getUint16(8, true)];
  if (n > 30 && v.getUint32(0) === 0x52494646 && v.getUint32(8) === 0x57454250) {
    const kind = v.getUint32(12);
    if (kind === 0x56503858) return [1 + (v.getUint32(24, true) & 0xffffff), 1 + (v.getUint32(26, true) >>> 8)]; // VP8X
    if (kind === 0x56503820) return [v.getUint16(26, true) & 0x3fff, v.getUint16(28, true) & 0x3fff]; // VP8
    if (kind === 0x5650384c) { // VP8L
      const b = v.getUint32(21, true);
      return [1 + (b & 0x3fff), 1 + ((b >>> 14) & 0x3fff)];
    }
    return null;
  }
  if (n > 4 && v.getUint16(0) === 0xffd8) {
    let i = 2;
    while (i + 9 < n) {
      if (bytes[i] !== 0xff) { i++; continue; }
      const m = bytes[i + 1];
      if (m === 0xff) { i++; continue; }
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) return [v.getUint16(i + 7), v.getUint16(i + 5)];
      i += 2 + v.getUint16(i + 2);
    }
  }
  return null;
}
