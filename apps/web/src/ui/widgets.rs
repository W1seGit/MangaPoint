//! Small pieces used by several screens: the shared ones (`mangapoint_ui::widgets`) and the
//! website's own.

use dioxus::prelude::*;
pub use mangapoint_ui::widgets::*;

/// The cover of a book: the browser fills the picture from the thumbnail it
/// kept; a book without one shows the placeholder.
#[component]
pub fn BrowserCover(id: String, has_cover: bool) -> Element {
    if !has_cover {
        return rsx! { NoCover {} };
    }
    rsx! {
        span { class: "cover-box",
            img { class: "cover", "data-cover": "{id}", alt: "", decoding: "async" }
        }
    }
}
