//! The app's screens.

mod browse;
mod download_dialog;
mod host;
mod tasks_page;
mod update_dialog;
mod export_page;
mod import;
mod onboarding;
mod library_page;
mod plan_page;
mod presets;
mod reader;
mod search;
mod settings;
mod setup;
mod sources_page;
mod widgets;
pub mod work;
mod workspace;

// The art and the chapter table are shared with the website.
pub use mangapoint_ui::art::{cat, pixels, walker};
use mangapoint_ui::chapter_table;
use mangapoint_ui::readers;
use mangapoint_ui::dialogs::{ToastHost, UnsavedHost};
use mangapoint_ui::shell::{Backdrop, BarItem, CornerBar, Dock};

use crate::backend;
use crate::state::spawn_forever;
use crate::state::{AppState, BootState, Modal, ModalAction, Page};
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::use_wry_event_handler;
use dioxus::prelude::*;
use sources::SourceManager;
use widgets::ProgressBar;

/// What only the desktop app shows, after the shared style sheets (`mangapoint_ui::CSS`).
const DESKTOP_CSS: &str = include_str!("../../css/desktop.css");
/// Keyboard shortcuts.
const KEYS_SCRIPT: &str = include_str!("../js/keys.js");
/// Remembers the scroll position of each page.
const SCROLL_SCRIPT: &str = include_str!("../js/scroll.js");
/// Lets a first-run step slide out of the window.
const SLIDE_SCRIPT: &str = include_str!("../js/slide.js");

#[component]
pub fn App() -> Element {
    let state = AppState::init();
    use_context_provider(|| state);
    use_context_provider(|| state.ui);
    use_hook(|| state.ui.set_host(std::rc::Rc::new(host::DesktopHost(state))));

    // Dev flag: `-- --toast-demo` shows one of each kind of toast.
    use_hook(|| {
        if std::env::args().any(|a| a == "--toast-demo") {
            state.notify("Reader saved.");
            state.warn("That drive is almost full: 300 MB left.");
            state.error("Nothing answered at http://192.168.3.3. Is the reader on?");
            // And one of the system's own, to see who it says it is from.
            crate::notify::finished(state, "Export finished", "The Greatest Estate Developer: Exported 1 file(s).");
        }
    });
    // Debug builds can start an import from a file dropped in a folder (for trying it).
    use_hook(|| import::watch_for_test_imports(state));
    // Start the catalog server once, when the app opens.
    use_hook(|| {
        spawn_forever(backend::boot(state, false));
    });
    // An earlier update leaves nothing behind, and a new one is looked for once a day.
    use_hook(|| {
        crate::updates::clear_leftovers();
        spawn_forever(update_dialog::check_at_startup(state));
    });
    // A staging folder an import left when the app was closed in the middle of it
    // (only ones it marked as its own, whose process is gone) is cleared away, and
    // a chapter that was being replaced is put back.
    use_hook(|| {
        spawn_forever(async move {
            let library = state.library.peek().clone();
            let files = crate::state::library_files();
            let removed =
                tokio::task::spawn_blocking(move || pipeline::import::sweep_leftovers(&files, &library))
                    .await
                    .unwrap_or(0);
            if removed > 0 {
                tracing::info!("Removed {removed} unfinished import folder(s) left by an earlier run");
            }
        });
    });
    // Changes to the library and the queue are written in the background.
    use_hook(|| state.start_saver());
    // When the window closes: ask first if a download or export is running;
    // otherwise write what is still waiting to be saved and stop the server,
    // so no process is left behind.
    use_wry_event_handler(move |event, _| {
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            let download = state.queue.peek().running().is_some();
            let export = state
                .job
                .peek()
                .as_ref()
                .is_some_and(|v| v.result.is_none() && !v.paused);
            let importing = work::import_running();
            if let Some(warning) = crate::logic::quit_warning(download, export, importing) {
                // The window would close (or hide) once this returns. Make it
                // only hide, then show it again at once with the question.
                let window = dioxus::desktop::window();
                window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowHides);
                spawn_forever(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    window.window.set_visible(true);
                    window.window.set_focus();
                    window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
                });
                let mut modal = state.modal;
                modal.set(Some(Modal {
                    title: "Quit while work is running?".into(),
                    body: warning.into(),
                    confirm: "Quit anyway".into(),
                    cancel: "Keep working".into(),
                    danger: true,
                    action: ModalAction::QuitApp,
                }));
                return;
            }
            state.flush_saves();
            backend::shutdown();
        }
    });

    use_hook(|| {
        spawn(async move {
            // Floats the explanation of a question mark above everything else.
            if let Some(tip) = mangapoint_ui::scripts::call("tooltip", "") {
                let _ = document::eval(&tip).await;
            }
            let _ = document::eval(KEYS_SCRIPT).await;
            let _ = document::eval(SCROLL_SCRIPT).await;
            let _ = document::eval(SLIDE_SCRIPT).await;
        });
    });

    let snappy = state.ui.settings.read().animations == library::Animations::Snappy;
    let boot = state.boot.read().clone();
    let setup_step = state.ui.settings.read().setup_step;
    rsx! {
        style { {mangapoint_ui::CSS} }
        style { {DESKTOP_CSS} }
        div { class: "app", "data-motion": if snappy { "snappy" } else { "clean" },
            {match boot {
                BootState::Failed(message) => rsx! { Failed { message } },
                // The very first run: the engine, sources and a reader, step by step.
                _ if setup_step != library::SetupStep::Done => rsx! { onboarding::Onboarding {} },
                // An engine still to download waits on the splash. Otherwise the
                // app opens at once; only what needs the engine (the search) waits.
                BootState::Starting { message, percent } if !state.engine_installed => rsx! { Splash { message, percent } },
                _ => rsx! { Shell {} },
            }}
            DesktopConfirmation {}
            UnsavedHost {}
            download_dialog::DownloadDialogHost {}
            update_dialog::UpdateDialogHost {}
            import::ImportHost {}
            ToastHost {}
        }
    }
}

