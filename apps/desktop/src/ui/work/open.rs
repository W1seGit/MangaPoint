//! Opening a series: search results and library entries, their chapter lists and covers.

use super::*;

pub(super) fn reset_workspace_state(state: AppState) {
    let (mut selected, mut table, mut preview) = (state.selected, state.table, state.preview);
    selected.set(HashSet::new());
    table.set(TableView::default());
    // A new series starts with an empty preview, but keeps how the user likes to view it.
    let view = preview.peek().view;
    preview.set(PreviewState {
        view,
        ..Default::default()
    });
    let mut source = state.download_source;
    source.set(SourceChoice::Adaptive);
}

/// Wait for the catalog engine if it is still starting (up to a minute and a
/// half). True if it is running.
pub(super) async fn wait_for_engine(state: AppState) -> bool {
    for _ in 0..450 {
        if state.client().is_some() {
            return true;
        }
        if !matches!(*state.boot.peek(), crate::state::BootState::Starting { .. }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    state.client().is_some()
}

/// Open a search result: show its chapters and fetch their lists.
pub fn open_series_from_search(state: AppState, ranked: RankedSeries) {
    let title = ranked.title.clone();
    let series_id = library::series_id(&title);
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: series_id.clone(),
        title,
        ranked: Some(ranked.clone()),
        origin: Page::Search,
    }));
    let mut chapters = state.chapters;
    chapters.set(ChaptersState::Loading);
    state.go(Page::Series);

    let members = logic::one_per_source(&ranked.members, 4);
    let mut open_details = state.open_details;
    open_details.set(None);
    // What the sources say about it (description, author, cover) is asked of
    // all of them at once and shown as soon as the first has it.
    {
        let sources: Vec<(i64, Option<String>)> = ranked
            .members
            .iter()
            .map(|m| (m.candidate.manga_id, m.candidate.thumbnail.clone()))
            .collect();
        let names: Vec<(String, String)> = logic::one_per_source(&ranked.members, 8)
            .iter()
            .map(|m| (m.candidate.source_id.clone(), m.candidate.source_name.clone()))
            .collect();
        let wanted = series_id.clone();
        spawn_forever(async move {
            wait_for_engine(state).await;
            let Some(client) = state.client() else { return };
            let (details, thumbnails) = details_race(&client, Vec::new(), sources).await;
            let still_here = state
                .workspace
                .peek()
                .as_ref()
                .is_some_and(|w| w.series_id == wanted);
            if let (Some(details), true) = (details, still_here) {
                let info = logic::series_info_from(&details, names, false, now_secs());
                let cover = (!thumbnails.is_empty())
                    .then(|| Rc::<str>::from(crate::covers::source_url(&wanted, &thumbnails)));
                let mut open_details = state.open_details;
                open_details.set(Some(crate::state::OpenDetails { series_id: wanted, info, cover }));
            }
        });
    }
    spawn_forever(async move {
        wait_for_engine(state).await;
        let result = load_chapters(state, members).await;
        // The user may have opened something else meanwhile.
        let still_here = state
            .workspace
            .peek()
            .as_ref()
            .is_some_and(|w| w.series_id == series_id);
        if still_here {
            if let Some(loaded) = &result {
                remember_chapter_total(state, &series_id, loaded.merged.len());
            }
            let mut chapters = state.chapters;
            chapters.set(match result {
                Some(loaded) => ChaptersState::Ready(Rc::new(loaded)),
                None => ChaptersState::Failed("The catalog engine is not running.".into()),
            });
        }
    });
}

