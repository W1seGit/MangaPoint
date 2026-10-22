//! Settings and About: what is kept on this device, the reading-state file,
//! the privacy promise, installing the app (and the desktop app), and deleting everything.

use super::widgets::{Icon, ProgressBar, SettingRow, Toggle};
use crate::app::{current_iso, Ctx, Modal, ModalAction};
use crate::bridge::{self, StorageInfo};
use crate::format::{human_bytes, plural};
use dioxus::prelude::*;
use serde_json::json;
use web_engine::state;

const FILE_NAME: &str = "mangapoint-reading.mpstate";

/// A state file arrived (picked or dropped): check it and merge it in.
pub fn state_file_loaded(ctx: Ctx, name: String, text: String) {
    let parsed = match state::parse_str(&text) {
        Ok(s) => s,
        Err(e) => {
            ctx.error(format!("{name}: {e}"));
            return;
        }
    };
    let now = current_iso();
    let settings = ctx.settings.peek().clone();
    let mut lib = ctx.library;
    let (loaded, merged_settings) = lib.write().apply_loaded(&parsed, settings, &now);
    let mut s = ctx.settings;
    s.set(merged_settings);
    ctx.save_settings();
    // Keep every record, so a reload finds them.
    let ids: Vec<String> = ctx.library.peek().states.keys().cloned().collect();
    for id in ids {
        ctx.save_state_of(&id);
    }
    let mut message = format!("Loaded {name}.");
    if !loaded.restored.is_empty() {
        message.push_str(&format!(" Your place was restored in {}.", plural(loaded.restored.len(), "book")));
    }
    if loaded.waiting > 0 {
        message.push_str(&format!(
            " {} waiting for {}: import the file{} and {} carry on.",
            plural(loaded.waiting, "book"),
            if loaded.waiting == 1 { "its file" } else { "their files" },
            if loaded.waiting == 1 { "" } else { "s" },
            if loaded.waiting == 1 { "it will" } else { "they will" },
        ));
    }
    ctx.notify(message);
}

fn save_state(ctx: Ctx, choose: bool) {
    let settings = ctx.settings.peek().clone();
    let file = ctx.library.peek().to_state(settings, &current_iso());
    let text = state::to_json_pretty(&file);
    dioxus::core::spawn_forever(async move {
        #[derive(serde::Deserialize)]
        struct Saved {
            mode: String,
            name: String,
        }
        match bridge::call::<Saved>("save_state", json!({ "text": text, "name": FILE_NAME, "choose": choose })).await {
            Ok(r) => match r.mode.as_str() {
                "file" => {
                    let mut sf = ctx.state_file;
                    sf.set(Some(r.name.clone()));
                    ctx.notify(format!("Reading state saved to {}.", r.name));
                }
                "download" => ctx.notify(format!("{} was downloaded. Keep it somewhere safe; load it on another device to carry on there.", r.name)),
                _ => {}
            },
            Err(e) => ctx.error(format!("Could not save the reading state: {e}")),
        }
    });
}

