//! Small pieces used by several screens.

use crate::state::AppState;
use dioxus::prelude::*;

/// A Lucide icon by name (see `icons.rs`). Takes the surrounding text color.
#[component]
pub fn Icon(name: &'static str, size: Option<u32>) -> Element {
    let size = size.unwrap_or(18);
    let body = super::icons::body(name).unwrap_or("");
    rsx! {
        svg {
            class: "icon",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            dangerous_inner_html: "{body}",
        }
    }
}

/// A screen with nothing on it yet: the cat napping, and a friendly line.
#[component]
pub fn EmptyState(
    title: String,
    message: String,
    compact: Option<bool>,
    /// The cat beside the words, in a card, rather than above them.
    side: Option<bool>,
    /// Show the cat asleep on an empty bookshelf instead of on its own.
    shelf: Option<bool>,
    /// Or another scene: "search", "ocean" or "waiting".
    scene: Option<&'static str>,
    children: Element,
) -> Element {
    let class = if side.unwrap_or(false) {
        "empty side"
    } else if compact.unwrap_or(false) {
        "empty compact"
    } else {
        "empty"
    };
    rsx! {
        div { class: "{class}",
            if scene == Some("ocean") {
                super::ocean::OceanCat {}
            } else if scene == Some("search") {
                super::scenes::SearchCat {}
            } else if scene == Some("waiting") {
                super::hang::HangingCat {}
            } else if shelf.unwrap_or(false) {
                super::shelf::ShelfCat {}
            } else {
                super::cat::RestingCat {}
            }
            div { class: "empty-words",
                h2 { "{title}" }
                p { "{message}" }
                {children}
            }
        }
    }
}

#[component]
pub fn ProgressBar(fraction: Option<f32>) -> Element {
    match fraction {
        Some(f) => {
            let pct = (f.clamp(0.0, 1.0) * 100.0).round();
            rsx! { div { class: "progress", div { style: "width: {pct}%" } } }
        }
        None => rsx! { div { class: "progress indeterminate", div {} } },
    }
}

/// A switch that slides between off and on.
#[component]
pub fn Toggle(on: bool, onchange: EventHandler<bool>) -> Element {
    let checked = if on { "true" } else { "false" };
    rsx! {
        button {
            r#type: "button", role: "switch", "aria-checked": "{checked}",
            class: if on { "toggle on" } else { "toggle" },
            onclick: move |_| onchange.call(!on),
            span { class: "knob" }
        }
    }
}

/// A small question mark that explains a setting when the pointer is on it.
#[component]
pub fn Help(text: String) -> Element {
    rsx! {
        span { class: "help", tabindex: "0", "data-tip": "{text}", "aria-label": "{text}",
            Icon { name: "circle-help", size: 15 }
        }
    }
}

#[component]
pub fn Chip(text: String, kind: Option<String>) -> Element {
    let class = format!("chip {}", kind.unwrap_or_default());
    rsx! { span { class: "{class}", "{text}" } }
}

/// Something is loading: the cat runs in circles in the middle of the place
/// where it will show up.
#[component]
pub fn Loading(label: String) -> Element {
    rsx! {
        div { class: "loading",
            super::cat::CirclingCat {}
            span { "{label}" }
        }
    }
}

/// A small working marker with a few words, to sit in a line of text.
#[component]
pub fn Busy(label: String, size: Option<u32>) -> Element {
    rsx! {
        span { class: "busy",
            super::cat::CirclingCat { size: size.unwrap_or(44) }
            "{label}"
        }
    }
}

/// What a page that needs the catalog engine shows while it is still
/// starting: the cat, how far it is, and that the rest of the app works.
#[component]
pub fn EngineStarting() -> Element {
    let state = use_context::<AppState>();
    let (message, percent) = match &*state.boot.read() {
        crate::state::BootState::Starting { message, percent } => (message.clone(), *percent),
        _ => ("Starting the catalog engine…".to_string(), None),
    };
    rsx! {
        div { class: "engine-starting",
            super::cat::Cat {}
            strong { "Getting the search ready" }
            p { class: "muted", "{message}" }
            div { class: "es-bar", super::ProgressBar { fraction: percent.map(|p| f32::from(p) / 100.0) } }
            p { class: "muted small", "Your Library and downloads work in the meantime." }
        }
    }
}