#[component]
fn Splash(message: String, percent: Option<u8>) -> Element {
    let fraction = percent.map(|p| p as f32 / 100.0);
    rsx! {
        div { class: "splash",
            walker::WalkingCat {}
            h1 { "MangaPoint" }
            p { class: "muted", "{message}" }
            ProgressBar { fraction }
            p { class: "muted small", style: "margin-top: 16px",
                "The first start downloads the catalog engine (about 340 MB). This only happens once."
            }
        }
    }
}

#[component]
fn Failed(message: String) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "splash",
            h1 { "Something went wrong" }
            div { class: "banner bad", "{message}" }
            button {
                class: "btn primary",
                onclick: move |_| {
                    spawn_forever(backend::boot(state, true));
                },
                "Try again"
            }
        }
    }
}

#[component]
fn Shell() -> Element {
    let state = use_context::<AppState>();
    let page = *state.page.read();
    // How many batches are downloading or waiting, for the badge on Tasks. The count is all the
    // dock shows: it changes when a batch comes or goes, not with every page that arrives.
    let in_queue = use_memo(move || {
        let (running, waiting, _) = state.queue.read().counts();
        running + waiting
    })();
    let busy = use_memo(move || state.queue.read().running().is_some() || state.job.read().as_ref().is_some_and(|j| j.result.is_none()))();
    // An open series belongs to wherever it was opened from.
    let opened_from_library = state.workspace.read().as_ref().is_some_and(|w| w.origin == Page::Library);
    let owner = match page {
        Page::Series if opened_from_library => Page::Library,
        Page::Series => Page::Search,
        other => other,
    };
    let scene = if page == Page::Search { "search" } else { "" };
    let mut tasks = BarItem::new("tasks", "Tasks", "pulse", owner == Page::Tasks);
    tasks.badge = (in_queue > 0).then(|| in_queue.to_string());
    tasks.busy = busy;
    rsx! {
        Backdrop { scene }
        if page != Page::Reader {
        Dock {
            items: vec![
                BarItem::new("search", "Search", "nav-search", owner == Page::Search),
                BarItem::new("library", "Library", "nav-library", owner == Page::Library),
                BarItem::new("export", "Export", "nav-export", owner == Page::Export),
            ],
            on_pick: move |id| pick(state, id),
        }
        CornerBar {
            items: vec![tasks, BarItem::new("settings", "Settings", "gear", owner == Page::Settings)],
            on_pick: move |id| pick(state, id),
        }
        }
        main { class: "content",
            div { class: "page",
                // The search stays built while another page is open, so coming
                // back to it is instant and it is as it was left.
                div { class: if page == Page::Search { "keep" } else { "keep gone" },
                    search::SearchPage {}
                }
                div { class: if page == Page::Library { "keep lib" } else { "keep gone lib" },
                    library_page::LibraryPage {}
                }
                {match page {
                    Page::Search => rsx! {},
                    Page::Series => rsx! { workspace::WorkspacePage {} },
                    Page::Library => rsx! {},
                    Page::Tasks => rsx! { tasks_page::TasksPage {} },
                    Page::Plan => rsx! { plan_page::PlanPage {} },
                    Page::Export => rsx! { export_page::ExportPage {} },
                    Page::Settings | Page::Sources => rsx! { settings::SettingsPage {} },
                    Page::Reader => rsx! { reader::ReaderPage {} },
                }}
            }
        }
    }
}

