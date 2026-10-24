//! Finishing a streaming writer must surface delayed storage errors.
#![cfg(feature = "write")]
use std::io::{self, Cursor, Seek, SeekFrom, Write};
use formats::{cbz, epub, pdf};

#[derive(Default)]
struct FlushFails(Cursor<Vec<u8>>);
impl Write for FlushFails {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { self.0.write(bytes) }
    fn flush(&mut self) -> io::Result<()> { Err(io::Error::other("disk flush failed")) }
}
impl Seek for FlushFails {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> { self.0.seek(from) }
}

fn page() -> Vec<u8> {
    epub::encode_page(&converter::LevelImage {
        width: 2, height: 2, depth: converter::Depth::Two, levels: vec![0, 1, 2, 3],
    }).unwrap()
}

#[test]
fn cbz_reports_a_final_flush_failure() {
    let mut book = cbz::CbzBuilder::new(FlushFails::default(), cbz::CbzMeta::default());
    book.add_page(&page()).unwrap();
    assert!(book.finish().err().unwrap().to_string().contains("disk flush failed"));
}
#[test]
fn epub_reports_a_final_flush_failure() {
    let mut book = epub::EpubBuilder::new(FlushFails::default(), epub::EpubMeta { title: "Flush test".into(), language: "en".into(), rtl: false, width: 2, height: 2, created: 0 }).unwrap();
    book.add_page(&page()).unwrap();
    assert!(book.finish().err().unwrap().to_string().contains("disk flush failed"));
}
#[test]
fn pdf_reports_a_final_flush_failure() {
    let mut book = pdf::PdfBuilder::new(FlushFails::default(), pdf::PdfMeta::default()).unwrap();
    book.add_page(pdf::PdfImage::Raw { pixels: &[0, 128, 200, 255], width: 2, height: 2, color: pdf::PdfColor::Gray }).unwrap();
    assert!(book.finish().err().unwrap().to_string().contains("disk flush failed"));
}