/// A cover image, or a placeholder until it has loaded.
#[component]
pub fn Cover(key_id: String) -> Element {
    let state = use_context::<AppState>();
    // A shared string: drawing a cover does not copy its picture.
    let url = state.covers.read().get(&key_id).cloned();
    // The address whose picture has arrived, and the one that could not be had:
    // until it arrives the cat runs in circles in its place, and a picture that
    // cannot be had shows the placeholder.
    let mut arrived = use_signal(String::new);
    let mut broken = use_signal(String::new);
    match url {
        Some(src) if *broken.read() != *src => {
            let waiting = *arrived.read() != *src;
            let (on_load, on_error) = (src.to_string(), src.to_string());
            rsx! {
                span { class: "cover-box",
                    img {
                        class: "cover", src: "{src}", loading: "lazy", decoding: "async",
                        onload: move |_| arrived.set(on_load.clone()),
                        onerror: move |_| broken.set(on_error.clone()),
                    }
                    if waiting {
                        span { class: "cover-wait", super::cat::CirclingCat { size: 56 } }
                    }
                }
            }
        }
        _ => rsx! { NoCover {} },
    }
}

/// The picture for a cover that is missing or has not loaded yet.
#[component]
pub fn NoCover() -> Element {
    rsx! { div { class: "cover empty art", dangerous_inner_html: super::placeholder::svg() } }
}

/// Point a cover at the sources' picture. Nothing is fetched or kept here:
/// the page asks for the picture by address when the cover comes into view
/// (see `covers`). With a series on several sources, all are asked and the
/// first picture to arrive is used.
pub fn load_cover_race(state: AppState, key: String, identity: &str, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let url: std::rc::Rc<str> = crate::covers::source_url(identity, &paths).into();
    if state.covers.peek().get(&key) == Some(&url) {
        return;
    }
    let mut covers = state.covers;
    covers.write().insert(key, url);
}

pub fn cover_key(source_id: &str, manga_id: i64) -> String {
    format!("{source_id}:{manga_id}")
}

/// A path written the way the system expects: on Windows every separator is a
/// backslash. A path mixing `/` and `\` is not understood by Explorer or the
/// folder dialog, which then open Documents instead.
pub fn native_path(path: &std::path::Path) -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        std::path::PathBuf::from(path.display().to_string().replace('/', "\\"))
    } else {
        path.to_path_buf()
    }
}