#[component]
pub fn SettingsPage() -> Element {
    let ctx = use_context::<Ctx>();
    let mut info = use_signal(StorageInfo::default);
    let mut refresh = use_signal(|| 0u32);
    {
        let books = ctx.library.read().books.len();
        use_effect(move || {
            let _ = (refresh(), books);
            dioxus::core::spawn_forever(async move {
                if let Ok(i) = bridge::call::<StorageInfo>("storage", json!({})).await {
                    info.set(i);
                }
            });
        });
        use_future(move || async move {
            if let Ok(Some(name)) = bridge::call::<Option<String>>("state_file_name", json!({})).await {
                let mut sf = ctx.state_file;
                sf.set(Some(name));
            }
        });
    }
    let caps = ctx.caps.read().clone();
    let lib = ctx.library.read();
    let books = lib.books.len();
    let waiting = lib.waiting().len();
    let total: u64 = lib.books.iter().map(|b| b.size).sum();
    let i = info();
    let fraction = if i.quota > 0.0 { Some((i.used / i.quota) as f32) } else { None };
    let file = ctx.state_file.read().clone();
    let can_install = *ctx.can_install.read();
    let engine = web_engine::engine_version();

    let mut section = ctx.settings_section;
    let books_label = plural(books, "book");
    let safe_note = if !caps.persist {
        String::new()
    } else if i.persisted {
        "Your library is protected from automatic clean-up. This can be undone, but only in your browser's settings for this site, not here.".to_string()
    } else {
        "The browser may clear your library when the device runs low on space; Keep my library safe asks it not to.".to_string()
    };
    let storage_note = {
        let mut note = format!("{}, {} imported.", plural(books, "book"), human_bytes(total));
        if i.quota > 0.0 {
            note.push_str(&format!(" The browser reports {} used of about {} it allows this site.", human_bytes(i.used as u64), human_bytes(i.quota as u64)));
        }
        note
    };

    rsx! {
        div { class: "settings",
            nav { class: "settings-nav",
                for (sec, icon, label, slug) in SECTIONS {
                    button {
                        key: "{label}",
                        id: "settings-nav-{slug}",
                        class: if sec == section() { "nav-item active" } else { "nav-item" },
                        onclick: move |_| section.set(sec),
                        Icon { name: icon, size: 17 }
                        "{label}"
                    }
                }
            }
            div { class: "settings-content",
                {match section() {
                    Section::Library => rsx! {
                        h2 { class: "sx-group", "Storage" }
                        div { id: "storage-card",
                            SettingRow {
                                label: "Storage on this device",
                                help: "MangaPoint on the web runs entirely in your browser. The books you import, your reading place and the files you export are never uploaded: there is no account, no analytics and no server that stores anything of yours. Books are kept in this browser's private storage. Browsers may delete a site's data when the device runs low on space, unless you allow this; keeping your library safe asks the browser not to.",
                                note: format!("{storage_note} {safe_note}"),
                                if caps.persist {
                                    div { class: "row", style: "gap: 10px",
                                        span { class: "muted small", "Keep my library safe" }
                                        Toggle {
                                            on: i.persisted, label: "Keep my library safe".to_string(),
                                            onchange: move |on: bool| {
                                                if on {
                                                    dioxus::core::spawn_forever(async move {
                                                        let granted = bridge::call::<bool>("persist", json!({})).await.unwrap_or(false);
                                                        if granted { ctx.notify("Your library is now protected from automatic clean-up."); } else { ctx.warn("The browser did not agree. Installing the app, or using the site more, often makes it say yes."); }
                                                        refresh.set(refresh() + 1);
                                                    });
                                                } else {
                                                    ctx.warn("This can be undone, but not through MangaPoint: do it in your browser's settings for this site.");
                                                }
                                            },
                                        }
                                    }
                                } else {
                                    span { class: "muted", "{books_label}" }
                                }
                            }
                            if fraction.is_some() { div { class: "sx-bar", ProgressBar { fraction } } }
                            if !caps.opfs {
                                div { class: "banner warn small", Icon { name: "triangle-alert", size: 16 }
                                    div { "This browser cannot keep big files directly, so books are kept in its database instead (slower, and some browsers limit them). If a book is not kept, your place is still remembered and importing the file again resumes it." }
                                }
                            }
                        }
                        h2 { class: "sx-group", "Data" }
                        div { id: "state-card",
                            SettingRow {
                                label: "Reading state file",
                                help: "A small file with your place in every book, the chapters you read and how you like to read. Save it, and load it on another device or browser: books you already have there resume at once; the others show up as waiting for their file, and resume as soon as you import it.",
                                note: match &file {
                                    Some(name) => format!("Saves go straight to {name}. The browser may ask for permission each time you open the site."),
                                    None if !caps.fsa => "This browser downloads the file when you save. You can also drop a .mpstate file anywhere on the page to load it.".to_string(),
                                    None => String::new(),
                                },
                                div { class: "row wrap sx-buttons",
                                    button { class: "btn primary", id: "state-save", onclick: move |_| save_state(ctx, false),
                                        Icon { name: "save", size: 15 } "Save"
                                    }
                                    button { class: "btn", id: "state-load", onclick: move |_| bridge::send("pick_state", json!({})),
                                        Icon { name: "file-up", size: 15 } "Load"
                                    }
                                    if caps.fsa && file.is_some() {
                                        button { class: "btn ghost", id: "state-save-as", onclick: move |_| save_state(ctx, true), "Save to another file\u{2026}" }
                                    }
                                }
                            }
                            if file.is_some() {
                                div { class: "sx-note", id: "state-file",
                                    button { class: "btn ghost small", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call_ok("forget_state_file", json!({})).await; let mut sf = ctx.state_file; sf.set(None); }); }, "Stop saving to that file" }
                                }
                            }
                            if waiting > 0 {
                                p { class: "sx-note", id: "state-waiting", "{plural(waiting, \"book\")} waiting for their file (see the Library)." }
                            }
                        }
                        div { id: "danger-card",
                            SettingRow {
                                label: "Delete everything",
                                help: "Removes every book, your places and your settings from this browser. Files you exported or saved stay where they are.",
                                button { class: "btn danger", id: "clear-all",
                                    onclick: move |_| ctx.ask(Modal {
                                        title: "Delete everything on this device?".into(),
                                        body: format!("{} and your reading places will be deleted from this browser. This cannot be undone. A reading-state file you saved earlier stays untouched.", plural(books, "book")),
                                        confirm: "Delete everything".into(), cancel: "Keep".into(), danger: true,
                                        action: ModalAction::ClearAll,
                                    }),
                                    Icon { name: "trash", size: 15 } "Delete everything"
                                }
                            }
                        }
                    },
                    Section::Readers => rsx! {
                        super::readers::DevicesEditor {}
                    },
                    Section::App => rsx! {
                        h2 { class: "sx-group", "App" }
                        div { id: "app-card",
                            SettingRow {
                                label: "Use it like an app",
                                help: "Install MangaPoint to open it from your home screen or desktop, full screen, and to read your library with no connection at all. On iPhone and iPad: the Share button, then Add to Home Screen.",
                                note: if caps.standalone || can_install { String::new() } else { "In your browser's menu choose Install app, or Add to Home Screen.".to_string() },
                                div { class: "row wrap sx-buttons",
                                    if caps.standalone {
                                        span { class: "chip ok", "Installed" }
                                    } else if can_install {
                                        button { class: "btn primary", id: "install", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call::<String>("install", json!({})).await; }); }, Icon { name: "download", size: 15 } "Install the app" }
                                    } else {
                                        span { id: "install-hint" }
                                    }
                                }
                            }
                        }
                        h2 { class: "sx-group", "The desktop app" }
                        div { class: "desk-app", id: "desktop-app",
                            p { class: "desk-intro",
                                "This website reads and exports the books you already have. The MangaPoint desktop app is the one that finds them: it searches your sources, downloads the chapters and makes them fit your e-reader."
                            }
                            div { class: "desk-features",
                                for (icon, title, text) in DESKTOP_FEATURES {
                                    div { class: "desk-feature", key: "{title}",
                                        span { class: "desk-icon", Icon { name: icon, size: 19 } }
                                        div {
                                            strong { "{title}" }
                                            p { class: "muted small", "{text}" }
                                        }
                                    }
                                }
                            }
                            h3 { class: "desk-heading", "Download it" }
                            div { class: "desk-downloads",
                                for (name, detail, slug) in DESKTOP_DOWNLOADS {
                                    a { class: "desk-download", id: "download-{slug}", key: "{slug}", href: "{RELEASES}", target: "_blank", rel: "noopener",
                                        span { class: "desk-os", "{name}" }
                                        span { class: "desk-detail muted small", "{detail}" }
                                        span { class: "desk-get", Icon { name: "download", size: 15 } "Download" }
                                    }
                                }
                            }
                            p { class: "desk-source muted small",
                                "Free and open source, for Windows, macOS and Linux. "
                                a { href: "{REPO}", target: "_blank", rel: "noopener", id: "github-link", Icon { name: "external-link", size: 13 } "MangaPoint on GitHub" }
                            }
                        }
                    },
                    Section::About => rsx! {
                        h2 { class: "sx-group", "About" }
                        div { id: "about-card",
                            SettingRow {
                                label: "MangaPoint on the web",
                                help: "It opens CBZ, ZIP, EPUB with pictures, folders of pictures and loose pictures. RAR, 7z and PDF are not supported on the web yet. It does not search for or download manga; the desktop app does.",
                                note: format!("Version {} \u{b7} engine {engine}", env!("CARGO_PKG_VERSION")),
                                span {}
                            }
                        }
                    },
                }}
            }
        }
    }
}

