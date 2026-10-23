//! A file's identity, the same on every device and in every browser.
//!
//! The reading-state file (see [`crate::state`]) recognises a book by its
//! fingerprint, so a position saved on a phone finds the same book on a laptop.
//! Nothing here reads a file: the page feeds slices to a [`Fingerprinter`] (for a
//! complete hash) or hands the first and last 256 KiB to
//! [`book_fingerprint_head_tail`] (the quick identity it uses for big files, which
//! never reads the whole file).

use sha2::{Digest, Sha256};

/// How many bytes of the start and of the end of a file the quick identity uses.
pub const HEAD_TAIL_BYTES: usize = 256 * 1024;

/// An incremental SHA-256 of whatever is fed to it.
#[derive(Clone, Default)]
pub struct Fingerprinter {
    hash: Sha256,
}

impl Fingerprinter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the next bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.hash.update(bytes);
    }

    /// The hash of everything fed so far, as 64 lower-case hex digits.
    pub fn finish(self) -> String {
        hex(&self.hash.finalize())
    }
}

/// Lower-case hex of some bytes.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// The quick identity of a file: the hex SHA-256 of the bytes
/// `"mp1|" + size (decimal, no padding) + "|" + head + tail`.
///
/// * `size` is the file's length in bytes.
/// * `head` is the first `min(size, 256 KiB)` bytes of the file.
/// * `tail` is the last `min(size, 256 KiB)` bytes of the file.
///
/// A file of up to 256 KiB therefore has its whole content in both slices. The
/// function takes only the first 256 KiB of `head` and the last 256 KiB of
/// `tail` if a caller passes more, so the result never depends on how much was
/// read. The same file gives the same fingerprint on any device.
pub fn book_fingerprint_head_tail(size: u64, head: &[u8], tail: &[u8]) -> String {
    let head = &head[..head.len().min(HEAD_TAIL_BYTES)];
    let tail = &tail[tail.len().saturating_sub(HEAD_TAIL_BYTES)..];
    let mut hash = Sha256::new();
    hash.update(b"mp1|");
    hash.update(size.to_string().as_bytes());
    hash.update(b"|");
    hash.update(head);
    hash.update(tail);
    hex(&hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_incremental_hash_is_sha256_whatever_the_slicing() {
        // SHA-256("abc"), from the standard's test vectors.
        let want = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let mut one = Fingerprinter::new();
        one.update(b"abc");
        assert_eq!(one.finish(), want);
        let mut parts = Fingerprinter::new();
        for b in b"abc" {
            parts.update(&[*b]);
        }
        assert_eq!(parts.finish(), want);
        assert_eq!(
            Fingerprinter::new().finish(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_quick_identity_is_defined_exactly() {
        let mut check = Fingerprinter::new();
        check.update(b"mp1|5|abcde");
        let expected = check.finish();
        assert_eq!(book_fingerprint_head_tail(5, b"abc", b"de"), expected);
        // Differences in size, head or tail all change it.
        assert_ne!(book_fingerprint_head_tail(6, b"abc", b"de"), expected);
        assert_ne!(book_fingerprint_head_tail(5, b"abd", b"de"), expected);
        assert_ne!(book_fingerprint_head_tail(5, b"abc", b"df"), expected);
    }

    #[test]
    fn more_than_256_kib_is_not_looked_at() {
        let big: Vec<u8> = (0..HEAD_TAIL_BYTES * 2).map(|i| (i % 251) as u8).collect();
        let head = &big[..HEAD_TAIL_BYTES];
        let tail = &big[big.len() - HEAD_TAIL_BYTES..];
        let exact = book_fingerprint_head_tail(big.len() as u64, head, tail);
        assert_eq!(book_fingerprint_head_tail(big.len() as u64, &big, &big), exact);
    }
}