/// A press on the dock or the corner bar. A series left open is where you come back to; pressing
/// the place it belongs to while looking at it goes to the list.
fn pick(state: AppState, id: &str) {
    let page = match id {
        "search" => Page::Search,
        "library" => Page::Library,
        "export" => Page::Export,
        "tasks" => Page::Tasks,
        _ => Page::Settings,
    };
    let current = *state.page.read();
    // On the Export screen the series that was open waits to be put back.
    let open_from = if current == Page::Export {
        state.parked_workspace.read().as_ref().map(|w| w.origin)
    } else {
        state.workspace.read().as_ref().map(|w| w.origin)
    };
    match open_from {
        Some(origin) if origin == page && current == Page::Series => state.close_series(page),
        Some(origin) if origin == page && current != Page::Series => state.go(Page::Series),
        _ => state.go(page),
    }
}

/// Confirmation dialogs.
#[component]
fn DesktopConfirmation() -> Element {
    let state = use_context::<AppState>();
    let Some(modal) = state.modal.read().clone() else { return rsx! {} };
    rsx! {
        mangapoint_ui::dialogs::Confirmation {
            title: modal.title, body: modal.body, confirm: modal.confirm, cancel: modal.cancel,
            danger: modal.danger, dismiss_outside: false,
            on_cancel: move |_| { let mut m = state.modal; m.set(None); },
            on_confirm: move |_| {
                let mut m = state.modal;
                m.set(None);
                run_action(state, modal.action.clone());
            },
        }
    }
}