/// Open a file's folder in the system file manager, with the file selected.
pub fn show_in_folder(path: &str) {
    let path = native_path(std::path::Path::new(path));
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // Explorer wants `/select,"path"` as one piece, quotes included.
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(&path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Open a folder in the system's file browser.
pub fn open_folder(dir: &std::path::Path) {
    let dir = native_path(dir);
    let dir = dir.as_path();
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Open a web address in the default browser.
pub fn open_link(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// The folder itself if it exists, else the closest parent that does.
fn nearest_existing(path: &std::path::Path) -> Option<&std::path::Path> {
    path.ancestors().find(|a| a.is_dir())
}

/// Ask the user for a folder with the system's own dialog. `start` is where
/// it opens, or else its nearest parent that exists (a folder not made yet
/// cannot be shown). `None` if they cancel.
pub async fn pick_folder(
    title: &str,
    start: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(dir) = start.as_deref().and_then(nearest_existing) {
        dialog = dialog.set_directory(native_path(dir));
    }
    dialog.pick_folder().await.map(|h| h.path().to_path_buf())
}

/// What the zoom box can hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZoomPick {
    /// The whole page.
    Fit,
    /// As wide as the space allows.
    Width,
    Percent(u16),
}

impl ZoomPick {
    /// What the box shows.
    pub fn label(self) -> String {
        match self {
            ZoomPick::Fit => "Fit page".to_string(),
            ZoomPick::Width => "Fit width".to_string(),
            ZoomPick::Percent(percent) => format!("{percent}%"),
        }
    }

    /// What was typed in the box: "125", "125%", "fit", "fit width", held to
    /// `min..=max` percent. `None` when it is not a zoom.
    pub fn parse(text: &str, min: u16, max: u16) -> Option<ZoomPick> {
        let text = text.trim().to_lowercase();
        if text.contains("width") {
            return Some(ZoomPick::Width);
        }
        if text.starts_with("fit") {
            return Some(ZoomPick::Fit);
        }
        let number: u32 = text.trim_end_matches('%').trim().parse().ok()?;
        Some(ZoomPick::Percent(number.clamp(u32::from(min), u32::from(max)) as u16))
    }
}

/// A zoom as a small menu, as in a document editor: a box showing the zoom
/// (type a percentage in it) and a list of the ready-made ones. Used for the
/// zoom of the page preview and for the zoom pages are converted at.
#[component]
pub fn ZoomDropdown(
    selected: ZoomPick,
    percents: Vec<u16>,
    min: u16,
    max: u16,
    onpick: EventHandler<ZoomPick>,
) -> Element {
    let mut open = use_signal(|| false);
    let label = selected.label();
    let mut choose = move |pick: ZoomPick| {
        onpick.call(pick);
        open.set(false);
    };
    let class = if open() { "zoom-menu is-open" } else { "zoom-menu" };
    rsx! {
        div { class: "{class}",
            div { class: "zoom-field",
                span { class: "zoom-icon", Icon { name: "zoom-in", size: 15 } }
                input {
                    r#type: "text", value: "{label}", "aria-label": "Zoom", spellcheck: "false",
                    onfocus: move |_| open.set(true),
                    onchange: move |e| {
                        if let Some(parsed) = ZoomPick::parse(&e.value(), min, max) {
                            choose(parsed);
                        }
                    },
                }
                button { class: "zoom-chevron", title: "Zoom choices", onclick: move |_| open.set(!open()),
                    Icon { name: "chevron-down", size: 15 }
                }
            }
            if open() {
                div { class: "menu-scrim", onclick: move |_| open.set(false) }
                div { class: "menu down zoom-list",
                    button { class: if selected == ZoomPick::Fit { "menu-item on" } else { "menu-item" }, onclick: move |_| choose(ZoomPick::Fit), "Fit page" }
                    button { class: if selected == ZoomPick::Width { "menu-item on" } else { "menu-item" }, onclick: move |_| choose(ZoomPick::Width), "Fit width" }
                    div { class: "menu-sep" }
                    for percent in percents {
                        button {
                            key: "{percent}",
                            class: if selected == ZoomPick::Percent(percent) { "menu-item on" } else { "menu-item" },
                            onclick: move |_| choose(ZoomPick::Percent(percent)),
                            "{percent}%"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_picker_opens_at_the_closest_folder_that_exists() {
        let here = std::env::temp_dir();
        assert_eq!(nearest_existing(&here), Some(here.as_path()));
        let missing = here.join("mangapoint-not-made").join("deeper");
        assert_eq!(nearest_existing(&missing), Some(here.as_path()));
    }

    #[test]
    fn a_native_path_keeps_a_unix_path_and_unmixes_a_windows_one() {
        let mixed = native_path(Path::new(r"C:\Data/Series/c1/1.png"));
        if cfg!(target_os = "windows") {
            assert_eq!(mixed, Path::new(r"C:\Data\Series\c1\1.png"));
        } else {
            assert_eq!(mixed, Path::new(r"C:\Data/Series/c1/1.png"));
        }
    }

    #[test]
    fn typed_zooms_are_understood() {
        use super::ZoomPick;
        assert_eq!(ZoomPick::parse("125", 25, 400), Some(ZoomPick::Percent(125)));
        assert_eq!(ZoomPick::parse(" 90 % ", 25, 400), Some(ZoomPick::Percent(90)));
        assert_eq!(ZoomPick::parse("Fit page", 25, 400), Some(ZoomPick::Fit));
        assert_eq!(ZoomPick::parse("fit width", 25, 400), Some(ZoomPick::Width));
        assert_eq!(ZoomPick::parse("1", 25, 400), Some(ZoomPick::Percent(25)));
        assert_eq!(ZoomPick::parse("99999", 25, 400), Some(ZoomPick::Percent(400)));
        assert_eq!(ZoomPick::parse("big", 25, 400), None);
        assert_eq!(ZoomPick::parse("150", 100, 300), Some(ZoomPick::Percent(150)));
        assert_eq!(ZoomPick::parse("20", 100, 300), Some(ZoomPick::Percent(100)));
        assert_eq!(ZoomPick::Width.label(), "Fit width");
        assert_eq!(ZoomPick::Percent(150).label(), "150%");
    }
}
