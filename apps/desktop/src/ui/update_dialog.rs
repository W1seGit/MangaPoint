//! The dialog that offers a newer version, and the check that opens it.
//!
//! Top to bottom: the mascot with a parcel, what the new version is, a short summary
//! of its release notes, a switch to skip this version, and the choice: later,
//! or update (and restart).

use super::widgets::{open_link, Icon, ProgressBar, Toggle};
use mangapoint_ui::art::mascot::UpdateScene;
use crate::state::{AppState, UpdatePhase};
use crate::updates::{self, Apply, Applied, Update};
use dioxus::prelude::*;
use std::time::Duration;
use tracing::info;

// ---- starting an update --------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Download, check and apply `update`, keeping the dialog up to date.
fn start_update(state: AppState, update: Update) {
    crate::state::spawn_forever(async move {
        let mut phase = state.update_phase;
        phase.set(UpdatePhase::Downloading(0.0));
        let mut last = 0.0f32;
        let downloaded = updates::download(&update, |f| {
            if f - last >= 0.01 || f >= 1.0 {
                last = f;
                phase.set(UpdatePhase::Downloading(f));
            }
        })
        .await;
        match downloaded.and_then(|path| updates::apply(&update, &path)) {
            Ok(Applied::Replacing) => {
                info!("Updating to {}: the installer is running", update.version);
                phase.set(UpdatePhase::Replacing);
                // Let the installer take over, then close so it can replace us.
                tokio::time::sleep(Duration::from_millis(1500)).await;
                // Anything changed in the last second is still waiting to be saved.
                state.flush_saves();
                crate::backend::shutdown();
                std::process::exit(0);
            }
            Ok(Applied::Handed { path, command }) => {
                info!("Update {} downloaded and handed over: {}", update.version, path.display());
                phase.set(UpdatePhase::Handed { path: path.display().to_string(), command });
            }
            Err(why) => {
                info!("Update {} did not work: {why}", update.version);
                phase.set(UpdatePhase::Failed(why));
            }
        }
    });
}

/// Close the offer. With `skip`, this version is not offered again.
fn close(state: AppState, version: &str, skip: bool) {
    if skip {
        let mut settings = state.ui.settings;
        settings.write().update_skipped = version.to_string();
        state.save_settings();
        info!("Update {version} skipped");
    }
    let mut prompt = state.update_prompt;
    prompt.set(None);
    let mut phase = state.update_phase;
    phase.set(UpdatePhase::Idle);
}

/// Open the offer for `update` (from the About page or the daily check).
pub fn offer(state: AppState, update: Update) {
    let mut phase = state.update_phase;
    phase.set(UpdatePhase::Idle);
    let mut prompt = state.update_prompt;
    prompt.set(Some(update));
}