/// Open a downloaded series from the Library, ready to preview.
pub fn open_series_from_library(state: AppState, series_id: &str) {
    let Some((title, sources)) = state
        .library
        .peek()
        .find(series_id)
        .map(|s| (s.title.clone(), s.sources.clone()))
    else {
        return;
    };
    reset_workspace_state(state);
    let mut ws = state.workspace;
    ws.set(Some(Workspace {
        series_id: series_id.to_string(),
        title,
        ranked: None,
        origin: Page::Library,
    }));
    let mut chapters = state.chapters;
    state.go(Page::Series);
    chapters.set(ChaptersState::Loading);
    let id = series_id.to_string();
    spawn_forever(async move {
        wait_for_engine(state).await;
        // Series downloaded before sources were kept find theirs again from
        // their downloaded chapters.
        let sources = if sources.is_empty() {
            backfill_sources(state, &id).await
        } else {
            sources
        };
        let result = if sources.is_empty() {
            Some(Err("none".to_string()))
        } else {
            let members = members_from_saved(&sources, &ws_title(state, &id));
            load_chapters(state, members).await.map(Ok)
        };
        let still_here = state
            .workspace
            .peek()
            .as_ref()
            .is_some_and(|w| w.series_id == id);
        if still_here {
            if let Some(Ok(loaded)) = &result {
                remember_chapter_total(state, &id, loaded.merged.len());
            }
            let mut chapters = state.chapters;
            chapters.set(match result {
                Some(Ok(loaded)) => ChaptersState::Ready(Rc::new(loaded)),
                Some(Err(_)) => ChaptersState::Idle,
                None => ChaptersState::Failed("The catalog engine is not running.".into()),
            });
        }
    });
}

/// Find where a library series lives on the sources again, by asking about the
/// chapters downloaded from each one. They are kept with the series after that.
pub(super) async fn backfill_sources(
    state: AppState,
    series_id: &str,
) -> Vec<library::SavedSource> {
    let Some(client) = state.client() else {
        return Vec::new();
    };
    let Some(series) = state.library.peek().find(series_id).cloned() else {
        return Vec::new();
    };
    let sources: HashMap<String, (String, String)> = client
        .list_sources()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.id, (s.name, s.lang)))
        .collect();
    let mut found: Vec<library::SavedSource> = Vec::new();
    for chapter in &series.chapters {
        if found.iter().any(|f| f.source_id == chapter.source_id) {
            continue;
        }
        let from_source = series
            .chapters
            .iter()
            .filter(|c| c.source_id == chapter.source_id)
            .take(3);
        for c in from_source {
            if let Ok(details) = client.manga_of_chapter(c.chapter_id).await {
                let (name, lang) = sources.get(&c.source_id).cloned().unwrap_or_default();
                found.push(library::SavedSource {
                    source_id: c.source_id.clone(),
                    source_name: name,
                    lang,
                    manga_id: details.id,
                    thumbnail: details.thumbnail_url,
                });
                break;
            }
        }
    }
    if !found.is_empty() {
        let mut library = state.library;
        library.write().add_sources(series_id, found.clone());
        state.save_library();
        info!(
            "Found the sources of \"{}\" again from its downloaded chapters",
            series.title
        );
    }
    found
}

pub(super) fn ws_title(state: AppState, series_id: &str) -> String {
    state
        .library
        .peek()
        .find(series_id)
        .map(|s| s.title.clone())
        .unwrap_or_default()
}

/// A library series' saved sources, as search results again.
pub(super) fn members_from_saved(
    sources: &[library::SavedSource],
    title: &str,
) -> Vec<ScoredMember> {
    sources
        .iter()
        .map(|s| ScoredMember {
            candidate: search::Candidate::new(
                &catalog::Source {
                    id: s.source_id.clone(),
                    name: s.source_name.clone(),
                    lang: s.lang.clone(),
                    is_nsfw: false,
                },
                &catalog::MangaSummary {
                    id: s.manga_id,
                    title: title.to_string(),
                    thumbnail_url: s.thumbnail.clone(),
                },
            ),
            score: 0.5,
            enrichment: None,
        })
        .collect()
}

/// What the library keeps of a series' sources: the best entry from each.
pub fn saved_sources_of(members: &[ScoredMember]) -> Vec<library::SavedSource> {
    logic::one_per_source(members, 8)
        .iter()
        .map(|m| library::SavedSource {
            source_id: m.candidate.source_id.clone(),
            source_name: m.candidate.source_name.clone(),
            lang: m.candidate.lang.clone(),
            manga_id: m.candidate.manga_id,
            thumbnail: m.candidate.thumbnail.clone(),
        })
        .collect()
}

