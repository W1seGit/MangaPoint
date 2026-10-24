//! The Preview tab: the pages drawn for one chapter, and how they are shown.

use std::rc::Rc;

pub use mangapoint_ui::logic::preview::Zoom;

/// A rendered page as a data URL.
#[derive(Clone, PartialEq)]
pub struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// Preview display choices that are not conversion settings.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PreviewView {
    pub zoom: Zoom,
    /// Hide the settings panel to give the pages the whole width.
    pub focus: bool,
    /// Show the downloaded page beside the converted one.
    pub show_original: bool,
    /// Draw the cropped-away area on the original.
    pub show_crop: bool,
    pub editing_crop: bool,
    pub source_focus: bool,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            zoom: Zoom::Fit,
            focus: false,
            show_original: true,
            show_crop: true,
            editing_crop: false,
            source_focus: false,
        }
    }
}

/// One page of the chapter's plan, as the Preview tab shows it.
#[derive(Clone, Default, PartialEq)]
pub struct PreviewPage {
    /// The page as it will appear on the reader; `None` until it is drawn.
    pub shot: Option<Shot>,
    pub thumbnail: Option<Shot>,
    /// The picture the page comes from, whole and as downloaded: a double
    /// page is one wide picture whatever the spread setting. Joined strip rows
    /// for a strip chapter.
    pub original: Option<Shot>,
    /// Where on `original` this page lies; `None` for a turned page and for
    /// strip rows.
    pub region: Option<converter::plan::SourceRegion>,
    /// Only while cropping: the part of `original` that is cropped (a half, a
    /// turned page, a piece), as the crop is measured on it.
    pub piece: Option<Shot>,
    /// The part of the original the conversion keeps (left, top, width,
    /// height as fractions); paged chapters only.
    pub kept: Option<[f32; 4]>,
    /// The page's image, for a paged chapter. A split double page is two pages
    /// from one image.
    pub image: Option<usize>,
    pub crop_key: String,
    pub source: Option<converter::PlannedPage>,
    pub generation: u64,
    /// The settings (see `PreviewState::signature`) this page was drawn with.
    /// A page drawn with the current ones needs no drawing again.
    pub rendered_for: u64,
}

/// What the Preview tab is showing: every page of one chapter.
#[derive(Clone, Default)]
pub struct PreviewState {
    /// Downloaded chapter being previewed.
    pub chapter_key: Option<String>,
    /// Every page the chapter becomes, in order. After a settings change the
    /// old drawing of a page stays until the new one replaces it.
    pub pages: Vec<Rc<PreviewPage>>,
    /// The chapter is one stitched strip (a webtoon), not one page per image.
    pub strip: bool,
    /// How many downloaded images the chapter has.
    pub images: usize,
    /// Pages drawn with the current settings so far.
    pub fresh: usize,
    pub view: PreviewView,
    pub busy: bool,
    pub error: Option<String>,
    /// Bumped on every change so an older render never overwrites a newer one.
    pub generation: u64,
    /// What the pages should look like, as one number: the chapter, the
    /// conversion settings, the reader, and the crops. Scrolling does not
    /// change it; anything that changes a page does.
    pub signature: u64,
    pub selected: usize,
    pub thumb_start: usize,
    /// The first and last page on or near the screen in the scrolling list;
    /// those are drawn full size.
    pub window: (usize, usize),
    pub overview: bool,
    /// An unsaved crop projected only on the selected source section.
    pub draft_crop: Option<(String, library::CropRect)>,
    pub shading_samples: Vec<String>,
}
