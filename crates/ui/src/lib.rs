//! What the desktop app (`apps/desktop`) and the website (`apps/web`) both show or decide, kept once
//! so a change lands in both. An app folder keeps only what the other app cannot have; see
//! `docs/architecture.md`.
//!
//! - `art`: the mascot, the scenes and the icons.
//! - `host`: how a shared screen reaches the app it is in (`Host`, `UiState`).
//! - `widgets`: the small pieces used by several screens.
//! - `chapter_table`: the chapter table with its filter, range and sorting.
//! - `conversion`: the controls for how pages are converted.
//! - `export`: the Export screen's folding panels and the bars between them.
//! - `dialogs`: the question about unsaved reader settings, and the toasts.
//! - `reader`: the reader's bars, settings, zoom menu and end-of-chapter card.
//! - `readers`: the list of readers, adding one, and the form for the chosen one.
//! - `scripts`: the page scripts both apps run (`crates/ui/js`).
//! - `library`: the Library's cards and the bar for the ticked ones.
//! - `logic`: the decisions behind the screens, plain data in and out, tested without a window.
//!
//! Next to the code: `css/` (the shared style sheets, loaded in the order of `css/ORDER`; `CSS`
//! holds them all) and `js/` (the shared page scripts, see `scripts`).

pub mod art;
pub mod host;
pub mod chapter_table;
pub mod conversion;
pub mod dialogs;
pub mod export;
pub mod library;
pub mod page_editor;
pub mod logic;
pub mod reader;
pub mod readers;
pub mod scripts;
pub mod shell;
pub mod widgets;

// The shared style sheets as one text, in their order (see `build.rs` and `css/ORDER`).
include!(concat!(env!("OUT_DIR"), "/css.rs"));

#[cfg(test)]
mod css_tests {
    /// Every style sheet in `css/` is loaded, and only once.
    #[test]
    fn every_style_sheet_is_in_the_order() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("css");
        let order = std::fs::read_to_string(dir.join("ORDER")).unwrap();
        let listed: Vec<&str> = order.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".css"))
            .collect();
        on_disk.sort();
        let mut sorted: Vec<String> = listed.iter().map(|s| s.to_string()).collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), listed.len(), "a style sheet is listed twice");
        assert_eq!(sorted, on_disk);
        assert!(super::CSS.contains("--panel"), "the colours come first");
        // The art's generated class names are not in any code, so a split by usage can lose them.
        for need in [".m-head", "@keyframes m-nod", ".m-steam.s2", ".m-window.w2 .m-lit", ".mdots span"] {
            assert!(super::CSS.contains(need), "the shared style sheets must hold {need}");
        }
    }
}