/// Remember how many chapters a library series has in all, to show next to
/// how many are downloaded.
pub fn remember_chapter_total(state: AppState, series_id: &str, total: usize) {
    if total == 0 {
        return;
    }
    let changed = {
        let mut library = state.library;
        let changed = library.write().set_chapter_total(series_id, total);
        changed
    };
    if changed {
        state.save_library();
    }
}

static TOTALS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static TOTALS_TRIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Look up, quietly and one after another, how many chapters each library
/// series has, for the ones that do not know yet. Each is tried once a run.
pub fn fill_chapter_totals(state: AppState) {
    use std::sync::atomic::Ordering;
    if state.client().is_none() || TOTALS_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    spawn_forever(async move {
        loop {
            let next = {
                let tried = TOTALS_TRIED.lock().map(|t| t.clone()).unwrap_or_default();
                state
                    .library
                    .peek()
                    .series
                    .iter()
                    .find(|s| {
                        s.chapter_total.is_none()
                            && !s.sources.is_empty()
                            && !s.is_sample()
                            && !tried.contains(&s.id)
                    })
                    .map(|s| (s.id.clone(), s.title.clone(), s.sources.clone()))
            };
            let Some((id, title, sources)) = next else {
                break;
            };
            if let Ok(mut tried) = TOTALS_TRIED.lock() {
                tried.push(id.clone());
            }
            let members = members_from_saved(&sources, &title);
            if let Some(loaded) = load_chapters(state, members).await {
                remember_chapter_total(state, &id, loaded.merged.len());
            }
        }
        TOTALS_BUSY.store(false, Ordering::SeqCst);
    });
}

/// Put a series in the library without downloading anything.
pub fn add_to_library(state: AppState, title: &str, sources: Vec<library::SavedSource>) {
    let (defaults, preset) = state.new_series_defaults();
    let already = state
        .library
        .peek()
        .find(&library::series_id(title))
        .is_some();
    let id = {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_saved(title, sources, &defaults, now_secs());
        db.adopt_default_preset(&id, &defaults, &preset);
        id
    };
    state.save_library();
    ensure_series_info(state, &id);
    if already {
        state.notify(format!("{title} is already in your library."));
    } else {
        state.notify(format!("Added {title} to your library."));
    }
}

/// Add a search result to the library.
pub fn add_ranked_to_library(state: AppState, ranked: &RankedSeries) {
    add_to_library(state, &ranked.title, saved_sources_of(&ranked.members));
}

/// Add the open series to the library.
pub fn add_open_series_to_library(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let sources = match &*state.chapters.peek() {
        ChaptersState::Ready(loaded) => loaded.saved.clone(),
        _ => ws
            .ranked
            .as_ref()
            .map(|r| saved_sources_of(&r.members))
            .unwrap_or_default(),
    };
    add_to_library(state, &ws.title, sources);
    if let ChaptersState::Ready(loaded) = &*state.chapters.peek() {
        remember_chapter_total(state, &ws.series_id, loaded.merged.len());
    }
}

/// The key a series' saved cover is kept under in `state.covers`.
pub fn saved_cover_key(series_id: &str) -> String {
    format!("saved:{series_id}")
}

/// Show the cover saved with a series: its address is given to the page,
/// which asks for the picture when it is in view.
pub fn load_saved_cover(state: AppState, series_id: &str) {
    let key = saved_cover_key(series_id);
    let path = library_files().cover_path(series_id);
    let Ok(meta) = std::fs::metadata(&path) else {
        return;
    };
    // The address changes when the picture on disk does.
    let version = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
        ^ meta.len();
    let url: Rc<str> = crate::covers::saved_url(series_id, version).into();
    if state.covers.peek().get(&key) == Some(&url) {
        return;
    }
    let mut covers = state.covers;
    covers.write().insert(key, url);
}

/// Fetch and keep what the sources say about a downloaded series (its
/// description, author, genres and cover), if that has not been done. It asks
/// about a chapter the series was downloaded from, so it also works for series
/// downloaded before details were saved.
/// Series whose details could not be fetched, held off for a while before
/// the next attempt (the library page asks again at every change to the library).
static INFO_RETRY: std::sync::LazyLock<std::sync::Mutex<logic::Backoff>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(logic::Backoff::new()));

