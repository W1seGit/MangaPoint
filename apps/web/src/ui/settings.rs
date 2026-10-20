//! Settings and About: what is kept on this device, the reading-state file,
//! the privacy promise, installing the app, and deleting everything.

use super::widgets::{Icon, ProgressBar, SettingRow};
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
                                note: storage_note,
                                if caps.persist {
                                    if i.persisted {
                                        span { class: "chip ok", id: "persisted", "Protected from clean-up" }
                                    } else {
                                        button { class: "btn", id: "persist",
                                            onclick: move |_| { dioxus::core::spawn_forever(async move {
                                                let granted = bridge::call::<bool>("persist", json!({})).await.unwrap_or(false);
                                                if granted { ctx.notify("Your library is now protected from automatic clean-up."); } else { ctx.warn("The browser did not agree. Installing the app, or using the site more, often makes it say yes."); }
                                                refresh.set(refresh() + 1);
                                            }); },
                                            Icon { name: "lock", size: 15 } "Keep my library safe"
                                        }
                                    }
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
                    Section::State => rsx! {
                        h2 { class: "sx-group", "Reading state" }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Library,
    Readers,
    State,
    App,
    About,
}

const SECTIONS: [(Section, &str, &str, &str); 5] = [
    (Section::Library, "hard-drive", "Your library", "library"),
    (Section::Readers, "hard-drive", "Your reader", "readers"),
    (Section::State, "save", "Reading state", "state"),
    (Section::App, "download", "App", "app"),
    (Section::About, "info", "About", "about"),
];
