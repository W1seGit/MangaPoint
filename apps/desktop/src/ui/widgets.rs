//! Small pieces used by several screens: the shared ones (`mangapoint_ui::widgets`) and the app's own.

use crate::state::AppState;
use dioxus::prelude::*;
pub use mangapoint_ui::widgets::*;

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

/// Ask the user for files with the system's own dialog. `extensions` are the
/// kinds shown (without dots). `None` if they cancel.
pub async fn pick_files(
    title: &str,
    start: Option<std::path::PathBuf>,
    kind: &str,
    extensions: &[&str],
) -> Option<Vec<std::path::PathBuf>> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(kind, extensions);
    if let Some(dir) = start.as_deref().and_then(nearest_existing) {
        dialog = dialog.set_directory(native_path(dir));
    }
    dialog
        .pick_files()
        .await
        .map(|files| files.into_iter().map(|h| h.path().to_path_buf()).collect())
}

/// Words as a row of pills inside a box: type one and press Enter or a comma to
/// add it, click a pill (or press Backspace in the empty box) to take it off.
#[component]
pub fn TagBox(
    value: Vec<String>,
    placeholder: String,
    label: String,
    on_change: EventHandler<Vec<String>>,
) -> Element {
    let mut text = use_signal(String::new);
    let mut focused = use_signal(|| false);
    let add = {
        let value = value.clone();
        move |word: &str| {
            let word = word.trim().trim_matches(',').trim().to_string();
            if word.is_empty() || value.iter().any(|v| v.eq_ignore_ascii_case(&word)) {
                return None;
            }
            let mut next = value.clone();
            next.push(word);
            Some(next)
        }
    };
    let (add_enter, add_blur) = (add.clone(), add.clone());
    let for_backspace = value.clone();
    rsx! {
        div { class: if focused() { "tags focused" } else { "tags" },
            for v in value.iter() {
                {
                    let name = v.clone();
                    let rest: Vec<String> = value.iter().filter(|x| *x != v).cloned().collect();
                    rsx! {
                        span { class: "tag", key: "{name}", role: "button", title: "Remove {name}",
                            onclick: move |_| on_change.call(rest.clone()),
                            "{name}"
                            span { class: "tag-x", Icon { name: "x", size: 11 } }
                        }
                    }
                }
            }
            input {
                r#type: "text", value: "{text}", spellcheck: "false", "aria-label": "{label}",
                placeholder: if value.is_empty() { placeholder.clone() } else { String::new() },
                onfocus: move |_| focused.set(true),
                onblur: move |_| {
                    focused.set(false);
                    let typed = text();
                    if let Some(next) = add_blur(&typed) {
                        on_change.call(next);
                    }
                    text.set(String::new());
                },
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| {
                    let typed = text();
                    if e.key() == Key::Enter || e.key() == Key::Character(",".to_string()) {
                        e.prevent_default();
                        if let Some(next) = add_enter(&typed) {
                            on_change.call(next);
                        }
                        text.set(String::new());
                    } else if e.key() == Key::Backspace && typed.is_empty() && !for_backspace.is_empty() {
                        let mut next = for_backspace.clone();
                        next.pop();
                        on_change.call(next);
                    }
                },
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

}
