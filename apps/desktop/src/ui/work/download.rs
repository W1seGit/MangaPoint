//! Downloading: the queue of batches and the plan that feeds it.

use super::*;

/// Is an export running? (Downloads have their own queue.)
pub(super) fn job_running(state: AppState) -> bool {
    state
        .job
        .peek()
        .as_ref()
        .is_some_and(|j| j.result.is_none())
}

/// What queueing some chapters did.
pub(super) struct Enqueued {
    /// Chapters newly in the queue.
    chapters: usize,
    /// Chapters the one chosen source does not have.
    not_offered: usize,
}

/// Put the picked chapters (positions in `loaded.merged`) that are not
/// downloaded yet into the download queue, and the series in the library.
pub(super) fn enqueue_picked(
    state: AppState,
    series_id: &str,
    title: &str,
    loaded: &LoadedChapters,
    mut picked: Vec<usize>,
    choice: SourceChoice,
) -> Result<Enqueued, String> {
    let have = downloaded_keys(state, series_id);
    picked.sort_unstable();
    let mut plan = Vec::new();
    let mut requests = Vec::new();
    for i in picked {
        let Some(ch) = loaded.merged.get(i) else {
            continue;
        };
        let key = merged_key(ch);
        if have.contains(&key) {
            continue;
        }
        requests.extend(requests_from_merged(
            std::slice::from_ref(ch),
            &loaded.member_sources,
        ));
        plan.push(key);
    }
    // A single chosen source: leave out what it does not have, and say so.
    let source_names: HashMap<String, String> = loaded
        .member_sources
        .iter()
        .cloned()
        .zip(loaded.member_names.iter().cloned())
        .collect();
    let mut not_offered = 0;
    if let SourceChoice::Only(source) = &choice {
        let chosen = pipeline::apply_choice(std::mem::take(&mut requests), &choice, &[]);
        not_offered = chosen.unavailable.len();
        plan = chosen.kept.iter().map(|&i| plan[i].clone()).collect();
        requests = chosen.requests;
        if requests.is_empty() {
            let name = source_names.get(source).cloned().unwrap_or_default();
            return Err(format!(
                "{name} does not have the chapters you picked. Choose Adaptive, or another source."
            ));
        }
    }
    if requests.is_empty() {
        return Ok(Enqueued {
            chapters: 0,
            not_offered,
        });
    }
    // The series is in the library from now on, with where to find it again.
    add_to_library_quietly(state, title, &loaded.saved);
    let batch = Batch::new(
        series_id.to_string(),
        title.to_string(),
        requests,
        plan,
        choice,
        source_names,
        now_secs(),
    );
    let added = state.edit_queue(|q| q.add(batch));
    info!(
        "Queued {} of \"{title}\"",
        logic::plural(added.chapters, "chapter")
    );
    Ok(Enqueued {
        chapters: added.chapters,
        not_offered,
    })
}

/// Start a download, after asking where its files should go (unless the user
/// asked not to be asked).
pub fn ask_download(state: AppState, pending: PendingDownload, summary: String) {
    if !state.settings.peek().ask_download_folder {
        start_pending(state, pending);
        return;
    }
    let mut dialog = state.download_dialog;
    dialog.set(Some(DownloadDialog { pending, summary }));
}

/// Begin what the dialog was asked about.
pub fn start_pending(state: AppState, pending: PendingDownload) {
    match pending {
        PendingDownload::Chapters => queue_download(state),
        PendingDownload::Plan => commit_plan(state),
    }
}

/// Keep new downloads in this folder. What was downloaded before stays where
/// it is, and is still found there.
pub fn set_download_folder(state: AppState, dir: std::path::PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        state.error(format!("Could not use that folder: {e}"));
        return;
    }
    let mut settings = state.settings;
    {
        let mut st = settings.write();
        let app_folder = paths().library_dir();
        let old = st.library_dir.clone().unwrap_or_else(|| app_folder.clone());
        if old != dir && !st.library_dirs.contains(&old) {
            st.library_dirs.push(old);
        }
        st.library_dir = (dir != app_folder).then_some(dir);
    }
    apply_library_dirs(&settings.peek());
    state.save_settings();
}

