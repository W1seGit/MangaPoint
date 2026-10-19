//! Preferences.

use super::ask_enable_browser;

use super::devices::DevicesEditor;
use super::widgets::{Icon, Toggle};
use crate::logic::{human_bytes, plural};
use crate::state::{paths, AppState, BootState, Modal, ModalAction, Page, SettingsSection};
use crate::updates::{self, UpdateState, VERSION};
use catalog::browser;
use dioxus::prelude::*;
use library::{DownloadLimits, DownloadSpeed, PageQuality};

/// The sections, in the order the sidebar lists them.
const SECTIONS: [(SettingsSection, &str, &str); 5] = [
    (SettingsSection::Reader, "hard-drive", "Your reader"),
    (SettingsSection::Downloads, "download", "Downloads"),
    (SettingsSection::Files, "folder-open", "Files"),
    (SettingsSection::Preferences, "sliders-horizontal", "Preferences"),
    (SettingsSection::About, "info", "About"),
];

#[component]
pub fn SettingsPage() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let limits = s.download.limits();
    let services = state.services.read().clone();
    // Walking the runtime folder takes a while, so it is done once in the
    // background and again when browser support or the server changes, never
    // while the page is drawn.
    // The memo only changes when browser support, the server's folder or the
    // server starting changes, so unrelated settings changes do not walk the
    // folder again.
    let browser_dir = use_memo(move || {
        let enabled = state.settings.read().browser_support;
        let starting = matches!(*state.boot.read(), BootState::Starting { .. });
        let dir = state.services.read().as_ref().map(|sv| sv.data_dir.clone());
        dir.filter(|_| enabled && !starting)
    });
    let browser_size = use_resource(move || async move {
        let dir = browser_dir()?;
        tokio::task::spawn_blocking(move || browser::size_on_disk(&dir))
            .await
            .ok()
    });
    let on_macos = !browser::supported();
    let starting = matches!(*state.boot.read(), BootState::Starting { .. });
    let (series_count, chapter_count, library_bytes) = {
        let library = state.library.read();
        (
            library.series.len(),
            library
                .series
                .iter()
                .map(|s| s.chapters.len())
                .sum::<usize>(),
            library.total_bytes(),
        )
    };

    let update = move |change: fn(&mut library::Settings, String), value: String| {
        let mut settings = state.settings;
        change(&mut settings.write(), value);
        state.save_settings();
    };

    let section = *state.settings_section.read();
    let export_folder = s
        .export
        .output_dir
        .clone()
        .unwrap_or_else(|| paths().default_export_dir())
        .display()
        .to_string();

    rsx! {
        div { class: "settings",
            nav { class: "settings-nav",
                for (sec, icon, label) in SECTIONS {
                    button {
                        key: "{label}",
                        class: if sec == section { "nav-item active" } else { "nav-item" },
                        onclick: move |_| state.leave(crate::state::Leave::Section(sec)),
                        Icon { name: icon, size: 17 }
                        "{label}"
                    }
                }
            }
            div { class: "settings-content",
                {match section {
                    SettingsSection::Reader => rsx! {
                        DevicesEditor {}
                    },
                    SettingsSection::Downloads => rsx! {
                        Group { title: "Speed" }
                        Row {
                            label: "Download speed",
                            desc: format!(
                                "{} Right now: {} at a time, {} pages in flight. If a source starts refusing requests, MangaPoint slows down by itself and speeds up again later.",
                                match s.download.speed {
                                    DownloadSpeed::Gentle => "Slow and careful.",
                                    DownloadSpeed::Normal => "A good balance.",
                                    DownloadSpeed::Fast => "Quick; some sources may refuse requests.",
                                    DownloadSpeed::Ultra => "The fastest, and the recommended one.",
                                    DownloadSpeed::Custom => "Your own numbers.",
                                },
                                plural(limits.chapters, "chapter"),
                                limits.pages
                            ),
                            div { class: "seg",
                                for (speed, label) in [
                                    (DownloadSpeed::Gentle, "Gentle"),
                                    (DownloadSpeed::Normal, "Normal"),
                                    (DownloadSpeed::Fast, "Fast"),
                                    (DownloadSpeed::Ultra, "Ultra"),
                                    (DownloadSpeed::Custom, "Advanced"),
                                ] {
                                    button {
                                        key: "{label}",
                                        class: if s.download.speed == speed { "on" } else { "" },
                                        onclick: move |_| { let mut st = state.settings; st.write().download.speed = speed; state.save_settings(); },
                                        "{label}"
                                    }
                                }
                            }
                        }
                        if s.download.speed == DownloadSpeed::Custom {
                            Row { label: "Chapters at a time", desc: format!("1 to {}", DownloadLimits::MAX_CHAPTERS),
                                input {
                                    r#type: "number", min: "1", max: "{DownloadLimits::MAX_CHAPTERS}", value: "{s.download.custom_chapters}",
                                    oninput: move |e| update(|s, v| s.download.custom_chapters = v.trim().parse().unwrap_or(1), e.value()),
                                }
                            }
                            Row { label: "Pages in flight", desc: format!("1 to {}. Past about 50 made no difference in tests.", DownloadLimits::MAX_PAGES),
                                input {
                                    r#type: "number", min: "1", max: "{DownloadLimits::MAX_PAGES}", value: "{s.download.custom_pages}",
                                    oninput: move |e| update(|s, v| s.download.custom_pages = v.trim().parse().unwrap_or(1), e.value()),
                                }
                            }
                            div { class: "banner warn small",
                                Icon { name: "triangle-alert", size: 16 }
                                "High numbers can get your address blocked by a source."
                            }
                        }

                        Group { title: "Mixing sources" }
                        Row { label: "Share chapters between sources", desc: "Different chapters come from different sources at once, which adds their speeds together. Can mix translations within one series; leave it off for a consistent read.",
                            Toggle {
                                on: s.download.spread_sources,
                                onchange: move |on: bool| update(|s, v| s.download.spread_sources = v == "true", on.to_string()),
                            }
                        }
                        Row { label: "Race every source", desc: "Asks every source for each chapter and keeps the first to finish. Fastest, but sends the most requests. Can mix translations within one series.",
                            Toggle {
                                on: s.download.race_sources,
                                onchange: move |on: bool| update(|s, v| s.download.race_sources = v == "true", on.to_string()),
                            }
                        }

                        Group { title: "Notifications" }
                        Row { label: "Tell me when something finishes", desc: "A notification from your system when a download or an export is done, or could not be finished.",
                            Toggle {
                                on: s.notifications,
                                onchange: move |on: bool| { let mut st = state.settings; st.write().notifications = on; state.save_settings(); },
                            }
                        }

                        Group { title: "Saved pages" }
                        Row {
                            label: "Page quality",
                            desc: match s.page_quality {
                                PageQuality::Ultra => "As small as still reads well on an e-ink screen, about a tenth of the full size. Best for small readers. Applies to new downloads.",
                                PageQuality::Save => "Tall pages are shrunk to twice your reader's screen, which loses nothing you can see. Applies to new downloads.",
                                PageQuality::Full => "Pages are kept exactly as downloaded. Uses the most space.",
                            },
                            div { class: "seg",
                                for (quality, name) in [(PageQuality::Ultra, "Ultra save"), (PageQuality::Save, "Save"), (PageQuality::Full, "Full")] {
                                    button {
                                        key: "{name}",
                                        class: if s.page_quality == quality { "on" } else { "" },
                                        onclick: move |_| { let mut st = state.settings; st.write().page_quality = quality; state.save_settings(); },
                                        "{name}"
                                    }
                                }
                            }
                        }
                        Row {
                            label: "Keep colors (uses more space)",
                            desc: "Saves shrunk pages in color instead of gray, so a color reader can show them in color. Pages are bigger, and only new downloads change: a page already saved gray cannot get its color back. Always on while a color reader is selected. Has no effect with Full, which keeps pages as downloaded.",
                            Toggle {
                                on: s.keeps_color_in_downloads(),
                                onchange: move |on: bool| update(|s, v| s.download.keep_color = v == "true", on.to_string()),
                            }
                        }
                        Group { title: "Browser" }
                        Row {
                            label: "Browser support",
                            desc: if on_macos {
                                "Not available on macOS.".to_string()
                            } else if s.browser_support {
                                match *browser_size.read() {
                                    Some(Some(bytes)) => {
                                        format!("On, using {} of disk space.", human_bytes(bytes))
                                    }
                                    _ => "On.".to_string(),
                                }
                            } else {
                                "A few sources need a real web browser. Turning this on downloads about 270 MB, once.".to_string()
                            },
                            if on_macos {
                            } else if s.browser_support {
                                button {
                                    class: "btn danger", disabled: starting,
                                    onclick: move |_| state.ask(Modal {
                                        title: "Remove browser support?".into(),
                                        body: browser::REMOVE_WARNING.into(),
                                        confirm: "Remove it".into(),
                                        cancel: "Keep it".into(),
                                        danger: true,
                                        action: ModalAction::DisableBrowser,
                                    }),
                                    "Remove…"
                                }
                            } else {
                                button { class: "btn", disabled: starting, onclick: move |_| ask_enable_browser(state, None), "Turn on…" }
                            }
                        }
                        Group { title: "Library" }
                        Row {
                            label: "Your library",
                            note: format!("{}, {}, {}", plural(series_count, "series"), plural(chapter_count, "chapter"), human_bytes(library_bytes)),
                            button { class: "btn", onclick: move |_| state.go(Page::Library), Icon { name: "book-open", size: 15 } "Open Library" }
                        }
                    },
                    SettingsSection::Files => rsx! {
                        Group { title: "Exported files" }
                        Row { label: "Save to folder", desc: "Where the files for your reader are made.",
                            div { class: "sx-inline",
                                input {
                                    r#type: "text", readonly: true, title: "{export_folder}",
                                    value: "{export_folder}",
                                }
                                button { class: "btn", title: "Choose where exported files are made",
                                    onclick: move |_| {
                                        let start = Some(
                                            state.settings.peek().export.output_dir.clone().unwrap_or_else(|| paths().default_export_dir()),
                                        );
                                        spawn(async move {
                                            if let Some(dir) = super::widgets::pick_folder("Where should the files go?", start).await {
                                                let mut st = state.settings;
                                                st.write().export.output_dir = Some(dir);
                                                state.save_settings();
                                            }
                                        });
                                    },
                                    Icon { name: "folder-open", size: 15 }
                                    "Choose folder…"
                                }
                            }
                        }
                        Row { label: "Save to reader", desc: "This is managed in the Your reader section: how files get to each reader, and the folder they go into.",
                            button { class: "btn",
                                onclick: move |_| { let mut current = state.settings_section; current.set(SettingsSection::Reader); },
                                Icon { name: "hard-drive", size: 15 }
                                "Your reader"
                            }
                        }
                        Group { title: "Downloaded chapters" }
                        Row {
                            label: "Download folder",
                            desc: "New downloads are kept in this folder. Series you already have stay where they are.",
                            note: crate::state::library_files().root().display().to_string(),
                            button { class: "btn", title: "Choose where new downloads are kept",
                                onclick: move |_| {
                                    spawn(async move {
                                        let start = Some(crate::state::library_files().root().to_path_buf());
                                        if let Some(dir) = super::widgets::pick_folder("Where should downloads be kept?", start).await {
                                            super::work::set_download_folder(state, dir);
                                        }
                                    });
                                },
                                Icon { name: "folder-open", size: 15 }
                                "Choose folder…"
                            }
                        }
                        Row { label: "Ask each time", desc: "Show where files go whenever you start a download.",
                            Toggle {
                                on: s.ask_download_folder,
                                onchange: move |on: bool| { let mut st = state.settings; st.write().ask_download_folder = on; state.save_settings(); },
                            }
                        }
                        Row { label: "Open the downloads folder", desc: "Where the pages you download are kept.",
                            button { class: "btn", title: "Open the downloads folder",
                                onclick: move |_| {
                                    let dir = crate::state::library_files().root().to_path_buf();
                                    let _ = std::fs::create_dir_all(&dir);
                                    super::widgets::open_folder(&dir);
                                },
                                Icon { name: "folder-open", size: 15 } "Open folder"
                            }
                        }
                    },
                    SettingsSection::Preferences => rsx! {
                        Group { title: "Animations" }
                        Row { label: "Animation style", desc: "Clean fades pages in one part after another. Snappy skips that: pages and menus appear as fast as they can.",
                            div { class: "seg",
                                button {
                                    class: if s.animations == library::Animations::Clean { "on" } else { "" },
                                    onclick: move |_| { let mut st = state.settings; st.write().animations = library::Animations::Clean; state.save_settings(); },
                                    "Clean"
                                }
                                button {
                                    class: if s.animations == library::Animations::Snappy { "on" } else { "" },
                                    onclick: move |_| { let mut st = state.settings; st.write().animations = library::Animations::Snappy; state.save_settings(); },
                                    "Snappy"
                                }
                            }
                        }
                    },
                    SettingsSection::About => rsx! {
                        UpdateRow {}
                        Row {
                            label: "Catalog engine",
                            note: match &services {
                                Some(sv) => format!("Suwayomi {}, running on this computer only (port {}).", catalog::release::VERSION_TAG, sv.port),
                                None => format!("Suwayomi {}", catalog::release::VERSION_TAG),
                            },
                        }
                        Row { label: "Your data", note: paths().root().display().to_string(),
                            button { class: "btn", title: "Open this folder",
                                onclick: move |_| {
                                    let dir = paths().root().to_path_buf();
                                    let _ = std::fs::create_dir_all(&dir);
                                    super::widgets::open_folder(&dir);
                                },
                                Icon { name: "folder-open", size: 15 } "Open folder"
                            }
                        }
                    },
                }}
            }
        }
    }
}

