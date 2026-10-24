//! First run: the engine, then sources, then a reader, each of which can be
//! skipped once it is reached. Where it got to is kept, so closing the app
//! part way through picks up again where it stopped.

use super::readers::ReaderAdder;
use super::setup::Setup;
use super::walker::WalkingCat;
use super::widgets::{Icon, ProgressBar};
use crate::backend;
use crate::state::{spawn_forever, AppState, BootState};
use dioxus::prelude::*;
use library::SetupStep;

/// Which step is on show when it is not the one the setup has reached: Back
/// goes to an earlier one, to look at or change it.
type View = Signal<Option<SetupStep>>;

/// Move first-run setup on to `step` and remember it. It never goes back.
fn set_step(state: AppState, step: SetupStep) {
    let reached = state.ui.settings.peek().setup_step;
    if step == SetupStep::Done || position(step) > position(reached) {
        let mut settings = state.ui.settings;
        settings.write().setup_step = step;
        state.save_settings();
    }
}

/// Leave a picture of the step on show sliding out of the window (left when
/// going on, right when going back). Done once it is in place.
async fn slide_out(forward: bool) {
    let mut done = document::eval(&format!(
        "if (window.mpSlideOut) {{ window.mpSlideOut({forward}); }} dioxus.send(true);"
    ));
    let _ = done.recv::<bool>().await;
}

const STEPS: [SetupStep; 3] = [SetupStep::Engine, SetupStep::Sources, SetupStep::Reader];

fn position(step: SetupStep) -> usize {
    STEPS.iter().position(|s| *s == step).unwrap_or(STEPS.len())
}

#[component]
pub fn Onboarding() -> Element {
    let state = use_context::<AppState>();
    let reached = state.ui.settings.read().setup_step;
    let mut view: View = use_signal(|| None);
    // Which way the page came in from, so it slides in from that side.
    let mut forward = use_signal(|| true);
    let shown = view().filter(|s| position(*s) <= position(reached)).unwrap_or(reached);
    let pos = position(shown);

    let engine_ready = matches!(*state.boot.read(), BootState::Ready);
    let source_installed = state
        .sources
        .read()
        .choices
        .values()
        .any(|c| *c == library::Choice::Install);

    let next_label = match shown {
        SetupStep::Engine | SetupStep::Sources => "Next",
        _ => "Finish",
    };
    // Nothing to press until the engine is there.
    let show_next = shown != SetupStep::Engine || engine_ready;

    // While one page is sliding out and the next in, nothing else is accepted
    // (the buttons stay where they are, so a second click would skip a page).
    let mut moving = use_signal(|| false);
    // The step on show slides out first; only then does the next one come in.
    let go_next = move |_| {
        if moving() {
            return;
        }
        moving.set(true);
        spawn(async move {
            if shown != SetupStep::Reader {
                slide_out(true).await;
            }
            forward.set(true);
            // Next is the page after the one on show, even when the setup has
            // already been further (after going Back to look at an earlier one).
            let next = match shown {
                SetupStep::Engine => SetupStep::Sources,
                SetupStep::Sources => {
                    if source_installed {
                        super::setup::finish_sources(state);
                    }
                    SetupStep::Reader
                }
                _ => SetupStep::Done,
            };
            set_step(state, next);
            view.set((next != SetupStep::Done).then_some(next));
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            moving.set(false);
        });
    };
    let go_back = move |_| {
        if pos > 0 && !moving() {
            moving.set(true);
            spawn(async move {
                slide_out(false).await;
                forward.set(false);
                view.set(Some(STEPS[pos - 1]));
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                moving.set(false);
            });
        }
    };

    rsx! {
        div { class: "onboard",
            // The stage clips what slides past its edges.
            div { class: "onboard-stage",
                // A new page is a new element (keyed by its step), so it slides in afresh.
                for step_key in [pos] {
                    div {
                        key: "{step_key}",
                        class: if forward() { "onboard-body from-right" } else { "onboard-body from-left" },
                        {match shown {
                            SetupStep::Engine => rsx! { EngineStep {} },
                            SetupStep::Sources => rsx! { Setup { onboarding: true } },
                            _ => rsx! { ReaderStep {} },
                        }}
                    }
                }
            }
            div { class: "onboard-nav",
                if pos > 0 {
                    button { class: "btn ghost", onclick: go_back,
                        Icon { name: "arrow-left", size: 15 }
                        "Back"
                    }
                }
                div { class: "grow" }
                if show_next {
                    button { class: "btn primary", onclick: go_next,
                        "{next_label}"
                        if next_label == "Finish" { Icon { name: "check", size: 15 } } else { Icon { name: "chevron-right", size: 15 } }
                    }
                }
            }
        }
    }
}

/// The engine: what it is and why, a button to install it, and the cat walking
/// while it downloads.
#[component]
fn EngineStep() -> Element {
    let state = use_context::<AppState>();
    let boot = state.boot.read().clone();
    let folder = crate::state::paths().suwayomi_dir().display().to_string();
    rsx! {
        div { class: "engine-step",
            WalkingCat {}
            {match boot {
                BootState::NeedsEngine => rsx! {
                    h1 { "Install the catalog engine" }
                    p { class: "muted", title: "{folder}",
                        "MangaPoint finds manga and downloads chapters through a helper program, the catalog engine (Suwayomi). "
                        "Searching and downloading cannot work without it. "
                        "It is a one-time 340 MB download from GitHub, kept in MangaPoint\u{2019}s own folder, "
                        "that only runs while the app is open."
                    }
                    button { class: "btn primary big", style: "margin-top: 22px",
                        onclick: move |_| spawn_forever(backend::boot(state, true)),
                        Icon { name: "download", size: 16 }
                        "Install the engine"
                    }
                },
                BootState::Starting { message, percent } => rsx! {
                    h1 { "Getting the engine ready" }
                    p { class: "muted", "{message}" }
                    div { class: "engine-bar", ProgressBar { fraction: percent.map(|p| f32::from(p) / 100.0) } }
                    p { class: "muted small", "This only happens once. You can leave the window open and come back." }
                },
                _ => rsx! {
                    h1 { "The engine is ready" }
                    p { class: "muted", "Nothing more to install." }
                },
            }}
        }
    }
}

#[component]
fn ReaderStep() -> Element {
    let state = use_context::<AppState>();
    let devices = state.ui.settings.read().devices();
    let added = !devices.is_empty();
    rsx! {
        div { class: "onboard-head",
            div { class: "step-hero", WalkingCat { day: true } }
            h1 { "Choose your reader" }
            p { class: "muted",
                "Pages are made at the size of your reader\u{2019}s screen. Search for it, or type a name to add one of your own. "
                "You can add, change or remove readers any time in Settings."
            }
        }
        // The readers you have, above the search box. The row opens up when the
        // first one is added, and each one pops in as it is.
        div { class: if added { "reader-picked has" } else { "reader-picked" },
            div { class: "reader-picked-in",
                div { class: "pills",
                    for d in devices {
                        span { class: "pill on pop", key: "{d.id}",
                            Icon { name: "check", size: 13 }
                            "{d.name}"
                        }
                    }
                }
            }
        }
        div { class: "reader-pick",
            // The page slides in for 0.6 s; the cursor goes into the box after that.
            ReaderAdder { always_open: !added, focus_after_ms: 700, on_added: move |_| {} }
        }
    }
}