/// Add the ticked chapters that are not downloaded yet to the download queue.
/// They download in turn, so there is nothing to wait for or start by hand:
/// keep picking chapters, here or in other series.
pub fn queue_download(state: AppState) {
    let Some(ws) = state.workspace.peek().clone() else {
        return;
    };
    let ChaptersState::Ready(loaded) = state.chapters.peek().clone() else {
        state.error("The chapter list is still loading.");
        return;
    };
    let picked: Vec<usize> = state.selected.peek().iter().copied().collect();
    let choice = state.download_source.peek().clone();
    match enqueue_picked(state, &ws.series_id, &ws.title, &loaded, picked, choice) {
        Err(message) => state.error(message),
        Ok(done) => {
            let mut selected = state.selected;
            selected.set(HashSet::new());
            if done.chapters == 0 {
                state.notify("Those chapters are already downloaded or in the queue.");
                return;
            }
            let skipped = if done.not_offered > 0 {
                format!(", {} not offered by that source", done.not_offered)
            } else {
                String::new()
            };
            state.notify(format!(
                "Added {} of {} to the downloads{skipped}.",
                logic::plural(done.chapters, "chapter"),
                ws.title
            ));
            ensure_queue_running(state);
        }
    }
}

/// Open the download planner for these library series.
pub fn open_plan(state: AppState, series_ids: Vec<String>) {
    let entries: Vec<PlanEntry> = series_ids
        .iter()
        .filter_map(|id| state.library.peek().find(id).cloned())
        .map(|s| PlanEntry {
            series_id: s.id,
            title: s.title,
            chapters: PlanChapters::Loading,
            picked: HashSet::new(),
            open: series_ids.len() == 1,
            filter: String::new(),
            sort: (logic::SortKey::Number, true),
        })
        .collect();
    let ids: Vec<String> = entries.iter().map(|e| e.series_id.clone()).collect();
    let mut plan = state.plan;
    plan.set(entries);
    state.go(Page::Plan);
    for id in ids {
        load_plan_entry(state, id);
    }
}

pub(super) fn load_plan_entry(state: AppState, id: String) {
    spawn_forever(async move {
        let mut sources = state
            .library
            .peek()
            .find(&id)
            .map(|s| s.sources.clone())
            .unwrap_or_default();
        if sources.is_empty() {
            sources = backfill_sources(state, &id).await;
        }
        let title = ws_title(state, &id);
        let loaded = if sources.is_empty() {
            Err("No source for this series could be found. Search for it once and add it to your library again.".to_string())
        } else {
            let members = members_from_saved(&sources, &title);
            load_chapters(state, members)
                .await
                .ok_or_else(|| "The catalog engine is not running.".to_string())
        };
        let have = downloaded_keys(state, &id);
        let queue = state.queue.peek().clone();
        let mut plan = state.plan;
        let mut entries = plan.write();
        let Some(entry) = entries.iter_mut().find(|e| e.series_id == id) else {
            return;
        };
        match loaded {
            Ok(loaded) => {
                // A series that already has chapters starts with the missing
                // ones ticked; a new one waits for the user to choose.
                if !have.is_empty() {
                    entry.picked = loaded
                        .merged
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| {
                            let key = merged_key(c);
                            !have.contains(&key) && queue.chapter_state(&id, &key).is_none()
                        })
                        .map(|(i, _)| i)
                        .collect();
                }
                entry.chapters = PlanChapters::Ready(Rc::new(loaded));
            }
            Err(message) => entry.chapters = PlanChapters::Failed(message),
        }
    });
}

/// Queue everything ticked in the planner, and show the downloads.
pub fn commit_plan(state: AppState) {
    let entries = state.plan.peek().clone();
    let (mut chapters, mut series, mut not_offered) = (0, 0, 0);
    for entry in entries {
        let PlanChapters::Ready(loaded) = &entry.chapters else {
            continue;
        };
        if entry.picked.is_empty() {
            continue;
        }
        let picked: Vec<usize> = entry.picked.iter().copied().collect();
        match enqueue_picked(
            state,
            &entry.series_id,
            &entry.title,
            loaded,
            picked,
            SourceChoice::Adaptive,
        ) {
            Ok(done) if done.chapters > 0 => {
                chapters += done.chapters;
                not_offered += done.not_offered;
                series += 1;
            }
            Ok(_) => {}
            Err(message) => state.error(message),
        }
    }
    let mut plan = state.plan;
    plan.set(Vec::new());
    if chapters == 0 {
        state.notify("Nothing new to download.");
        state.go(Page::Library);
        return;
    }
    let skipped = if not_offered > 0 {
        format!(" ({not_offered} not offered by that source)")
    } else {
        String::new()
    };
    state.notify(format!(
        "Added {} from {series} series to the downloads{skipped}.",
        logic::plural(chapters, "chapter"),
    ));
    state.go(Page::Tasks);
    ensure_queue_running(state);
}

