//! Installed sources: where each comes from and whether it works, with
//! adding new ones in a dialog. The heading and its buttons stay in view.

use super::ask_enable_browser;
use super::widgets::{open_link, EmptyState, EngineStarting, Icon, Loading};
use crate::backend;
use crate::logic::{ago, plural, verdict_label};
use crate::state::spawn_forever;
use crate::state::{now_secs, AppState, Modal, ModalAction};
use catalog::Source;
use dioxus::prelude::*;
use library::Verdict;
use sources::defaults::EXTENSION_TRUST_WARNING;
use sources::{check_source, test_many, CheckOptions, InstalledSource, Requires, SourceManager};
use std::time::Duration;
use tracing::{info, warn};

#[component]
pub fn SourcesPage() -> Element {
    let state = use_context::<AppState>();
    let mut adding = use_signal(|| false);
    let mut closing = use_signal(|| false);

    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.ui.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });

    let now = now_secs();
    let health = state.health.read().clone();
    let testing = state.testing.read().clone();
    let browser_on = state.ui.settings.read().browser_support;

    let rows: Vec<Element> = match &*installed.read() {
        Some(Some(list)) => list
            .iter()
            .map(|item| source_card(state, item, &health, &testing, browser_on, now))
            .collect(),
        _ => Vec::new(),
    };
    let installed_count = rows.len();
    let loading = installed.read().is_none();

    let mut close = move || {
        if closing() {
            return;
        }
        closing.set(true);
        spawn_forever(async move {
            tokio::time::sleep(Duration::from_millis(180)).await;
            adding.set(false);
            closing.set(false);
        });
    };

    // Without the catalog engine there is nothing to list yet.
    if state.services.read().is_none() {
        return rsx! { EngineStarting {} };
    }

    rsx! {
        div { class: "rise-in",
        div { class: "sources-head",
            div { class: "grow",
                h1 { "Sources" }
                p { class: "muted",
                    if installed_count > 0 {
                        "{plural(installed_count, \"source\")} searched. Retest checks that one really works."
                    } else {
                        "The websites MangaPoint searches."
                    }
                }
            }
            button {
                class: "btn",
                disabled: installed_count == 0 || !testing.is_empty(),
                onclick: move |_| retest_all(state),
                Icon { name: "refresh-cw", size: 15 }
                "Retest all"
            }
            button { class: "btn primary", onclick: move |_| adding.set(true),
                Icon { name: "plus", size: 15 }
                "Add sources"
            }
        }

        if loading { Loading { label: "Loading your sources…" } }
        if !loading && installed_count == 0 {
            EmptyState {
                title: "No sources yet",
                message: "MangaPoint can't fetch anything without them. Add some with the button above.",
                compact: true,
            }
        }
        div { class: "src-list", {rows.into_iter()} }
        }

        if adding() {
            div { class: if closing() { "overlay out" } else { "overlay" }, onclick: move |_| close(),
                div { class: "modal add-sources", onclick: move |e| e.stop_propagation(),
                    AddSources { on_close: move |_| close() }
                }
            }
        }
    }
}

/// Which list the dialog for adding sources shows.
#[derive(Clone, Copy, PartialEq)]
enum AddTab {
    Recommended,
    Store,
}

