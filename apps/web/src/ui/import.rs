//! The import dialog: what the browser found in the picked files, one card per
//! book to check and edit, then keeping the books on this device.

use super::widgets::{Busy, Chip, Icon, ProgressBar};
use crate::app::{Ctx, ImportStage, ImportState};
use crate::bridge;
use crate::format::{human_bytes, plural};
use crate::model::Book;
use crate::review::{Draft, Failure, Review};
use dioxus::prelude::*;
use serde_json::json;

/// The browser has looked at what was picked.
pub fn analysis_done(ctx: Ctx, drafts: Vec<Draft>, failures: Vec<Failure>, into: Option<String>) {
    // Chapters picked for a book: the ones it has already start unticked.
    let existing = into.as_deref().and_then(|id| ctx.book(id)).map(|b| b.chapters).unwrap_or_default();
    let mut imp = ctx.import;
    let mut i = imp.write();
    if into.is_some() {
        i.into = into;
    }
    i.reviews.extend(drafts.into_iter().map(|d| {
        let mut r = Review::new(d);
        if !existing.is_empty() {
            r.leave_out_duplicates(&existing);
        }
        r
    }));
    i.failures.extend(failures);
    i.stage = ImportStage::Review;
}

pub fn close_import(ctx: Ctx) {
    let mut imp = ctx.import;
    let ids: Vec<String> = imp.peek().reviews.iter().map(|r| r.draft.draft_id.clone()).collect();
    *imp.write() = ImportState::default();
    dioxus::core::spawn_forever(async move {
        for id in ids {
            let _ = bridge::call_ok("discard", json!({ "draft_id": id })).await;
        }
    });
}

fn remove_review(ctx: Ctx, index: usize) {
    let mut imp = ctx.import;
    let id = {
        let mut i = imp.write();
        if index >= i.reviews.len() {
            return;
        }
        i.reviews.remove(index).draft.draft_id
    };
    dioxus::core::spawn_forever(async move {
        let _ = bridge::call_ok("discard", json!({ "draft_id": id })).await;
    });
}

fn edit(ctx: Ctx, index: usize, f: impl FnOnce(&mut Review)) {
    let mut imp = ctx.import;
    let mut guard = imp.write();
    if let Some(r) = guard.reviews.get_mut(index) {
        f(r);
    }
}

/// A book that was committed is now in the library: add it, tell the user when their place in it
/// came back, and keep its state. True when the browser would not keep the file.
pub fn kept(ctx: Ctx, book: Book) -> bool {
    let stored_none = book.stored == "none";
    let title = book.title.clone();
    let id = book.id.clone();
    let mut lib = ctx.library;
    let waiting = lib.write().add_book(book);
    if let Some(w) = waiting {
        if w.last_read_at > 0 {
            ctx.notify(format!("{title} is back. You were at {}, page {}.", w.position.chapter_name, w.position.page + 1));
        }
    }
    // Chapters edited, so the state's chapter count must follow.
    if let Some(b) = ctx.book(&id) {
        let mut lib = ctx.library;
        let mut guard = lib.write();
        if let Some(s) = guard.states.get_mut(&b.id) {
            s.chapters = b.chapters.len() as u32;
            s.title = b.title.clone();
        }
        drop(guard);
        ctx.save_state_of(&b.id);
    }
    stored_none
}

/// Add the reviewed chapters to the book they were picked for.
fn add_all_to(ctx: Ctx, book_id: String) {
    dioxus::core::spawn_forever(async move {
        let reviews: Vec<Review> = ctx.import.peek().reviews.clone();
        let mut added = 0usize;
        let mut title = String::new();
        for review in reviews {
            if review.problem().is_some() {
                continue;
            }
            let mut imp = ctx.import;
            imp.write().stage = ImportStage::Keeping { title: review.draft.file_name.clone(), done: 0, total: 1 };
            let chapters = review.final_chapters();
            let n = chapters.len();
            let args = json!({ "book": book_id, "draft_id": review.draft.draft_id, "chapters": chapters });
            match bridge::call::<Book>("append", args).await {
                Ok(book) => {
                    title = book.title.clone();
                    let mut lib = ctx.library;
                    lib.write().add_book(book);
                    added += n;
                }
                Err(e) => ctx.error(format!("Could not add the chapters of {}: {e}", review.draft.file_name)),
            }
        }
        let mut imp = ctx.import;
        *imp.write() = ImportState::default();
        if added > 0 {
            ctx.notify(format!("Added {} to {title}.", plural(added, "chapter")));
        }
    });
}