/// Put a series in the library as a download is queued, without a message.
pub(super) fn add_to_library_quietly(
    state: AppState,
    title: &str,
    sources: &[library::SavedSource],
) {
    let (defaults, preset) = state.new_series_defaults();
    let id = {
        let mut library = state.library;
        let mut db = library.write();
        let id = db.add_saved(title, sources.to_vec(), &defaults, now_secs());
        db.adopt_default_preset(&id, &defaults, &preset);
        id
    };
    state.save_library();
    ensure_series_info(state, &id);
}

/// Start working through the download queue, unless that is already going on.
pub fn ensure_queue_running(state: AppState) {
    {
        let queue = state.queue.peek();
        if queue.runner_active || queue.next_to_start().is_none() {
            return;
        }
    }
    state.edit_queue(|q| q.runner_active = true);
    spawn_forever(async move {
        loop {
            let next = state.queue.peek().next_to_start();
            let Some(id) = next else { break };
            run_batch(state, id).await;
        }
        state.edit_queue(|q| q.runner_active = false);
        // Something may have been added just as the last check was made.
        ensure_queue_running(state);
    });
}

/// Download one batch of the queue.
pub(super) async fn run_batch(state: AppState, id: u64) {
    let Some(batch) = state.queue.peek().get(id).cloned() else {
        return;
    };
    state.edit_queue(|q| q.start(id));
    let fail = |status: BatchStatus, summary: String, failed: Vec<String>| {
        state.edit_queue(|q| q.finish(id, status, summary, Vec::new(), failed));
    };
    let Some(client) = state.client() else {
        fail(
            BatchStatus::Failed("The catalog engine is not running.".into()),
            "The catalog engine is not running.".into(),
            batch.plan.clone(),
        );
        return;
    };
    let started = Instant::now();
    let files = library_files();
    let (defaults, default_preset) = state.new_series_defaults();
    let quality = state.settings.peek().page_quality;
    let policy = RetryPolicy::default();
    let limits = state.settings.peek().download.limits();
    info!(
        "Download started: \"{}\", {}{}",
        batch.title,
        logic::plural(batch.requests.len(), "chapter"),
        match quality {
            library::PageQuality::Ultra => " (ultra save-space pages)",
            library::PageQuality::Save => " (saving space)",
            library::PageQuality::Full => "",
        }
    );
    // Adaptive: test the sources that cover about the same, then put the best
    // first for every chapter (the others stay as fallbacks).
    // The test can take a while on a slow source, so Cancel and Pause are
    // looked at while it runs, not only afterwards.
    let ordered = if batch.choice == SourceChoice::Adaptive {
        logic::unless_stale(
            order_sources(
                state,
                id,
                &client,
                batch.requests.clone(),
                &batch.source_names,
            ),
            || batch.cancel.is_cancelled(),
            Duration::from_millis(100),
        )
        .await
    } else {
        Some(batch.requests.clone())
    };
    let result = match ordered {
        None => {
            state.edit_queue_quiet(|q| {
                if let Some(b) = q.get_mut(id) {
                    b.progress.note = None;
                }
            });
            Err(PipelineError::Cancelled)
        }
        Some(requests) => {
        let (tx, mut rx) = unbounded_channel();
        let run = download_chapters(
            &*client,
            &files,
            &batch.series_id,
            &requests,
            quality,
            limits,
            &policy,
            Some(tx),
            &batch.cancel,
        );
        // Was the series in the library when this started (or has this download
        // recorded a chapter of it)? Then its absence later means it was removed.
        let known = std::cell::Cell::new(state.library.peek().find(&batch.series_id).is_some());
        let watch = async {
            let mut logged = HashMap::new();
            while let Some(first) = rx.recv().await {
                // Take everything that has arrived and show it in one go:
                // redrawing for every single page would keep the screen busy
                // far more than anyone can see.
                let mut events = vec![first];
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
                for event in &events {
                    log_event(event, &mut logged);
                    if let JobEvent::ChapterSaved { chapter } = event {
                        // The series was removed while this was still downloading:
                        // recording would bring it back, so stop instead.
                        let exists = state.library.peek().find(&batch.series_id).is_some();
                        if !logic::may_record_chapter(known.get(), exists) {
                            state.edit_queue(|q| {
                                q.cancel(id);
                            });
                            continue;
                        }
                        known.set(true);
                        // Record each chapter as soon as it is safe on disk, so
                        // cancelling or failing later never loses it.
                        let mut library = state.library;
                        {
                            let mut db = library.write();
                            let series =
                                db.record_chapter(&batch.title, &defaults, chapter.clone(), now_secs());
                            db.adopt_default_preset(&series, &defaults, &default_preset);
                        }
                        state.save_library();
                    }
                }
                // How far it has got is not kept on disk, so nothing is saved here.
                state.edit_queue_quiet(|q| {
                    if let Some(b) = q.get_mut(id) {
                        for event in &events {
                            b.progress.apply(event);
                        }
                    }
                });
                tokio::time::sleep(PROGRESS_REFRESH).await;
            }
        };
        let (result, ()) = join(run, watch).await;
        result
        }
    };

    // Chapters of the batch that did not end up saved.
    let unsaved = |state: AppState| -> Vec<String> {
        let have = downloaded_keys(state, &batch.series_id);
        batch
            .plan
            .iter()
            .filter(|k| !have.contains(*k))
            .cloned()
            .collect()
    };
    match result {
        Ok(outcome) => {
            {
                let mut health = state.health;
                let mut h = health.write();
                for u in &outcome.source_use {
                    h.record_use(&u.source_id, u.ok, u.ms);
                }
            }
            state.save_health();
            let bytes: u64 = outcome.chapters.iter().map(|c| c.bytes).sum();
            let skipped: Vec<String> = outcome
                .skipped
                .iter()
                .map(|s| format!("{}: {}", s.name, s.reason))
                .collect();
            let summary = format!(
                "Downloaded {}, {}{}",
                logic::plural(outcome.chapters.len(), "chapter"),
                logic::human_bytes(bytes),
                if skipped.is_empty() {
                    String::new()
                } else {
                    format!("; {} could not be downloaded", skipped.len())
                }
            );
            info!("{summary} in {:.0}s", started.elapsed().as_secs_f32());
            // `skipped.index` counts within the chapters we asked for.
            let failed: Vec<String> = outcome
                .skipped
                .iter()
                .filter_map(|s| batch.plan.get(s.index).cloned())
                .collect();
            let pages: usize = outcome.chapters.iter().map(|c| c.pages).sum();
            state.edit_queue(|q| {
                q.finish(id, BatchStatus::Done, summary.clone(), skipped, failed);
                q.set_stats(
                    id,
                    crate::queue::BatchStats {
                        secs: started.elapsed().as_secs(),
                        pages,
                    },
                );
            });
            state.notify(format!("{}: {}", batch.title, summary.to_lowercase()));
            crate::notify::finished(state, "Download finished", &format!("{}: {summary}", batch.title));
        }
        Err(e) => {
            let cancelled = matches!(e, PipelineError::Cancelled);
            if cancelled && state.queue.peek().get(id).is_some_and(|b| b.pausing) {
                let have = downloaded_keys(state, &batch.series_id);
                state.edit_queue(|q| q.park(id, &have));
                info!("Download of \"{}\" paused", batch.title);
                ensure_series_info(state, &batch.series_id);
                return;
            }
            let message = if cancelled {
                "Cancelled. Chapters that finished are kept.".to_string()
            } else {
                e.to_string()
            };
            if cancelled {
                info!("Download of \"{}\" cancelled", batch.title);
            } else {
                warn!("Download of \"{}\" failed: {message}", batch.title);
            }
            let status = if cancelled {
                BatchStatus::Cancelled
            } else {
                BatchStatus::Failed(message.clone())
            };
            let stopped_on_purpose = cancelled;
            fail(status, message.clone(), unsaved(state));
            if !stopped_on_purpose {
                crate::notify::finished(state, "Download failed", &format!("{}: {message}", batch.title));
            }
        }
    }
    ensure_series_info(state, &batch.series_id);
}

