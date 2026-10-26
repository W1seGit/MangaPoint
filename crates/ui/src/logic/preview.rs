//! The page editor's layout: how big a page is at a zoom, which pages come from one source page,
//! and what a strip page repeats of the one before. Both apps describe each converted page as a
//! `PageSlot`: the desktop app from the converter's plan, the website from the engine's preview.

/// How big the pages are on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zoom {
    /// As large as fits without scrolling.
    #[default]
    Fit,
    /// As wide as the space allows; scroll down for the rest.
    Width,
    /// A percentage of the reader's own size: 100 is one screen pixel for
    /// each pixel of the reader.
    Custom(u16),
}

impl Zoom {
    pub const MIN_PERCENT: u16 = 25;
    pub const MAX_PERCENT: u16 = 400;
}

/// Which part of its source page a converted page shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotPart {
    /// The whole page, or something not worth naming.
    #[default]
    Whole,
    /// The left half of a double page.
    Left,
    /// The right half of a double page.
    Right,
    /// One piece of a page cut into several.
    Piece,
    /// Rows of a long strip, from `top`, `height` rows tall.
    Strip,
}

/// One converted page, as the layout needs it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageSlot {
    /// The source page it is made from, if it is made from one picture.
    pub image: Option<usize>,
    pub part: SlotPart,
    /// For a strip page: the first row of the strip it shows, and how many rows.
    pub top: u32,
    pub height: u32,
}

/// What the page after this one repeats of it: how many rows, and where those
/// rows sit on the page (as shares of the screen height).
#[derive(Clone, Debug, PartialEq)]
pub struct Seam {
    pub rows: u32,
    pub band_top: f32,
    pub band_height: f32,
}

/// The seam after each page of a strip chapter; `None` for the last page and
/// for chapters that are not strips.
pub fn seams(pages: &[PageSlot], screen_height: u32) -> Vec<Option<Seam>> {
    let rows_of = |p: &PageSlot| (p.part == SlotPart::Strip).then_some((p.top, p.height));
    (0..pages.len())
        .map(|i| {
            let (top, height) = rows_of(&pages[i])?;
            let (next_top, _) = rows_of(pages.get(i + 1)?)?;
            let rows = (top + height).saturating_sub(next_top);
            let screen = screen_height.max(1) as f32;
            Some(Seam {
                rows,
                band_top: height.saturating_sub(rows) as f32 / screen * 100.0,
                band_height: rows as f32 / screen * 100.0,
            })
        })
        .collect()
}

/// How tall a page is at this zoom, as a CSS length.
pub fn row_height(zoom: Zoom, width: u32, height: u32) -> String {
    match zoom {
        Zoom::Fit => format!("min(var(--reader-page-height),calc(var(--reader-page-width) * {height} / {width}))"),
        Zoom::Width => format!("calc(var(--reader-page-width) * {height} / {width})"),
        Zoom::Custom(percent) => format!("{}px", height * percent as u32 / 100),
    }
}

/// The size of the reader's screen at this zoom, as CSS, keeping its shape.
pub fn reader_style(zoom: Zoom, width: u32, height: u32) -> String {
    let size = match zoom {
        Zoom::Fit => format!("height:{};width:auto;", row_height(zoom, width, height)),
        Zoom::Width => "height:auto;width:100%;".to_string(),
        Zoom::Custom(percent) => {
            format!("height:{}px;width:{}px;", height * percent as u32 / 100, width * percent as u32 / 100)
        }
    };
    format!("{size}aspect-ratio:{width}/{height};")
}

/// Which pages share a picture: a double page split in two, or a page zoomed
/// into pieces, is several reader pages from one source page. Every other page
/// is a group of its own. As `(first, one past the last)`.
pub fn groups(pages: &[PageSlot]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        match out.last_mut() {
            Some(last) if page.image.is_some() && pages[last.0].image == page.image => {
                last.1 = i + 1;
            }
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// What a page is of its source page, when the source page makes several
/// ("left half of source page 12"): which one of them this is.
pub fn part_note(page: &PageSlot, k: usize, of: usize) -> Option<String> {
    let image = page.image?;
    if of < 2 {
        return None;
    }
    let what = match page.part {
        SlotPart::Left => "left half".to_string(),
        SlotPart::Right => "right half".to_string(),
        _ => format!("part {} of {of}", k + 1),
    };
    Some(format!("{what} of source page {}", image + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(image: Option<usize>, part: SlotPart) -> PageSlot {
        PageSlot { image, part, ..PageSlot::default() }
    }

    #[test]
    fn the_pages_of_one_source_page_share_a_group() {
        let pages = vec![
            page(Some(0), SlotPart::Whole),
            page(Some(1), SlotPart::Right),
            page(Some(1), SlotPart::Left),
            page(Some(2), SlotPart::Whole),
            page(None, SlotPart::Whole),
            page(None, SlotPart::Whole),
        ];
        assert_eq!(groups(&pages), vec![(0, 1), (1, 3), (3, 4), (4, 5), (5, 6)]);
        assert_eq!(part_note(&pages[0], 0, 1), None);
        assert_eq!(part_note(&pages[1], 0, 2).as_deref(), Some("right half of source page 2"));
        assert_eq!(part_note(&pages[2], 1, 2).as_deref(), Some("left half of source page 2"));
        let tile = page(Some(4), SlotPart::Piece);
        assert_eq!(part_note(&tile, 1, 4).as_deref(), Some("part 2 of 4 of source page 5"));
    }

    #[test]
    fn a_page_is_sized_by_its_zoom() {
        assert_eq!(row_height(Zoom::Custom(50), 480, 800), "400px", "half of the screen's own height");
        assert!(row_height(Zoom::Fit, 480, 800).starts_with("min("));
        assert_eq!(row_height(Zoom::Width, 480, 800), "calc(var(--reader-page-width) * 800 / 480)");
    }

    #[test]
    fn the_reader_style_keeps_the_page_shape() {
        let fit = reader_style(Zoom::Fit, 480, 800);
        assert!(fit.contains("width:auto;") && fit.ends_with("aspect-ratio:480/800;"));
        assert_eq!(reader_style(Zoom::Custom(100), 480, 800), "height:800px;width:480px;aspect-ratio:480/800;");
        assert!(reader_style(Zoom::Width, 480, 800).starts_with("height:auto;width:100%;"));
    }

    #[test]
    fn a_strip_page_says_what_the_next_one_repeats() {
        let strip = |top, height| PageSlot { image: None, part: SlotPart::Strip, top, height };
        let s = seams(&[strip(0, 800), strip(700, 800), strip(1500, 800)], 800);
        assert_eq!(s[0].as_ref().map(|s| s.rows), Some(100));
        assert_eq!(s[1].as_ref().map(|s| s.rows), Some(0), "a clean cut");
        assert_eq!(s[2], None, "the last page");
        assert!(seams(&[page(Some(0), SlotPart::Whole), page(Some(1), SlotPart::Whole)], 800).iter().all(Option::is_none));
    }
}