fn info_may_be_tried(series_id: &str) -> bool {
    INFO_RETRY
        .lock()
        .map_or(true, |b| b.may_try(series_id, now_secs()))
}

fn note_info_result(series_id: &str, worked: bool) {
    if let Ok(mut b) = INFO_RETRY.lock() {
        if worked {
            b.succeeded(series_id);
        } else {
            b.failed(series_id, now_secs());
        }
    }
}

pub fn ensure_series_info(state: AppState, series_id: &str) {
    let Some(series) = state.library.peek().find(series_id).cloned() else {
        return;
    };
    if series.info.is_some() || series.is_sample() || state.info_busy.peek().contains(series_id) {
        return;
    }
    let Some(client) = state.client() else {
        return;
    };
    if !info_may_be_tried(series_id) {
        return;
    }
    let id = series_id.to_string();
    let mut busy = state.info_busy;
    busy.write().insert(id.clone());
    spawn_forever(async move {
        let files = library_files();
        // Every source of the series is asked at once; the details come from
        // whichever answers first with a description, and what it leaves out
        // is filled in from the others.
        let (details, thumbnails) = details_race(
            &client,
            series.chapters.iter().map(|c| (c.source_id.clone(), c.chapter_id)).collect(),
            series.sources.iter().map(|s| (s.manga_id, s.thumbnail.clone())).collect(),
        )
        .await;
        // Taken out of the library while the sources were being asked: nothing
        // is written for it, or its folder would come back.
        if state.library.peek().find(&id).is_none() {
            let mut busy = state.info_busy;
            busy.write().remove(&id);
            return;
        }
        if let Some(details) = details {
            let names: HashMap<String, String> = client
                .list_sources()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.id, s.name))
                .collect();
            let mut source_names: Vec<(String, String)> = series
                .sources
                .iter()
                .map(|s| (s.source_id.clone(), s.source_name.clone()))
                .collect();
            for chapter in &series.chapters {
                if let Some(name) = names.get(&chapter.source_id) {
                    if !source_names.iter().any(|(id, _)| *id == chapter.source_id) {
                        source_names.push((chapter.source_id.clone(), name.clone()));
                    }
                }
            }
            let mut has_cover = false;
            // The cover is the first picture to arrive from any of the sources.
            if let Some(bytes) = crate::covers::first_picture(&client, thumbnails).await {
                let small = tokio::task::spawn_blocking(move || logic::small_cover(&bytes))
                    .await
                    .ok()
                    .flatten();
                // The series may have been removed while the picture came in.
                let still_here = state.library.peek().find(&id).is_some();
                if let Some(small) = small.filter(|_| still_here) {
                    let target = files.cover_path(&id);
                    if let Some(dir) = target.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    has_cover = std::fs::write(&target, &small).is_ok();
                }
            }
            let info = logic::series_info_from(&details, source_names, has_cover, now_secs());
            let mut library = state.library;
            library.write().set_info(&id, info);
            state.save_library();
            note_info_result(&id, true);
            info!("Saved details for \"{}\"", series.title);
            if has_cover {
                load_saved_cover(state, &id);
            }
        } else {
            note_info_result(&id, false);
            warn!("Could not fetch details for \"{}\"", series.title);
        }
        let mut busy = state.info_busy;
        busy.write().remove(&id);
    });
}