/// Stop one batch of the queue (a waiting one is simply dropped).
pub fn cancel_batch(state: AppState, id: u64) {
    state.edit_queue(|q| {
        q.cancel(id);
    });
}

/// Put one download aside; it goes on from the chapters not yet saved when
/// resumed.
pub fn pause_batch(state: AppState, id: u64) {
    state.edit_queue(|q| {
        q.pause(id);
    });
}

/// Let a paused download take its turn again.
pub fn resume_batch(state: AppState, id: u64) {
    state.edit_queue(|q| {
        q.resume(id);
    });
    ensure_queue_running(state);
}

/// Take one finished download off the list.
pub fn dismiss_batch(state: AppState, id: u64) {
    state.edit_queue(|q| {
        q.remove(id);
    });
}

/// Stop everything in the queue.
pub fn cancel_all_downloads(state: AppState) {
    state.edit_queue(|q| q.cancel_all());
}

/// Pause the queue (the batch running carries on) or let it go on.
pub fn set_queue_paused(state: AppState, paused: bool) {
    state.edit_queue(|q| q.paused = paused);
    if !paused {
        ensure_queue_running(state);
    }
}

/// Try the chapters of a finished batch that were not downloaded once more.
pub fn retry_batch(state: AppState, id: u64) {
    let queued = state.edit_queue(|q| q.retry(id, now_secs()));
    if queued > 0 {
        state.notify(format!(
            "Trying {} again.",
            logic::plural(queued, "chapter")
        ));
        ensure_queue_running(state);
    }
}

