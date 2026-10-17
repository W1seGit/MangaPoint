//! Importing books and pictures: the dialog that shows what was found (so it
//! can be corrected before anything is written), and the fields for a series'
//! details, which the page of an imported series uses too.

use super::widgets::{pick_files, Busy, Chip, Help, Icon, NoCover, ProgressBar, TagBox};
use super::work::{
    cancel_import, close_import, cover_from_file, first_page_cover, import_paths, run_import,
    start_import,
};
use crate::logic::plural;
use crate::state::{
    AppState, BookDraft, ChapterPlace, ImportSession, ImportStage, InfoDraft, Page,
};
use dioxus::prelude::*;
use std::sync::Arc;

const STATUSES: [&str; 5] = ["", "Ongoing", "Completed", "On hiatus", "Cancelled"];

/// The extensions of pictures that can be a cover.
const COVER_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "gif"];

/// The fields of a series' details. Controlled: every change gives the whole
/// draft back. `first_page` is the series whose first page can be made the
/// cover (when there is one).
#[component]
pub fn InfoFields(
    info: InfoDraft,
    onchange: EventHandler<InfoDraft>,
    show_title: bool,
    first_page: Option<String>,
) -> Element {
    let edit = {
        let info = info.clone();
        move |change: fn(&mut InfoDraft, String), text: String| {
            let mut next = info.clone();
            change(&mut next, text);
            onchange.call(next);
        }
    };
    let (on_title, on_author, on_artist, on_description, on_status) =
        (edit.clone(), edit.clone(), edit.clone(), edit.clone(), edit);
    let genres_info = info.clone();
    let choose_info = info.clone();
    let page_info = info.clone();
    let has_status = STATUSES.contains(&info.status.as_str());
    rsx! {
        div { class: "info-fields",
            if show_title {
                label { class: "info-field wide",
                    span { "Title" }
                    input {
                        r#type: "text", value: "{info.title}", spellcheck: "false",
                        oninput: move |e| on_title(|i, v| i.title = v, e.value()),
                    }
                }
            }
            label { class: "info-field",
                span { "Author" }
                input { r#type: "text", value: "{info.author}", spellcheck: "false",
                    oninput: move |e| on_author(|i, v| i.author = v, e.value()) }
            }
            label { class: "info-field",
                span { "Artist" }
                input { r#type: "text", value: "{info.artist}", spellcheck: "false",
                    oninput: move |e| on_artist(|i, v| i.artist = v, e.value()) }
            }
            label { class: "info-field",
                span { "Status" }
                select {
                    value: "{info.status}",
                    onchange: move |e| on_status(|i, v| i.status = v, e.value()),
                    for s in STATUSES {
                        option { key: "{s}", value: "{s}", selected: info.status == s,
                            if s.is_empty() { "Not set" } else { "{s}" }
                        }
                    }
                    if !has_status {
                        option { value: "{info.status}", selected: true, "{info.status}" }
                    }
                }
            }
            div { class: "info-field",
                span { "Genres" }
                TagBox {
                    value: info.genres.clone(),
                    placeholder: "Add a genre".to_string(),
                    label: "Add a genre".to_string(),
                    on_change: move |genres: Vec<String>| {
                        let mut next = genres_info.clone();
                        next.genres = genres;
                        onchange.call(next);
                    },
                }
            }
            label { class: "info-field wide",
                span { "Description" }
                textarea {
                    rows: "4", value: "{info.description}", spellcheck: "true",
                    oninput: move |e| on_description(|i, v| i.description = v, e.value()),
                }
            }
            div { class: "info-field wide info-cover",
                span { "Cover" }
                div { class: "info-cover-row",
                    div { class: "info-cover-art",
                        if let Some(url) = &info.cover_url { img { src: "{url}", alt: "The cover" } } else { NoCover {} }
                    }
                    div { class: "info-cover-actions",
                        button { class: "btn small", r#type: "button",
                            onclick: move |_| {
                                let current = choose_info.clone();
                                spawn(async move {
                                    let picked = pick_files("Choose a cover picture", None, "Pictures", &COVER_EXTENSIONS).await;
                                    if let Some((bytes, url)) = picked.and_then(|p| p.first().cloned()).and_then(|p| cover_from_file(&p)) {
                                        let mut next = current;
                                        next.cover = Some(bytes);
                                        next.cover_url = Some(url);
                                        onchange.call(next);
                                    }
                                });
                            },
                            Icon { name: "image", size: 14 } "Choose a picture…"
                        }
                        if let Some(series) = first_page.clone() {
                            button { class: "btn small", r#type: "button", title: "Use the first page of the first chapter",
                                onclick: move |_| {
                                    if let Some((bytes, url)) = first_page_cover(&series) {
                                        let mut next = page_info.clone();
                                        next.cover = Some(bytes);
                                        next.cover_url = Some(url);
                                        onchange.call(next);
                                    }
                                },
                                "Use the first page"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Change one book of the dialog.
fn edit_book(state: AppState, index: usize, change: impl FnOnce(&mut BookDraft)) {
    let mut signal = state.import;
    let mut guard = signal.write();
    if let Some(book) = guard.as_mut().and_then(|s| s.books.get_mut(index)) {
        change(book);
    }
}

/// The dialog, when an import is open. It sits over whatever screen it was
/// opened from.
#[component]
pub fn ImportHost() -> Element {
    let state = use_context::<AppState>();
    let Some(session) = state.import.read().clone() else {
        return rsx! {};
    };
    let busy = matches!(session.stage, ImportStage::Reading | ImportStage::Importing);
    let heading = match (&session.target, session.stage) {
        (_, ImportStage::Finished) => "Import finished".to_string(),
        (Some(id), _) => {
            let title = state
                .library
                .read()
                .find(id)
                .map(|s| s.title.clone())
                .unwrap_or_default();
            format!("Add pages to {title}")
        }
        (None, _) => "Import".to_string(),
    };
    rsx! {
        div { class: "overlay", onclick: move |_| { if !busy { close_import(state); } },
            div { class: "modal import", role: "dialog", "aria-label": "Import", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    h2 { "{heading}" }
                    button { class: "btn ghost small", title: "Close", onclick: move |_| close_import(state),
                        Icon { name: "x", size: 16 }
                    }
                }
                {match session.stage {
                    ImportStage::Reading => rsx! {
                        div { class: "import-wait", Busy { label: "Reading what you chose…" } }
                    },
                    ImportStage::Review => rsx! { ReviewStage { session: session.clone() } },
                    ImportStage::Importing => rsx! { ImportingStage { session: session.clone() } },
                    ImportStage::Finished => rsx! { FinishedStage { session: session.clone() } },
                }}
            }
        }
    }
}

#[component]
fn ReviewStage(session: ImportSession) -> Element {
    let state = use_context::<AppState>();
    let adding = session.target.is_some();
    let included = session.books.iter().filter(|b| b.include).count();
    let pages: usize = session
        .books
        .iter()
        .filter(|b| b.include)
        .flat_map(|b| &b.chapters)
        .filter(|c| c.include || c.meets.is_some())
        .map(|c| c.pages)
        .sum();
    let can_combine = session.books.len() > 1 && !adding;
    let label = if adding {
        "Add the pages".to_string()
    } else if session.combine && included > 1 {
        format!("Import as one series ({})", plural(pages, "page"))
    } else {
        format!("Import {} ({})", plural(included, "book"), plural(pages, "page"))
    };
    let target = session.target.clone();
    rsx! {
        div { class: "import-body",
            if !session.failures.is_empty() {
                div { class: "banner bad small import-failures",
                    Icon { name: "circle-alert", size: 16 }
                    div {
                        for (file, message) in session.failures.iter() {
                            div { key: "{file}{message}", "{message}" }
                        }
                    }
                }
            }
            if can_combine {
                label { class: "check-line import-combine",
                    input {
                        r#type: "checkbox", checked: session.combine,
                        onchange: move |e| {
                            let mut signal = state.import;
                            let mut guard = signal.write();
                            if let Some(s) = guard.as_mut() { s.combine = e.checked(); }
                        },
                    }
                    span { "Make one series of all of these, with the first one's details" }
                }
            }
            for (i, book) in session.books.iter().enumerate() {
                BookCard {
                    key: "{i}-{book.label}", index: i, book: book.clone(),
                    follower: session.combine && i > 0 && can_combine,
                    adding,
                    series: target.clone(),
                }
            }
            if session.books.is_empty() {
                p { class: "muted", "Nothing could be imported from what you chose." }
            }
        }
        div { class: "actions",
            button { class: "btn ghost", onclick: move |_| close_import(state), "Cancel" }
            button { class: "btn", title: "Choose more files to add to this", onclick: {
                    let target = session.target.clone();
                    move |_| start_import(state, target.clone(), false)
                },
                Icon { name: "file-up", size: 16 } "Choose other files"
            }
            button { class: "btn primary", disabled: included == 0 || pages == 0,
                onclick: move |_| run_import(state),
                "{label}"
            }
        }
    }
}

#[component]
fn BookCard(index: usize, book: BookDraft, follower: bool, adding: bool, series: Option<String>) -> Element {
    let state = use_context::<AppState>();
    let counted: usize = book.chapters.iter().map(|c| c.pages).sum();
    let chosen = book.chapters.iter().filter(|c| c.include).count();
    let class = if book.include { "import-book" } else { "import-book off" };
    let fixed_title = if adding {
        book.label.clone()
    } else {
        format!("{}: added to the first series", book.label)
    };
    let toggle_details = book.show_details;
    let details_icon = if book.show_details { "chevron-down" } else { "chevron-right" };
    let chapters_icon = if book.show_chapters { "chevron-down" } else { "chevron-right" };
    let chapters_label = format!("Chapters ({chosen} of {})", book.chapters.len());
    let toggle_chapters = book.show_chapters;
    rsx! {
        div { class: "{class}",
            div { class: "import-cover",
                if let Some(url) = &book.info.cover_url { img { src: "{url}", alt: "Cover of {book.info.title}" } } else { NoCover {} }
            }
            div { class: "import-main",
                div { class: "import-title-row",
                    input {
                        r#type: "checkbox", checked: book.include, title: "Import this one",
                        onchange: move |e| edit_book(state, index, |b| b.include = e.checked()),
                    }
                    if adding || follower {
                        strong { class: "import-title-fixed", "{fixed_title}" }
                    } else {
                        input {
                            class: "import-title", r#type: "text", value: "{book.info.title}", spellcheck: "false",
                            "aria-label": "Title",
                            oninput: move |e| edit_book(state, index, |b| b.info.title = e.value()),
                        }
                    }
                    Chip { text: book.format.to_string() }
                }
                p { class: "muted small import-count",
                    "{plural(book.chapters.len(), \"chapter\")} · {plural(counted, \"page\")}"
                    if !adding { " · from {book.label}" }
                }
                for w in book.warnings.iter() {
                    p { class: "import-warning small", key: "{w}", Icon { name: "triangle-alert", size: 13 } " {w}" }
                }
                div { class: "import-toggles",
                    if !adding {
                        button { class: "btn ghost small", r#type: "button",
                            onclick: move |_| edit_book(state, index, |b| b.show_details = !b.show_details),
                            Icon { name: details_icon, size: 13 }
                            "Details"
                        }
                    }
                    button { class: "btn ghost small", r#type: "button",
                        onclick: move |_| edit_book(state, index, |b| b.show_chapters = !b.show_chapters),
                        Icon { name: chapters_icon, size: 13 }
                        "{chapters_label}"
                    }
                }
                if toggle_details && !adding {
                    InfoFields {
                        info: book.info.clone(),
                        show_title: false,
                        first_page: None,
                        onchange: move |info: InfoDraft| edit_book(state, index, |b| b.info = info),
                    }
                }
                if toggle_chapters {
                    div { class: "import-chapters",
                        for (c, chapter) in book.chapters.iter().enumerate() {
                            div { class: if chapter.include || chapter.meets.is_some() { "import-chapter" } else { "import-chapter off" }, key: "{c}",
                                if chapter.meets.is_some() {
                                    select {
                                        class: "import-place", "aria-label": "What to do with a chapter you already have",
                                        value: "{place_key(chapter.place)}",
                                        onchange: move |e| {
                                            let place = place_of(&e.value());
                                            edit_book(state, index, |b| if let Some(ch) = b.chapters.get_mut(c) { ch.place = place; });
                                        },
                                        option { value: "skip", selected: chapter.place == ChapterPlace::Skip, "Already have it: skip" }
                                        option { value: "add", selected: chapter.place == ChapterPlace::Add, "Add these pages to it" }
                                        option { value: "replace", selected: chapter.place == ChapterPlace::Replace, "Replace it" }
                                        option { value: "both", selected: chapter.place == ChapterPlace::Both, "Keep both" }
                                    }
                                } else {
                                    input {
                                        r#type: "checkbox", checked: chapter.include, title: "Import this chapter",
                                        onchange: move |e| edit_book(state, index, |b| if let Some(ch) = b.chapters.get_mut(c) { ch.include = e.checked(); }),
                                    }
                                }
                                input {
                                    class: "import-number", r#type: "text", value: "{chapter.number}", placeholder: "No.",
                                    "aria-label": "Chapter number", spellcheck: "false",
                                    oninput: {
                                        let series = series.clone();
                                        move |e| {
                                            let text = e.value();
                                            let meets = series.as_deref().and_then(|id| chapter_with_number(state, id, &text));
                                            edit_book(state, index, |b| if let Some(ch) = b.chapters.get_mut(c) {
                                                ch.number = text;
                                                // A number the series has: ask what to do with it.
                                                if ch.meets.is_none() && meets.is_some() {
                                                    ch.place = ChapterPlace::Skip;
                                                }
                                                ch.meets = meets;
                                            });
                                        }
                                    },
                                }
                                input {
                                    class: "import-name", r#type: "text", value: "{chapter.name}", "aria-label": "Chapter name",
                                    oninput: move |e| edit_book(state, index, |b| if let Some(ch) = b.chapters.get_mut(c) { ch.name = e.value(); }),
                                }
                                span { class: "muted small import-pages", "{plural(chapter.pages, \"page\")}" }
                            }
                        }
                        if series.is_none() {
                            p { class: "muted small", style: "margin: 4px 0 0",
                                "Leave a number empty to give it the next one. "
                                Help { text: "A chapter without a number takes the next free number in the series. A chapter whose number the series already has is left out, unless you choose to add its pages, replace it or keep both." }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The key of the chapter of a series that has this number, if it has one.
fn chapter_with_number(state: AppState, series_id: &str, text: &str) -> Option<String> {
    let number = text.trim().replace(',', ".").parse::<f64>().ok()?;
    state
        .library
        .peek()
        .find(series_id)?
        .chapters
        .iter()
        .find(|c| c.number.is_some_and(|n| (n - number).abs() < 0.0005))
        .map(|c| c.key.clone())
}

fn place_key(place: ChapterPlace) -> &'static str {
    match place {
        ChapterPlace::Skip => "skip",
        ChapterPlace::Add => "add",
        ChapterPlace::Replace => "replace",
        ChapterPlace::Both => "both",
    }
}

fn place_of(key: &str) -> ChapterPlace {
    match key {
        "add" => ChapterPlace::Add,
        "replace" => ChapterPlace::Replace,
        "both" => ChapterPlace::Both,
        _ => ChapterPlace::Skip,
    }
}

#[component]
fn ImportingStage(session: ImportSession) -> Element {
    let fraction = if session.total_pages == 0 {
        None
    } else {
        Some(session.done_pages as f32 / session.total_pages as f32)
    };
    rsx! {
        div { class: "import-body import-progress",
            ProgressBar { fraction }
            p { class: "muted", style: "margin: 8px 0 0",
                "{session.done_pages} of {plural(session.total_pages, \"page\")}"
            }
        }
        div { class: "actions",
            button { class: "btn", onclick: move |_| cancel_import(), "Cancel" }
        }
    }
}

#[component]
fn FinishedStage(session: ImportSession) -> Element {
    let state = use_context::<AppState>();
    let only = session.finished.first().map(|d| d.series_id.clone());
    let many = session.finished.len() > 1;
    rsx! {
        div { class: "import-body",
            for d in session.finished.iter() {
                div { class: "import-done", key: "{d.series_id}",
                    Icon { name: "circle-check", size: 16 }
                    div {
                        strong { "{d.title}" }
                        if d.chapters > 0 {
                            span { class: "muted", " · {plural(d.chapters, \"chapter\")}, {plural(d.pages, \"page\")}" }
                        }
                        if d.already > 0 {
                            div { class: "muted small",
                                "{plural(d.already, \"chapter\")} already in the series, left out."
                            }
                        }
                        if d.skipped > 0 {
                            div { class: "import-warning small",
                                "{plural(d.skipped, \"page\")} could not be read, and left out."
                            }
                        }
                    }
                }
            }
            if session.finished.is_empty() && session.errors.is_empty() {
                p { class: "muted", "There was nothing to add." }
            }
            for e in session.errors.iter() {
                div { class: "banner bad small", key: "{e}", Icon { name: "circle-alert", size: 16 } div { "{e}" } }
            }
        }
        div { class: "actions",
            button { class: "btn", onclick: move |_| close_import(state), "Done" }
            if let (Some(id), false) = (only, many) {
                button { class: "btn primary",
                    onclick: move |_| {
                        close_import(state);
                        crate::ui::work::open_series_from_library(state, &id);
                    },
                    "Open it"
                }
            } else if many {
                button { class: "btn primary",
                    onclick: move |_| { close_import(state); state.go(Page::Library); },
                    "Go to the library"
                }
            }
        }
    }
}

/// Watches a folder for files that start an import, so the dialog can be tried
/// without the system's file dialog. Only in debug builds, and only when the
/// `MANGAPOINT_IMPORT_WATCH` folder is named. A file there holds one path per
/// line; a line `series: <id>` makes it add to that series, and a line `auto`
/// imports what was found without asking. It is deleted once read.
#[cfg(debug_assertions)]
pub fn watch_for_test_imports(state: AppState) {
    let Ok(dir) = std::env::var("MANGAPOINT_IMPORT_WATCH") else {
        return;
    };
    crate::state::spawn_forever(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "txt") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let _ = std::fs::remove_file(&path);
                let mut target = None;
                let mut auto = false;
                let mut paths = Vec::new();
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if line == "auto" {
                        auto = true;
                    } else if let Some(id) = line.strip_prefix("series:") {
                        target = Some(id.trim().to_string());
                    } else {
                        paths.push(std::path::PathBuf::from(line));
                    }
                }
                if !paths.is_empty() {
                    import_paths(state, target, paths);
                    if auto {
                        // Wait for the reading to finish, then import what it found.
                        while state
                            .import
                            .peek()
                            .as_ref()
                            .is_some_and(|s| s.stage == ImportStage::Reading)
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        run_import(state);
                        while state
                            .import
                            .peek()
                            .as_ref()
                            .is_some_and(|s| s.stage != ImportStage::Finished)
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        close_import(state);
                    }
                }
            }
        }
    });
}

#[cfg(not(debug_assertions))]
pub fn watch_for_test_imports(_state: AppState) {}

/// Arc is used for covers so drafts can be made on another thread.
#[allow(dead_code)]
type Cover = Arc<Vec<u8>>;
