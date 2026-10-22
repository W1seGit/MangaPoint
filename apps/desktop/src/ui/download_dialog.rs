//! The dialog that asks where downloaded files should go, before a download
//! starts.

use super::widgets::{pick_folder, Icon, Toggle};
use super::work::{set_download_folder, start_pending};
use crate::state::{library_files, AppState};
use dioxus::prelude::*;

#[component]
pub fn DownloadDialogHost() -> Element {
    let state = use_context::<AppState>();
    let mut dont_ask = use_signal(|| false);
    // Original quality for just this download; the setting is left alone.
    let mut original = use_signal(|| false);
    let Some(dialog) = state.download_dialog.read().clone() else {
        return rsx! {};
    };
    let folder = library_files().root().display().to_string();
    let pending = dialog.pending;
    let close = move || {
        let mut d = state.download_dialog;
        d.set(None);
    };
    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "modal", onclick: move |e| e.stop_propagation(),
                h2 { "Download {dialog.summary}?" }
                p { class: "muted small", style: "margin: 0 0 10px",
                    "Choose where the downloaded pages are kept. Series you have already downloaded stay where they are; new ones go to this folder."
                }
                div { class: "folder-pick",
                    Icon { name: "folder-open", size: 16 }
                    span { class: "grow folder-path", title: "{folder}", "{folder}" }
                    button { class: "btn small",
                        onclick: move |_| {
                            spawn(async move {
                                let start = Some(library_files().root().to_path_buf());
                                if let Some(dir) = pick_folder("Where should downloads be kept?", start).await {
                                    set_download_folder(state, dir);
                                }
                            });
                        },
                        "Choose folder…"
                    }
                }
                div { class: "px-label", style: "margin-top: 14px", "Page quality" }
                div { class: "seg", id: "download-quality", role: "group", "aria-label": "Page quality",
                    button {
                        id: "download-quality-setting", class: if original() { "" } else { "on" },
                        "aria-pressed": "{!original()}", onclick: move |_| original.set(false),
                        "Use my setting"
                    }
                    button {
                        id: "download-quality-original", class: if original() { "on" } else { "" },
                        "aria-pressed": "{original()}", onclick: move |_| original.set(true),
                        "Original"
                    }
                }
                p { class: "muted small", style: "margin: 6px 0 0",
                    if original() {
                        "Pages are kept exactly as downloaded, in color. They take the most space. Good for reading on a phone or tablet."
                    } else {
                        "Pages are saved as your setting in Settings → Downloads says."
                    }
                }
                div { class: "check-line", style: "margin-top: 14px",
                    Toggle { on: dont_ask(), onchange: move |on: bool| dont_ask.set(on) }
                    span { "Don't ask again. You can change it in Settings → Files." }
                }
                div { class: "actions",
                    button { class: "btn", onclick: move |_| close(), "Cancel" }
                    button { class: "btn primary",
                        onclick: move |_| {
                            if dont_ask() {
                                let mut settings = state.ui.settings;
                                settings.write().ask_download_folder = false;
                                state.save_settings();
                            }
                            close();
                            start_pending(state, pending, original().then_some(library::PageQuality::Full));
                        },
                        Icon { name: "download", size: 16 }
                        "Download"
                    }
                }
            }
        }
    }
}
