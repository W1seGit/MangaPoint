//! Settings and About: what is kept on this device, the reading-state file,
//! the privacy promise, installing the app, and deleting everything.

use super::widgets::{Icon, ProgressBar};
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
    let offline_ready = *ctx.offline_ready.read();
    let engine = web_engine::engine_version();

    rsx! {
        div { class: "q-head",
            div { class: "grow",
                h1 { "Settings" }
                p { class: "muted", "What this site keeps on your device, and how to carry your place to another one." }
            }
        }

        div { class: "card privacy", id: "privacy",
            h2 { Icon { name: "lock", size: 18 } "Your books stay on your device" }
            p { "MangaPoint on the web is a page that runs entirely in your browser. The books you import, your reading place and the files you export are never uploaded. There is no account, no analytics and no server that stores anything of yours: the people who host this page only serve its files." }
            p { class: "muted small", style: "margin-bottom: 0", "Closing the tab keeps everything. Clearing this site's data in your browser, or the button below, deletes it." }
        }

        div { class: "card", id: "storage-card",
            h2 { Icon { name: "hard-drive", size: 18 } "Storage on this device" }
            p { class: "muted small",
                "{plural(books, \"book\")}, {human_bytes(total)} imported."
                if i.quota > 0.0 { " The browser reports {human_bytes(i.used as u64)} used of about {human_bytes(i.quota as u64)} it allows this site." }
            }
            if fraction.is_some() { ProgressBar { fraction } }
            div { class: "row wrap", style: "margin-top: 10px",
                if caps.persist {
                    if i.persisted {
                        span { class: "chip ok", id: "persisted", "Protected from automatic clean-up" }
                    } else {
                        button { class: "btn", id: "persist",
                            onclick: move |_| { dioxus::core::spawn_forever(async move {
                                let granted = bridge::call::<bool>("persist", json!({})).await.unwrap_or(false);
                                if granted { ctx.notify("Your library is now protected from automatic clean-up."); } else { ctx.warn("The browser did not agree. Installing the app, or using the site more, often makes it say yes."); }
                                refresh.set(refresh() + 1);
                            }); },
                            Icon { name: "lock", size: 15 } "Keep my library safe"
                        }
                        span { class: "muted small", "Browsers may delete a site's data when the device runs low on space, unless you allow this." }
                    }
                }
            }
            if !caps.opfs {
                div { class: "banner warn small", Icon { name: "triangle-alert", size: 16 }
                    div { "This browser cannot keep big files directly, so books are kept in its database instead (slower, and some browsers limit them). If a book is not kept, your place is still remembered and importing the file again resumes it." }
                }
            }
        }

        div { class: "card", id: "state-card",
            h2 { Icon { name: "save", size: 18 } "Reading state" }
            p { class: "muted small",
                "A small file with your place in every book, the chapters you read and how you like to read. Save it, and load it on another device or browser: books you already have there resume at once; the others show up as waiting for their file, and resume as soon as you import it."
            }
            div { class: "row wrap",
                button { class: "btn primary", id: "state-save", onclick: move |_| save_state(ctx, false),
                    Icon { name: "save", size: 15 } if file.is_some() { "Save reading state" } else { "Save reading state" }
                }
                button { class: "btn", id: "state-load", onclick: move |_| bridge::send("pick_state", json!({})),
                    Icon { name: "file-up", size: 15 } "Load reading state"
                }
                if caps.fsa && file.is_some() {
                    button { class: "btn ghost", id: "state-save-as", onclick: move |_| save_state(ctx, true), "Save to another file\u{2026}" }
                }
            }
            if let Some(name) = &file {
                p { class: "muted small", style: "margin: 8px 0 0", id: "state-file",
                    "Saves go straight to {name}. The browser may ask for permission each time you open the site."
                    button { class: "btn ghost small", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call_ok("forget_state_file", json!({})).await; let mut sf = ctx.state_file; sf.set(None); }); }, "Stop using it" }
                }
            } else if !caps.fsa {
                p { class: "muted small", style: "margin: 8px 0 0", "This browser downloads the file when you save. You can also drop a .mpstate file anywhere on the page to load it." }
            }
            if waiting > 0 {
                p { class: "muted small", style: "margin: 8px 0 0", id: "state-waiting", "{plural(waiting, \"book\")} waiting for their file (see the Library)." }
            }
        }

        div { class: "card", id: "app-card",
            h2 { Icon { name: "globe", size: 18 } "Use it like an app" }
            p { class: "muted small",
                "Install MangaPoint to open it from your home screen or desktop, full screen, and to read your library with no connection at all."
            }
            div { class: "row wrap",
                if caps.standalone {
                    span { class: "chip ok", "Installed" }
                } else if can_install {
                    button { class: "btn primary", id: "install", onclick: move |_| { dioxus::core::spawn_forever(async move { let _ = bridge::call::<String>("install", json!({})).await; }); }, Icon { name: "download", size: 15 } "Install the app" }
                } else {
                    span { class: "muted small", id: "install-hint", "In your browser's menu choose Install app, or Add to Home Screen (on iPhone and iPad: the Share button, then Add to Home Screen)." }
                }
                if offline_ready { span { class: "chip ok", id: "offline-ready", "Works offline" } }
            }
        }

        div { class: "card", id: "danger-card",
            h2 { Icon { name: "trash", size: 18 } "Delete everything" }
            p { class: "muted small", "Removes every book, your places and your settings from this browser. Files you exported or saved stay where they are." }
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

        div { class: "card", id: "about-card",
            h2 { Icon { name: "info", size: 18 } "About" }
            p { class: "muted small", style: "margin-bottom: 0",
                "MangaPoint on the web {env!(\"CARGO_PKG_VERSION\")} \u{b7} engine {engine}. It opens CBZ, ZIP, EPUB with pictures, folders of pictures and loose pictures. RAR, 7z and PDF are not supported on the web yet. It does not search for or download manga; the desktop app does."
            }
        }
    }
}
