//! The Export screen: full window. Editing the series is part of exporting
//! them, so this is where it happens. Presets, the editing controls and the
//! list of series being exported are panels on the left, the pages in the
//! middle, and the chapters and where to send them on the right. Every panel
//! folds away to give the others room.

mod chapters_panel;
mod edit_panel;
mod footer;
mod original_preview;
mod presets_panel;
mod send_panel;
mod series_panel;

use crate::state::AppState;
use crate::ui::widgets::Icon;
use crate::ui::work::{activate_export_series, load_saved_cover, open_sample, reconcile_export_plan};
use crate::ui::workspace::preview::PreviewTab;
use chapters_panel::ChaptersPanel;
use dioxus::prelude::*;
use edit_panel::EditPanel;
use footer::ExportFooter;
use library::DownloadedSeries;
use presets_panel::PresetsPanel;
use send_panel::SendPanel;
use series_panel::{AddSeries, SeriesPanel};
use std::time::Duration;

/// The series on screen in the Export list.
fn active_series(state: AppState) -> Option<DownloadedSeries> {
    let plan = state.export_plan.read();
    let id = plan.items.get(plan.active)?.series_id.clone();
    drop(plan);
    state.library.read().find(&id).cloned()
}

/// A panel with a title that folds away.
#[component]
fn Panel(
    title: String,
    icon: &'static str,
    /// What the splitters call this panel.
    id: &'static str,
    /// A starting height in pixels. Without one the panel takes what is left.
    height: Option<u32>,
    /// Buttons for the title bar, which do not fold the panel.
    actions: Option<Element>,
    children: Element,
) -> Element {
    let mut open = use_signal(|| true);
    let class = match (open(), height.is_some()) {
        (true, true) => "px-panel open fixed",
        (true, false) => "px-panel open",
        _ => "px-panel collapsed",
    };
    let style = height.map(|h| format!("--h: {h}px;")).unwrap_or_default();
    let chevron = if open() {
        "chevron-down"
    } else {
        "chevron-right"
    };
    rsx! {
        section { class: "{class}", style: "{style}", "data-panel": "{id}",
            header { class: "px-panel-head", onclick: move |_| open.set(!open()),
                Icon { name: chevron, size: 15 }
                Icon { name: icon, size: 15 }
                span { class: "grow", "{title}" }
                if let Some(actions) = actions {
                    div { class: "px-panel-actions", onclick: move |e| e.stop_propagation(), {actions} }
                }
            }
            div { class: "px-panel-body", {children} }
        }
    }
}

#[component]
pub fn ExportPage() -> Element {
    let state = use_context::<AppState>();
    let mut adding = use_signal(|| false);
    let mut closing = use_signal(|| false);
    let plan = state.export_plan.read().clone();
    let original = device_profiles::is_original(&state.settings.read().device);

    // Nothing chosen to export: show the sample pages.
    use_effect(move || {
        if state.export_plan.peek().items.is_empty() {
            open_sample(state);
        }
    });
    // Series that were removed (or lost every downloaded chapter) while this
    // list was open go off it, and the sample pages come back if none is left.
    let usable_series = use_memo(move || {
        state
            .library
            .read()
            .series
            .iter()
            .filter(|s| !s.chapters.is_empty())
            .map(|s| s.id.clone())
            .collect::<Vec<_>>()
    });
    use_effect(move || {
        let _ = usable_series();
        reconcile_export_plan(state);
    });
    // Opened again after other pages were visited: they changed what the
    // preview shows, so put the series of the list back.
    use_effect(move || {
        let plan = state.export_plan.peek().clone();
        let wanted = plan.items.get(plan.active).map(|i| i.series_id.clone());
        let shown = state.workspace.peek().as_ref().map(|w| w.series_id.clone());
        if wanted.is_some() && wanted != shown {
            activate_export_series(state, plan.active);
        }
    });
    // Remember the chapter being previewed.
    use_effect(move || {
        let key = state.preview.read().chapter_key.clone();
        let Some(key) = key else { return };
        let mut plan = state.export_plan;
        let differs = {
            let p = plan.peek();
            p.items
                .get(p.active)
                .is_some_and(|i| i.chapter.as_deref() != Some(key.as_str()))
        };
        if differs {
            let mut guard = plan.write();
            let active = guard.active;
            if let Some(item) = guard.items.get_mut(active) {
                item.chapter = Some(key);
            }
        }
    });

    // The covers of the series in the list.
    {
        let ids: Vec<String> = plan.items.iter().map(|i| i.series_id.clone()).collect();
        use_effect(move || {
            for id in &ids {
                load_saved_cover(state, id);
            }
        });
    }

    if plan.items.is_empty() {
        return rsx! { p { class: "muted", style: "padding: 24px", "Opening the sample pages…" } };
    }

    let mut close = move || {
        if closing() {
            return;
        }
        closing.set(true);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(180)).await;
            adding.set(false);
            closing.set(false);
        });
    };

    rsx! {
        div { class: "export-screen",
            onmounted: move |_| { spawn(async move { let _ = document::eval(SPLIT_SCRIPT).await; }); },
            div { class: "export-body",
                aside { class: "export-side left",
                    // The e-ink controls have nothing to do for a phone or tablet:
                    // its pages go out as they are.
                    if !original {
                        PresetsPanel {}
                        Split { target: "presets", dir: 1 }
                        EditPanel {}
                        Split { target: "series", dir: -1 }
                    }
                    SeriesPanel { on_add: move |_| adding.set(true) }
                    div { class: "px-split-v", "data-split": "left", title: "Drag to make this column wider or narrower" }
                }
                main { class: "export-center",
                    if original { original_preview::OriginalPreview {} } else { PreviewTab {} }
                }
                aside { class: "export-side right",
                    div { class: "px-split-v", "data-split": "right", title: "Drag to make this column wider or narrower" }
                    ChaptersPanel {}
                    Split { target: "send", dir: -1 }
                    SendPanel {}
                    ExportFooter {}
                }
            }
            if adding() {
                div { class: if closing() { "overlay out" } else { "overlay" }, onclick: move |_| close(),
                    div { class: "modal wide", onclick: move |e| e.stop_propagation(),
                        AddSeries { on_close: move |_| close() }
                    }
                }
            }
        }
    }
}

/// The bar between two panels, which can be dragged to resize them. `target`
/// is the panel that changes size; `dir` is -1 when dragging down shrinks it.
#[component]
fn Split(target: &'static str, dir: i32) -> Element {
    rsx! {
        div { class: "px-split", "data-split": "panel", "data-target": "{target}", "data-dir": "{dir}",
            title: "Drag to resize the panels" }
    }
}

/// Dragging the bars between the panels and the edges of the side columns.
/// The sizes are remembered between visits.
const SPLIT_SCRIPT: &str = include_str!("../../js/export-split.js");