/// What the sources say about a series, from all of them at once: the first
/// answer that has a description is the one used, and anything it lacks (an
/// author, the genres) is taken from the others that came in by then. Also the
/// cover addresses to try, the best first. Nothing when no source answers.
pub(super) async fn details_race(
    client: &Arc<catalog::SuwayomiClient>,
    chapters: Vec<(String, i64)>,
    sources: Vec<(i64, Option<String>)>,
) -> (Option<catalog::MangaDetails>, Vec<String>) {
    use futures_util::stream::FuturesUnordered;
    use futures_util::FutureExt;
    type Asked = futures_util::future::BoxFuture<'static, Option<catalog::MangaDetails>>;
    let mut asking: FuturesUnordered<Asked> = FuturesUnordered::new();
    // A chapter downloaded from each source (series downloaded before their
    // sources were kept can only be found this way).
    let mut seen = HashSet::new();
    for (source_id, chapter_id) in chapters {
        if !seen.insert(source_id) || seen.len() > 3 {
            continue;
        }
        let client = client.clone();
        asking.push(async move { client.manga_of_chapter(chapter_id).await.ok() }.boxed());
    }
    // Where it was found on each source.
    for (manga_id, _) in sources.iter().take(6) {
        let (client, id) = (client.clone(), *manga_id);
        asking.push(async move { client.manga_details(id).await.ok() }.boxed());
    }
    let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
    let mut merged: Option<catalog::MangaDetails> = None;
    let mut thumbnails: Vec<String> = Vec::new();
    let gather = async {
        while let Some(found) = asking.next().await {
            let Some(d) = found else { continue };
            if let Some(path) = &d.thumbnail_url {
                if !thumbnails.contains(path) {
                    thumbnails.push(path.clone());
                }
            }
            match merged.as_mut() {
                None => merged = Some(d),
                Some(m) => {
                    if blank(&m.description) {
                        m.description = d.description;
                    }
                    if blank(&m.author) {
                        m.author = d.author;
                    }
                    if blank(&m.artist) {
                        m.artist = d.artist;
                    }
                    if m.genre.is_empty() {
                        m.genre = d.genre;
                    }
                    if blank(&m.status) {
                        m.status = d.status;
                    }
                }
            }
            if merged.as_ref().is_some_and(|m| !blank(&m.description)) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(20), gather).await;
    for (_, thumbnail) in &sources {
        if let Some(path) = thumbnail {
            if !thumbnails.contains(path) {
                thumbnails.push(path.clone());
            }
        }
    }
    (merged, thumbnails)
}

pub(super) async fn load_chapters(
    state: AppState,
    members: Vec<ScoredMember>,
) -> Option<LoadedChapters> {
    let client = state.client()?;
    let names: Vec<&str> = members
        .iter()
        .map(|m| m.candidate.source_name.as_str())
        .collect();
    info!("Loading chapter lists from {}", names.join(", "));
    let started = Instant::now();
    // At most six at a time, within the app-wide limit on requests to the engine.
    let ids: Vec<i64> = members.iter().map(|m| m.candidate.manga_id).collect();
    let mut lists = Vec::new();
    let mut unreachable = 0;
    for result in search::fanout::fetch_chapter_lists(&*client, &ids, Duration::from_secs(60), 6).await {
        match result {
            Some(chapters) => lists.push(chapters),
            None => {
                unreachable += 1;
                lists.push(Vec::new());
            }
        }
    }
    let merged = merge_chapters(&lists);
    let both = merged.iter().filter(|m| m.options.len() > 1).count();
    info!(
        "{} chapters in total ({both} available from more than one source) in {:.1}s{}",
        merged.len(),
        started.elapsed().as_secs_f32(),
        if unreachable > 0 {
            format!("; {unreachable} source(s) did not answer")
        } else {
            String::new()
        }
    );
    Some(LoadedChapters {
        merged,
        member_sources: members
            .iter()
            .map(|m| m.candidate.source_id.clone())
            .collect(),
        member_names: members
            .iter()
            .map(|m| m.candidate.source_name.clone())
            .collect(),
        saved: members
            .iter()
            .map(|m| library::SavedSource {
                source_id: m.candidate.source_id.clone(),
                source_name: m.candidate.source_name.clone(),
                lang: m.candidate.lang.clone(),
                manga_id: m.candidate.manga_id,
                thumbnail: m.candidate.thumbnail.clone(),
            })
            .collect(),
        unreachable,
    })
}

/// Keys of the chapters of this series that are already downloaded.
pub fn downloaded_keys(state: AppState, series_id: &str) -> HashSet<String> {
    state
        .library
        .read()
        .find(series_id)
        .map(|s| s.chapters.iter().map(|c| c.key.clone()).collect())
        .unwrap_or_default()
}

/// The library key of a merged chapter (named after its best source's copy).
pub fn merged_key(ch: &search::MergedChapter) -> String {
    chapter_key(ch.number, ch.options.first().map_or(0, |o| o.chapter_id))
}