/// Keep every reviewed book on this device.
fn keep_all(ctx: Ctx) {
    if let Some(book) = ctx.import.peek().into.clone() {
        add_all_to(ctx, book);
        return;
    }
    dioxus::core::spawn_forever(async move {
        let reviews: Vec<Review> = ctx.import.peek().reviews.clone();
        let mut added = 0usize;
        let mut lost = Vec::new();
        for review in reviews {
            if review.problem().is_some() {
                continue;
            }
            let mut imp = ctx.import;
            imp.write().stage = ImportStage::Keeping { title: review.title.clone(), done: 0, total: 1 };
            let chapters = review.final_chapters();
            let args = json!({
                "draft_id": review.draft.draft_id,
                "title": review.title.trim(),
                "chapters": chapters,
            });
            match bridge::call::<Book>("commit", args).await {
                Ok(book) => {
                    let title = book.title.clone();
                    if kept(ctx, book) {
                        lost.push(title);
                    }
                    added += 1;
                }
                Err(e) => ctx.error(format!("Could not keep {}: {e}", review.title)),
            }
        }
        let mut imp = ctx.import;
        *imp.write() = ImportState::default();
        if added > 0 {
            ctx.notify(if added == 1 { "Added to your library.".to_string() } else { format!("Added {added} books to your library.") });
            ctx.go(crate::app::Page::Library);
        }
        if !lost.is_empty() {
            ctx.warn(format!(
                "This browser would not keep the file of {}. You can read it until you close the page; import it again next time and your place is remembered.",
                lost.join(", ")
            ));
        }
    });
}

#[component]
pub fn ImportHost() -> Element {
    let ctx = use_context::<Ctx>();
    let state = ctx.import.read().clone();
    if state.stage == ImportStage::Idle {
        return rsx! {};
    }
    let busy = matches!(state.stage, ImportStage::Analyzing(_) | ImportStage::Keeping { .. });
    rsx! {
        div { class: "overlay", id: "import-dialog", onclick: move |_| { if !busy { close_import(ctx); } },
            div { class: "modal import", role: "dialog", "aria-label": "Import", onclick: move |e| e.stop_propagation(),
                div { class: "modal-head",
                    h2 { "Import" }
                    if !matches!(state.stage, ImportStage::Keeping { .. }) {
                        button { class: "btn ghost small", id: "import-close", title: "Close", "aria-label": "Close", onclick: move |_| close_import(ctx),
                            Icon { name: "x", size: 16 }
                        }
                    }
                }
                {match state.stage.clone() {
                    ImportStage::Analyzing(status) => rsx! {
                        div { class: "import-wait", id: "import-wait", Busy { label: "{status}" } }
                    },
                    ImportStage::Review => rsx! { ReviewStage { state: state.clone() } },
                    ImportStage::Keeping { title, done, total } => {
                        let fraction = if total > 1 { Some(done as f32 / total as f32) } else { None };
                        rsx! {
                            div { class: "import-body import-progress", id: "import-keeping",
                                p { style: "margin: 0 0 8px", "Keeping {title} on this device\u{2026}" }
                                ProgressBar { fraction }
                            }
                        }
                    }
                    ImportStage::Idle => rsx! {},
                }}
            }
        }
    }
}

#[component]
fn ReviewStage(state: ImportState) -> Element {
    let ctx = use_context::<Ctx>();
    let ready: Vec<&Review> = state.reviews.iter().filter(|r| r.problem().is_none()).collect();
    let pages: usize = ready.iter().map(|r| r.included_pages()).sum();
    let into = state.into.as_deref().and_then(|id| ctx.book(id));
    let label = match (&into, ready.len()) {
        (Some(book), _) => format!("Add {} to {}", plural(ready.iter().map(|r| r.included_count()).sum(), "chapter"), book.title),
        (None, 0) => "Add".to_string(),
        (None, 1) => format!("Add to library ({})", plural(pages, "page")),
        (None, n) => format!("Add {n} books ({})", plural(pages, "page")),
    };
    let more_into = state.into.clone();
    rsx! {
        div { class: "import-body",
            if !state.failures.is_empty() {
                div { class: "banner bad small import-failures", id: "import-failures",
                    Icon { name: "circle-alert", size: 16 }
                    div {
                        for f in state.failures.iter() {
                            div { key: "{f.name}{f.message}", strong { "{f.name}" } ": {f.message}" }
                        }
                    }
                }
            }
            for (i, review) in state.reviews.iter().enumerate() {
                BookCard { key: "{review.draft.draft_id}", index: i, review: review.clone() }
            }
            if state.reviews.is_empty() {
                p { class: "muted", "Nothing could be imported from what you chose." }
                p { class: "muted small", "MangaPoint on the web opens CBZ and ZIP files, EPUBs with pictures, folders of pictures and loose pictures. RAR, 7z and PDF are not supported on the web yet." }
            }
        }
        div { class: "actions",
            button { class: "btn ghost", id: "import-cancel", onclick: move |_| close_import(ctx), if state.reviews.is_empty() { "Close" } else { "Cancel" } }
            if !state.reviews.is_empty() {
                button { class: "btn", title: "Add more books and pictures to this list",
                    onclick: move |_| match &more_into {
                        Some(id) => bridge::send("pick_more", json!({ "book": id })),
                        None => super::library::pick_files(),
                    },
                    Icon { name: "file-up", size: 16 } "More files"
                }
                button { class: "btn primary", id: "import-add", disabled: ready.is_empty(), onclick: move |_| keep_all(ctx), "{label}" }
            }
        }
    }
}