#[component]
pub fn UpdateDialogHost() -> Element {
    let state = use_context::<AppState>();
    let mut skip = use_signal(|| false);
    let Some(update) = state.update_prompt.read().clone() else {
        return rsx! {};
    };
    let phase = state.update_phase.read().clone();
    let working = matches!(phase, UpdatePhase::Downloading(_) | UpdatePhase::Replacing);
    let in_place = update.apply == Apply::Replace;
    let go_label = match update.apply {
        Apply::Replace => "Update and restart",
        Apply::Page => "Open the download page",
        _ => "Download update",
    };
    let icon_name: &'static str = if in_place { "refresh-cw" } else { "download" };
    let version = update.version.clone();
    let for_close = version.clone();
    let for_page = update.page.clone();
    let to_install = update.clone();
    rsx! {
        div { class: "overlay", onclick: move |_| if !working { close(state, &for_close, skip()) },
            div { class: "modal update-modal", onclick: move |e| e.stop_propagation(),
                div { class: "upd-top", UpdateScene {} }
                h2 { "MangaPoint {version} is out" }
                p { class: "muted small", style: "margin: 0 0 10px", "You have {updates::VERSION}." }
                if !update.summary.is_empty() {
                    ul { class: "upd-notes",
                        for line in update.summary.iter() {
                            li { key: "{line}", "{line}" }
                        }
                    }
                }
                {match phase {
                    UpdatePhase::Downloading(fraction) => rsx! {
                        div { class: "upd-status",
                            p { class: "muted small", "Downloading and checking the update\u{2026} {(fraction * 100.0) as u32}%" }
                            ProgressBar { fraction: Some(fraction) }
                        }
                    },
                    UpdatePhase::Replacing => rsx! {
                        div { class: "upd-status",
                            p { class: "muted small", "Installing. MangaPoint will close and open again by itself." }
                            ProgressBar { fraction: None }
                        }
                    },
                    UpdatePhase::Handed { path, command } => rsx! {
                        div { class: "upd-status",
                            p { class: "small", "The update is downloaded and checked." }
                            if let Some(command) = command {
                                p { class: "muted small", "Install it with:" }
                                code { class: "upd-command", "{command}" }
                            } else {
                                p { class: "muted small", "Finish installing it from the window that opened, then start MangaPoint again." }
                            }
                            p { class: "muted small", style: "word-break: break-all", "{path}" }
                        }
                    },
                    UpdatePhase::Failed(why) => rsx! {
                        div { class: "upd-status",
                            p { class: "small", style: "color: var(--bad)", "{why}" }
                        }
                    },
                    UpdatePhase::Idle => rsx! {
                        div { class: "check-line upd-skip",
                            Toggle { on: skip(), onchange: move |on: bool| skip.set(on) }
                            span { "Skip this version" }
                        }
                    },
                }}
                div { class: "actions",
                    {match state.update_phase.read().clone() {
                        UpdatePhase::Handed { .. } => rsx! {
                            button { class: "btn primary", onclick: move |_| close(state, &version, false), "Done" }
                        },
                        UpdatePhase::Failed(_) => rsx! {
                            button { class: "btn", onclick: move |_| close(state, &version, false), "Close" }
                            if update.apply != Apply::Page {
                                button { class: "btn", onclick: move |_| open_link(&for_page), "Open the download page" }
                            }
                            button { class: "btn primary",
                                onclick: move |_| start_update(state, to_install.clone()),
                                "Try again"
                            }
                        },
                        _ => rsx! {
                            button { class: "btn", disabled: working,
                                onclick: move |_| close(state, &version, skip()),
                                "Later"
                            }
                            button { class: "btn primary", disabled: working,
                                onclick: move |_| {
                                    if update.apply == Apply::Page {
                                        open_link(&for_page);
                                    } else {
                                        start_update(state, to_install.clone());
                                    }
                                },
                                Icon { name: icon_name, size: 15 }
                                {go_label}
                            }
                        },
                    }}
                }
            }
        }
    }
}

// ---- the daily check ----------------------------------------------------------

/// Look for a newer version shortly after the app opens, if a check is due, and
/// offer it unless it was skipped. It waits while a download or an export is
/// running and while the first-run setup is still on, so it never gets in the way.
pub async fn check_at_startup(state: AppState) {
    tokio::time::sleep(Duration::from_secs(8)).await;
    {
        let s = state.ui.settings.peek();
        if s.setup_step != library::SetupStep::Done
            || !updates::check_due(s.update_check, s.update_last_check, now_secs())
        {
            return;
        }
    }
    match updates::check().await {
        Ok(found) => {
            let mut settings = state.ui.settings;
            settings.write().update_last_check = now_secs();
            state.save_settings();
            let Some(update) = found else {
                info!("Update check: this is the newest version");
                return;
            };
            if !updates::offer_unasked(&update.version, &state.ui.settings.peek().update_skipped) {
                info!("Update check: {} is out, and was skipped", update.version);
                return;
            }
            info!("Update check: {} is out", update.version);
            // Not in the middle of something.
            for _ in 0..40 {
                let busy = state.queue.peek().running().is_some()
                    || state.job.peek().as_ref().is_some_and(|j| j.result.is_none())
                    || state.ui.settings.peek().setup_step != library::SetupStep::Done;
                if !busy {
                    offer(state, update);
                    return;
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
        Err(why) => info!("Update check did not work: {why}"),
    }
}