/// Move a waiting batch to the front of the line.
pub fn prioritize_batch(state: AppState, id: u64) {
    state.edit_queue(|q| q.prioritize(id));
}

/// Rank the sources for these chapters, speed-testing the ones that matter
/// and have not been tested lately, and put the best first for every chapter.
pub(super) async fn order_sources(
    state: AppState,
    batch_id: u64,
    client: &catalog::SuwayomiClient,
    requests: Vec<pipeline::ChapterRequest>,
    names: &HashMap<String, String>,
) -> Vec<pipeline::ChapterRequest> {
    let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let now = now_secs();
    let ranked = pipeline::rank_sources(&requests, |s| state.speeds.peek().speed(s));
    // Sources that cover about as much as the best are worth comparing; one
    // that was tested recently does not need testing again.
    let to_test: Vec<(String, pipeline::ChapterRef)> = {
        let speeds = state.speeds.peek();
        let tier = pipeline::top_tier(&ranked);
        if tier.len() < 2 {
            Vec::new()
        } else {
            tier.iter()
                .filter(|r| !speeds.is_fresh(&r.source_id, now))
                .filter_map(|r| {
                    let option = requests
                        .iter()
                        .find_map(|q| q.options.iter().find(|o| o.source_id == r.source_id))?;
                    Some((r.source_id.clone(), option.clone()))
                })
                .collect()
        }
    };
    if !to_test.is_empty() {
        state.edit_queue(|q| {
            if let Some(b) = q.get_mut(batch_id) {
                b.progress.note = Some("Checking which source is fastest…".into());
            }
        });
        let results = pipeline::probe_sources(client, &to_test, &RetryPolicy::default()).await;
        for (source, speed) in results {
            match speed {
                Some(pages_per_second) => {
                    info!(
                        "Speed test: {} {pages_per_second:.1} pages/s",
                        name_of(&source)
                    );
                    state.edit_speeds(|db| db.record(&source, pages_per_second, now_secs()));
                }
                None => {
                    warn!("Speed test: {} gave no usable pages", name_of(&source));
                    state.edit_speeds(|db| db.record_failure(&source, now_secs()));
                }
            }
        }
        state.save_speeds();
    }
    let ranked = pipeline::rank_sources(&requests, |s| state.speeds.peek().speed(s));
    for r in &ranked {
        info!(
            "Source order: {} covers {} chapter(s), {}",
            name_of(&r.source_id),
            r.coverage,
            r.speed
                .map_or("not measured".to_string(), |v| format!("{v:.1} pages/s"))
        );
    }
    pipeline::apply_choice(requests, &SourceChoice::Adaptive, &ranked).requests
}

/// How often the progress on screen is brought up to date while a download or
/// export runs (about seven times a second).
pub(super) const PROGRESS_REFRESH: Duration = Duration::from_millis(140);

/// Stop the running download or export. The screen says so at once; the job
/// itself ends as soon as the pages already on their way are dropped.
pub fn cancel_job(state: AppState) {
    let mut job = state.job;
    // Paused: nothing is running, so it is simply given up.
    if job.peek().as_ref().is_some_and(|v| v.paused && v.result.is_none()) {
        drop_paused_export(state);
        return;
    }
    let mut guard = job.write();
    if let Some(view) = guard.as_mut() {
        if view.result.is_none() {
            view.paused = false;
            view.cancel.cancel();
            view.progress.stopping = true;
        }
    }
}