#[component]
fn BookCard(index: usize, review: Review) -> Element {
    let ctx = use_context::<Ctx>();
    let mut leaving = use_signal(|| false);
    let class = if leaving() { "import-book open leaving" } else { "import-book open" };
    let d = &review.draft;
    let counts = format!(
        "{} \u{b7} {} \u{b7} {}",
        plural(review.included_count(), "chapter"),
        plural(review.included_pages(), "page"),
        human_bytes(d.size)
    );
    let kind = match d.kind.as_str() {
        "epub" => "EPUB",
        "images" => "Pictures",
        _ => "CBZ/ZIP",
    };
    let replaced = d.existing.as_ref().is_some_and(|e| e.stored != "none");
    let revived = d.existing.as_ref().is_some_and(|e| e.stored == "none");
    let problem = review.problem();
    // Chapters for a book already in the library: they take its name, and the ones it has are marked.
    let into = ctx.import.read().into.clone().and_then(|id| ctx.book(&id));
    let dup_note = into.as_ref().filter(|_| review.duplicates > 0).map(|book| {
        let one = review.duplicates == 1;
        format!(
            "{} of this file {} already in {}, so {} left unticked. Tick {} to add {} again.",
            plural(review.duplicates, "chapter"),
            if one { "is" } else { "are" },
            book.title,
            if one { "it was" } else { "they were" },
            if one { "it" } else { "them" },
            if one { "it" } else { "them" },
        )
    });
    rsx! {
        div { class: "{class}", "data-draft": "{d.draft_id}",
            div { class: "import-bar",
                if let Some(book) = &into {
                    strong { class: "import-title import-into", "Adding to {book.title}" }
                } else {
                    input {
                        class: "import-title", r#type: "text", value: "{review.title}", spellcheck: "false",
                        "aria-label": "Title", placeholder: "Title",
                        oninput: move |e| edit(ctx, index, |r| r.title = e.value()),
                    }
                }
                span { class: "import-file muted small", title: "{d.file_name}", "{d.file_name}" }
                Chip { text: kind.to_string() }
                button {
                    class: "btn ghost small import-remove", r#type: "button", title: "Leave this out", "aria-label": "Leave {d.file_name} out",
                    onclick: move |_| {
                        leaving.set(true);
                        dioxus::core::spawn_forever(async move {
                            bridge::sleep(220).await;
                            remove_review(ctx, index);
                        });
                    },
                    Icon { name: "x", size: 15 }
                }
            }
            div { class: "import-book-body",
                div { class: "import-cover",
                    if let Some(url) = &d.cover_url { img { src: "{url}", alt: "Cover of {review.title}" } } else { super::widgets::NoCover {} }
                }
                div { class: "import-main",
                    p { class: "muted small import-count", "{counts}" }
                    if let Some(note) = &dup_note {
                        p { class: "import-warning small import-duplicates", Icon { name: "triangle-alert", size: 13 } " {note}" }
                    }
                    if replaced && into.is_none() {
                        p { class: "import-warning small", Icon { name: "info", size: 13 } " This book is already in your library. Adding it again replaces the copy; your place in it is kept." }
                    }
                    if revived {
                        p { class: "import-warning small", Icon { name: "info", size: 13 } " You have been reading this book here before. It resumes where you stopped." }
                    }
                    for w in d.warnings.iter() {
                        p { class: "import-warning small", key: "{w}", Icon { name: "triangle-alert", size: 13 } " {w}" }
                    }
                    if let Some(p) = problem {
                        p { class: "import-warning small import-problem", Icon { name: "circle-alert", size: 13 } " {p}" }
                    }
                    div { class: "import-chapters",
                        for (c, chapter) in review.chapters.iter().enumerate() {
                            div { class: if chapter.included { "import-chapter" } else { "import-chapter off" }, key: "{c}",
                                input {
                                    r#type: "checkbox", checked: chapter.included, title: "Keep this chapter", "aria-label": "Keep chapter {c + 1}",
                                    onchange: move |_| edit(ctx, index, |r| r.toggle(c)),
                                }
                                input {
                                    class: "import-number", r#type: "text", inputmode: "decimal", value: "{chapter.number}", placeholder: "No.",
                                    "aria-label": "Chapter number", spellcheck: "false",
                                    "aria-invalid": if review.number_is_valid(c) { "false" } else { "true" },
                                    oninput: move |e| edit(ctx, index, |r| r.set_number(c, &e.value())),
                                }
                                input {
                                    class: "import-name", r#type: "text", value: "{chapter.name}", "aria-label": "Chapter name",
                                    oninput: move |e| edit(ctx, index, |r| r.set_name(c, &e.value())),
                                }
                                span { class: "muted small import-pages", "{plural(chapter.count, \"page\")}" }
                            }
                        }
                    }
                    if review.chapters.len() > 1 {
                        p { class: "muted small", style: "margin: 6px 0 0",
                            "Untick a chapter to leave it out. Leave a number empty if the chapter has none."
                        }
                    }
                }
            }
        }
    }
}