fn run_action(state: AppState, action: ModalAction) {
    spawn_forever(async move {
        match action {
            ModalAction::EnableBrowser { then_install } => {
                backend::enable_browser(state).await;
                if let Some(id) = then_install {
                    backend::install_default(state, &id).await;
                }
            }
            ModalAction::DisableBrowser => backend::disable_browser(state).await,
            ModalAction::RemoveSeries { series_ids } => {
                work::remove_from_library(state, series_ids)
            }
            ModalAction::DeleteChapters { series_id, keys } => {
                work::delete_chapters(state, series_id, keys)
            }
            ModalAction::DeleteDownloads { series_ids } => {
                work::delete_downloads(state, series_ids)
            }
            ModalAction::DeletePreset { id } => presets::delete_preset(state, id),
            ModalAction::RemoveReader { id } => {
                let mut settings = state.ui.settings;
                {
                    let mut st = settings.write();
                    st.custom_devices.retain(|d| d.id != id);
                    st.device_presets.remove(&id);
                    st.reader_setups.remove(&id);
                    st.device = st.custom_devices.first().map(|d| d.id.clone()).unwrap_or_default();
                }
                state.save_settings();
            }
            ModalAction::ClearFinishedDownloads => state.edit_queue(|q| q.clear_finished()),
            ModalAction::CancelAllDownloads => work::cancel_all_downloads(state),
            ModalAction::ResumeExportAnyway => work::resume_export_unchecked(state),
            ModalAction::QuitApp => {
                state.flush_saves();
                backend::shutdown();
                dioxus::desktop::window().close();
            }
            ModalAction::LoadPreset { series_id, preset_id, settings } => {
                presets::apply_to_series(state, &series_id, preset_id, settings)
            }
            ModalAction::SavePreset { id, settings } => presets::save_to_preset(state, &id, settings),
            ModalAction::DeleteExport { record_id } => work::delete_export(state, record_id),
            ModalAction::DeleteExportedFile { record_id, path } => {
                work::delete_exported_file(state, record_id, path)
            }
            ModalAction::RemoveExtension(pkg) => {
                let Some(client) = state.client() else { return };
                tracing::info!("Removing source {pkg}");
                let mut sources_state = state.sources.read().clone();
                let result = SourceManager::new(&*client)
                    .remove(&mut sources_state, &pkg)
                    .await;
                let mut s = state.sources;
                s.set(sources_state);
                state.save_sources();
                match result {
                    Ok(()) => state.notify("Source removed."),
                    Err(e) => state.error(format!("Could not remove it: {e}")),
                }
            }
        }
    });
}

/// Ask before turning on browser support. Used wherever a source needs it.
pub fn ask_enable_browser(state: AppState, then_install: Option<String>) {
    state.ask(Modal {
        title: "Turn on browser support?".into(),
        body: catalog::browser::ENABLE_PROMPT.into(),
        confirm: "Turn on".into(),
        cancel: "Not now".into(),
        danger: false,
        action: ModalAction::EnableBrowser { then_install },
    });
}

#[cfg(test)]
mod script_tests {
    const SCRIPTS: [(&str, &str); 9] = [
        ("crop-drag.js", include_str!("../js/crop-drag.js")),
        ("export-split.js", include_str!("../../../../crates/ui/js/export-split.js")),
        ("preview-editor.js", include_str!("../../../../crates/ui/js/preview-editor.js")),
        ("tooltip.js", include_str!("../../../../crates/ui/js/tooltip.js")),
        ("zoom-scrub.js", include_str!("../../../../crates/ui/js/zoom-scrub.js")),
        ("adder-scroll.js", include_str!("../../../../crates/ui/js/adder-scroll.js")),
        ("keys.js", include_str!("../js/keys.js")),
        ("scroll.js", include_str!("../js/scroll.js")),
        ("reader.js", include_str!("../js/reader.js")),
    ];

    /// `cropBound` as it is written in HTML: `data-crop-bound`.
    fn attribute(camel: &str) -> String {
        let mut out = String::from("data-");
        for c in camel.chars() {
            if c.is_ascii_uppercase() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        }
        out
    }

    /// A script that marks an element with `dataset.x = ...` and also looks
    /// elements up with `[data-x]` will find the marked element too. That once
    /// made a whole screen swallow every click, so it must not come back.
    #[test]
    fn a_script_never_marks_what_it_also_selects() {
        for (name, source) in SCRIPTS {
            for line in source.lines() {
                let Some(at) = line.find(".dataset.") else {
                    continue;
                };
                let rest = &line[at + ".dataset.".len()..];
                let field: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if !rest[field.len()..].trim_start().starts_with("= ")
                    && !rest[field.len()..].trim_start().starts_with('=')
                {
                    continue;
                }
                let selector = format!("[{}", attribute(&field));
                assert!(
                    !source.contains(&selector),
                    "{name} sets dataset.{field} and also selects {selector}"
                );
            }
        }
    }

    #[test]
    fn the_scripts_are_not_empty() {
        for (name, source) in SCRIPTS {
            assert!(source.trim().len() > 100, "{name}");
        }
    }
}