const REPO: &str = "https://github.com/W1seGit/MangaPoint";
const RELEASES: &str = "https://github.com/W1seGit/MangaPoint/releases/latest";

/// What the desktop app does that the website does not: (icon, title, text).
const DESKTOP_FEATURES: [(&str, &str, &str); 6] = [
    ("search", "Search many sources at once", "Type a title and every source you installed is asked together. The same series from several sources becomes one card."),
    ("download", "A download queue that keeps going", "Pick chapters, press download. Pause, resume, retry, and carry on after you close the app."),
    ("sliders-horizontal", "Made for your reader's screen", "Cropping, spreads, long strips cut into screen-sized pages, contrast and dithering, with a live preview of the result."),
    ("hard-drive", "Send it straight to the device", "Export to a folder, a USB drive, an SD card or over Wi-Fi, in the formats your e-reader opens."),
    ("layers", "29 readers ready to go", "Xteink, Kindle, Kobo, Boox, PocketBook and reMarkable, each with its own size, formats and colour."),
    ("book-open", "A library and a reader", "Everything you downloaded or imported, with the reader you are using here, and exports for phones and tablets."),
];

/// The downloads: (name, what it is, id).
const DESKTOP_DOWNLOADS: [(&str, &str, &str); 3] = [
    ("macOS", "Apple silicon and Intel (.dmg)", "macos"),
    ("Windows", "Installer (.exe)", "windows"),
    ("Linux", ".deb, .rpm or the plain program", "linux"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Library,
    Readers,
    App,
    About,
}

impl Section {
    /// The part of the address after `#` (`/settings#reader`).
    pub fn slug(self) -> &'static str {
        SECTIONS.iter().find(|s| s.0 == self).map_or("library", |s| s.3)
    }

    pub fn from_slug(slug: &str) -> Option<Section> {
        SECTIONS.iter().find(|s| s.3 == slug).map(|s| s.0)
    }
}

const SECTIONS: [(Section, &str, &str, &str); 4] = [
    (Section::Library, "hard-drive", "Your library", "library"),
    (Section::Readers, "hard-drive", "Your reader", "reader"),
    (Section::App, "download", "App", "app"),
    (Section::About, "info", "About", "about"),
];