/// The version of MangaPoint, a button that looks for a newer one (and opens the
/// offer when there is one, even if that version was skipped), and the switch
/// for looking when the app starts.
#[component]
fn UpdateRow() -> Element {
    let state = use_context::<AppState>();
    let mut update = use_signal(|| UpdateState::Idle);
    let checks = state.settings.read().update_check;
    let note = match update() {
        UpdateState::Idle => format!("MangaPoint {VERSION}"),
        UpdateState::Checking => format!("MangaPoint {VERSION}. Looking for a newer version\u{2026}"),
        UpdateState::UpToDate => format!("MangaPoint {VERSION}. This is the newest version."),
        UpdateState::Available { ref version } => format!("MangaPoint {VERSION}. Version {version} is out."),
        UpdateState::Failed(ref why) => format!("MangaPoint {VERSION}. {why}"),
    };
    rsx! {
        Row { label: "Version", note,
            button { class: "btn", disabled: matches!(update(), UpdateState::Checking),
                onclick: move |_| {
                    spawn(async move {
                        update.set(UpdateState::Checking);
                        match updates::check().await {
                            Ok(Some(found)) => {
                                update.set(UpdateState::Available { version: found.version.clone() });
                                super::update_dialog::offer(state, found);
                            }
                            Ok(None) => update.set(UpdateState::UpToDate),
                            Err(why) => update.set(UpdateState::Failed(why)),
                        }
                    });
                },
                Icon { name: "refresh-cw", size: 15 }
                "Check for updates"
            }
        }
        Row { label: "Check when MangaPoint starts",
            desc: "Once a day, MangaPoint asks GitHub whether a newer version is out, and offers it. Nothing else is sent. You can always check by hand above.",
            Toggle {
                on: checks,
                onchange: move |on: bool| { let mut st = state.settings; st.write().update_check = on; state.save_settings(); },
            }
        }
    }
}

/// A heading that groups a few settings.
#[component]
fn Group(title: String) -> Element {
    rsx! { h2 { class: "sx-group", "{title}" } }
}

/// One setting: what it is on the left, how to change it on the right.
#[component]
fn Row(
    label: String,
    /// Explains the setting, behind a question mark.
    desc: Option<String>,
    /// What is true right now, shown under the label.
    note: Option<String>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label",
                    "{label}"
                    if let Some(text) = desc { super::widgets::Help { text } }
                }
                if let Some(text) = note { div { class: "sx-desc", "{text}" } }
            }
            div { class: "sx-control", {children} }
        }
    }
}