/// The dialog for adding sources: the recommended ones (tested by
/// MangaPoint), or any extension store the user adds.
#[component]
fn AddSources(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut tab = use_signal(|| AddTab::Recommended);
    let mut store_url = use_signal(String::new);
    let mut filter = use_signal(String::new);
    // The store's list can be narrowed to one language, and shows no 18+ source unless asked.
    let mut store_lang = use_signal(|| state.ui.settings.read().language.clone());
    let mut show_nsfw = use_signal(|| false);
    let installed = use_resource(move || async move {
        let _ = state.sources_version.read();
        let lang = state.ui.settings.read().language.clone();
        let client = state.client()?;
        SourceManager::new(&*client).installed(&lang).await.ok()
    });
    let available = use_resource(move || async move {
        let _ = state.sources_version.read();
        let client = state.client()?;
        client.fetch_extensions().await.ok()
    });
    let query = filter.read().trim().to_lowercase();
    let wanted_lang = store_lang.read().clone();
    // The languages the stores offer, for the filter.
    let languages: Vec<String> = match &*available.read() {
        Some(Some(all)) => {
            let mut langs: Vec<String> = all.iter().filter(|e| !e.is_installed).map(|e| e.lang.clone()).collect();
            langs.sort();
            langs.dedup();
            langs
        }
        _ => Vec::new(),
    };
    let (candidates, matching): (Vec<catalog::Extension>, usize) = match &*available.read() {
        Some(Some(all)) => {
            let found: Vec<&catalog::Extension> = all
                .iter()
                .filter(|e| !e.is_installed)
                .filter(|e| wanted_lang.is_empty() || e.lang == wanted_lang || e.lang == "all")
                .filter(|e| show_nsfw() || !e.is_nsfw)
                .filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query))
                .collect();
            let total = found.len();
            (found.into_iter().take(60).cloned().collect(), total)
        }
        _ => (Vec::new(), 0),
    };
    let missing: Vec<&'static sources::DefaultSource> = sources::DEFAULTS
        .iter()
        .filter(|d| match &*installed.read() {
            Some(Some(list)) => !list.iter().any(|i| i.default.is_some_and(|x| x.id == d.id)),
            _ => false,
        })
        .collect();
    let loaded = installed.read().is_some();

    rsx! {
        div { class: "as-head",
            h2 { "Add sources" }
            button { class: "btn ghost small square-btn", title: "Close", onclick: move |_| on_close.call(()),
                Icon { name: "x", size: 16 }
            }
        }
        div { class: "seg as-tabs",
            button { class: if tab() == AddTab::Recommended { "on" } else { "" }, onclick: move |_| tab.set(AddTab::Recommended),
                "Recommended"
            }
            button { class: if tab() == AddTab::Store { "on" } else { "" }, onclick: move |_| tab.set(AddTab::Store),
                "From an extension store"
            }
        }
        div { class: "as-body",
            if tab() == AddTab::Recommended {
                p { class: "muted small as-lead", "Tested by MangaPoint. These are the ones we know work." }
                if loaded && missing.is_empty() {
                    EmptyState { title: "You have them all", message: "Every recommended source is installed.", compact: true }
                }
                div { class: "as-grid",
                    for d in missing.iter().copied() {
                        {
                            let needs_browser = d.requires == Requires::Browser;
                            let id = d.id;
                            rsx! {
                                div { class: "as-card", key: "{id}",
                                    div { class: "as-card-top",
                                        div { class: "src-avatar", "{initial_of(d.name)}" }
                                        div { class: "grow",
                                            strong { "{d.name}" }
                                            div { class: "muted small", "{host_of(d.site)}" }
                                        }
                                    }
                                    div { class: "muted small as-desc", "{d.description}" }
                                    div { class: "as-card-foot",
                                        if needs_browser { span { class: "src-status warn", span { class: "dot" } "Needs browser support" } }
                                        span { class: "grow" }
                                        button {
                                            class: "btn primary small",
                                            onclick: move |_| {
                                                if needs_browser && !state.ui.settings.read().browser_support {
                                                    ask_enable_browser(state, Some(id.to_string()));
                                                } else {
                                                    spawn_forever(async move {
                                                        if backend::install_default(state, id).await {
                                                            state.notify("Installed. Press Retest to check it.");
                                                        }
                                                        state.bump_sources();
                                                    });
                                                }
                                            },
                                            Icon { name: "download", size: 14 }
                                            "Install"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "banner warn small",
                    Icon { name: "triangle-alert", size: 16 }
                    "{EXTENSION_TRUST_WARNING}"
                }
                p { class: "muted small as-lead", "Add an extension store (an https address), then search it below." }
                form {
                    class: "as-form",
                    onsubmit: move |e| {
                        e.prevent_default();
                        let url = store_url.read().clone();
                        spawn_forever(async move {
                            let Some(client) = state.client() else { return };
                            let mut sources_state = state.sources.read().clone();
                            let result = SourceManager::new(&*client).add_store(&mut sources_state, &url).await;
                            let mut s = state.sources;
                            s.set(sources_state);
                            state.save_sources();
                            match result {
                                Ok(list) => state.notify(format!("Store added: {} extensions available.", list.len())),
                                Err(e) => state.error(format!("Could not add that store: {e}")),
                            }
                            state.bump_sources();
                        });
                        store_url.set(String::new());
                    },
                    input {
                        r#type: "url", class: "grow",
                        placeholder: "https://example.org/repo/index.pb",
                        value: "{store_url}",
                        oninput: move |e| store_url.set(e.value()),
                    }
                    button { class: "btn", r#type: "submit", disabled: store_url.read().trim().is_empty(),
                        Icon { name: "plus", size: 15 } "Add store"
                    }
                }
                div { class: "as-search",
                    Icon { name: "search", size: 16 }
                    input {
                        r#type: "search",
                        placeholder: "Search the available sources by name…",
                        value: "{filter}",
                        oninput: move |e| filter.set(e.value()),
                    }
                }
                div { class: "as-filters",
                    label { class: "as-filter",
                        span { "Language" }
                        select {
                            value: "{store_lang}",
                            onchange: move |e| store_lang.set(e.value()),
                            option { value: "", selected: wanted_lang.is_empty(), "All languages" }
                            for l in languages.iter() {
                                option { key: "{l}", value: "{l}", selected: *l == wanted_lang, "{lang_name(l)}" }
                            }
                        }
                    }
                    label { class: if show_nsfw() { "as-filter toggle on" } else { "as-filter toggle" },
                        input { r#type: "checkbox", checked: show_nsfw(), onchange: move |e| show_nsfw.set(e.checked()) }
                        span { "Show 18+" }
                    }
                    span { class: "grow" }
                    span { class: "muted small", "{matching} available" }
                }
                div { class: "as-results",
                    for ext in candidates.iter() {
                        {
                            let pkg = ext.pkg_name.clone();
                            let name = ext.name.clone();
                            rsx! {
                                div { class: "as-row", key: "{pkg}",
                                    div { class: "src-avatar", "{initial_of(&name)}" }
                                    div { class: "grow as-row-name",
                                        strong { "{name}" }
                                    }
                                    span { class: "lang-tag", "{ext.lang}" }
                                    if ext.is_nsfw { span { class: "lang-tag warn", "18+" } }
                                    button {
                                        class: "btn small",
                                        onclick: move |_| {
                                            let pkg = pkg.clone();
                                            spawn_forever(async move {
                                                let Some(client) = state.client() else { return };
                                                let mut sources_state = state.sources.read().clone();
                                                let result = SourceManager::new(&*client).install_custom(&mut sources_state, &pkg).await;
                                                let mut s = state.sources;
                                                s.set(sources_state);
                                                state.save_sources();
                                                match result {
                                                    Ok(()) => state.notify("Installed. Press Retest to check it."),
                                                    Err(e) => state.error(format!("Could not install it: {e}")),
                                                }
                                                state.bump_sources();
                                            });
                                        },
                                        Icon { name: "download", size: 14 }
                                        "Install"
                                    }
                                }
                            }
                        }
                    }
                    if candidates.is_empty() {
                        p { class: "muted small", "Nothing matches in your stores. Add a store above, or change the filters." }
                    }
                    if matching > candidates.len() {
                        p { class: "muted small", "Showing the first {candidates.len()}. Type to narrow the list." }
                    }
                }
            }
        }
    }
}

/// The first letter of a name, for the round badge before it.
fn initial_of(name: &str) -> String {
    name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

/// A language code as its name when it is a common one.
fn lang_name(code: &str) -> String {
    let name = match code {
        "all" => "Multiple languages",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" | "zh-hans" => "Chinese",
        "zh-hant" => "Chinese (traditional)",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "pt" | "pt-br" => "Portuguese",
        "ru" => "Russian",
        "id" => "Indonesian",
        "tr" => "Turkish",
        "ar" => "Arabic",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "pl" => "Polish",
        _ => return code.to_string(),
    };
    format!("{name} ({code})")
}

/// `https://www.example.org/path` as `example.org`.
fn host_of(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_string()
}

fn source_card(
    state: AppState,
    item: &InstalledSource,
    health: &library::HealthDb,
    testing: &std::collections::HashSet<String>,
    browser_on: bool,
    now: u64,
) -> Element {
    let source = item.source.clone();
    let id = source.id.clone();
    let last = health.latest(&id).cloned();
    let is_testing = testing.contains(&id);
    let reference = item
        .default
        .and_then(|d| d.reference_title)
        .map(String::from);
    let pkg = item.extension.pkg_name.clone();
    let name = source.name.clone();
    let site = item.default.map(|d| d.site);
    let description = item.default.map(|d| d.description);
    let needs_browser = last
        .as_ref()
        .is_some_and(|r| r.verdict == Verdict::NeedsBrowser)
        && !browser_on;
    let (chip_text, chip_kind) = match &last {
        None => ("not tested yet".to_string(), None),
        Some(r) => {
            let kind = match r.verdict {
                Verdict::Pass => "ok",
                Verdict::NeedsBrowser => "warn",
                _ => "bad",
            };
            (verdict_label(r.verdict).to_string(), Some(kind.to_string()))
        }
    };

    let status_class = if is_testing { "src-status testing".to_string() } else { format!("src-status {}", chip_kind.clone().unwrap_or_else(|| "idle".into())) };
    let status_text = if is_testing { "Testing…".to_string() } else { chip_text };
    let initial = initial_of(&name);
    // Why it failed, when it did; a source that works has nothing to explain.
    let why = last.as_ref().filter(|r| r.verdict != Verdict::Pass).map(|r| r.message.clone());
    let tested = last.as_ref().map(|r| format!("Tested {} · {:.1} s", ago(now, r.at), r.total_ms() as f64 / 1000.0));

    rsx! {
        div { class: "src-card", key: "{id}",
            div { class: "src-avatar", "{initial}" }
            div { class: "src-main",
                div { class: "src-title",
                    strong { class: "src-name", title: "Extension {pkg}", "{name}" }
                    span { class: "lang-tag", "{source.lang}" }
                    if source.is_nsfw { span { class: "lang-tag warn", "18+" } }
                }
                if let Some(site) = site {
                    button { class: "src-link", title: "Open {site} in your browser", onclick: move |_| open_link(site),
                        "{host_of(site)}"
                        Icon { name: "external-link", size: 12 }
                    }
                }
                if let Some(text) = description { div { class: "src-desc", "{text}" } }
                div { class: "src-status-row",
                    span { class: "{status_class}", span { class: "dot" } "{status_text}" }
                    if let Some(text) = tested { span { class: "muted small", "{text}" } }
                }
                if let Some(text) = why { div { class: "src-why", "{text}" } }
            }
            div { class: "src-actions",
                if needs_browser {
                    button { class: "btn small", onclick: move |_| ask_enable_browser(state, None), "Turn on browser support" }
                }
                button {
                    class: "btn small icon-btn",
                    title: "Test this source again",
                    "aria-label": "Retest {name}",
                    disabled: is_testing,
                    onclick: {
                        let source = source.clone();
                        let reference = reference.clone();
                        move |_| retest(state, source.clone(), reference.clone())
                    },
                    Icon { name: "refresh-cw", size: 14 }
                }
                button {
                    class: "btn small ghost danger icon-btn", title: "Remove this source", "aria-label": "Remove {name}",
                    onclick: move |_| {
                        state.ask(Modal {
                            title: "Remove this source?".into(),
                            body: format!("{name} will no longer be searched. You can install it again later."),
                            confirm: "Remove".into(),
                            cancel: "Keep".into(),
                            danger: true,
                            action: ModalAction::RemoveExtension(pkg.clone()),
                        });
                    },
                    Icon { name: "trash", size: 14 }
                }
            }
        }
    }
}

/// Test one source and record the result.
pub fn retest(state: AppState, source: Source, reference_title: Option<String>) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let mut testing = state.testing;
        testing.write().insert(source.id.clone());
        info!("Testing source {}…", source.name);
        let options = CheckOptions {
            reference_title,
            ..Default::default()
        };
        let record = check_source(&*client, &source, &options).await;
        log_test(&source.name, &record);
        {
            let mut health = state.health;
            health.write().record_test(&source.id, &source.name, record);
        }
        state.save_health();
        testing.write().remove(&source.id);
        state.bump_sources();
    });
}

/// One line per test: the verdict, how long it took, and why if it failed.
fn log_test(name: &str, record: &library::TestRecord) {
    let label = verdict_label(record.verdict);
    if record.verdict == Verdict::Pass {
        info!("  {name}: {label} ({} ms)", record.total_ms());
    } else {
        warn!("  {name}: {label}. {}", record.message);
    }
}

fn retest_all(state: AppState) {
    spawn_forever(async move {
        let Some(client) = state.client() else { return };
        let lang = state.ui.settings.read().language.clone();
        let Ok(installed) = SourceManager::new(&*client).installed(&lang).await else {
            return;
        };
        let jobs: Vec<(Source, CheckOptions)> = installed
            .iter()
            .map(|i| {
                (
                    i.source.clone(),
                    CheckOptions {
                        reference_title: i
                            .default
                            .and_then(|d| d.reference_title)
                            .map(String::from),
                        step_timeout: Duration::from_secs(45),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let mut testing = state.testing;
        info!("Testing all {} installed source(s)…", jobs.len());
        for (s, _) in &jobs {
            testing.write().insert(s.id.clone());
        }
        let _ = test_many(&*client, jobs, 3, |source, record| {
            log_test(&source.name, record);
            let mut health = state.health;
            health
                .write()
                .record_test(&source.id, &source.name, record.clone());
            testing.write().remove(&source.id);
            state.bump_sources();
        })
        .await;
        state.save_health();
    });
}
