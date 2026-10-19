//! The footer of the right-hand column: totals, and the button that starts the export.

use crate::logic::{self, plural};
use crate::state::{AppState, GroupingChoice, JobKind};
use crate::ui::widgets::Icon;
use crate::ui::work::start_export_plan;
use dioxus::prelude::*;

#[component]
pub(super) fn ExportFooter() -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read();
    let opts = state.export_opts.read().clone();
    let original = device_profiles::is_original(&state.settings.read().device);
    // Whether something runs, not how far it has got: the footer is not drawn
    // again for every page.
    let (busy, exporting) = use_memo(move || {
        let job = state.job.read();
        let running = job.as_ref().is_some_and(|j| j.result.is_none());
        let exporting = job
            .as_ref()
            .is_some_and(|j| j.kind == JobKind::Export && j.result.is_none());
        (running, exporting)
    })();

    let (mut series_count, mut chapters, mut pages, mut bytes) = (0usize, 0usize, 0usize, 0u64);
    for item in plan.items.iter().filter(|i| !i.off) {
        let Some(series) = library.find(&item.series_id).filter(|s| !s.is_sample()) else {
            continue;
        };
        let included: Vec<_> = series
            .chapters
            .iter()
            .filter(|c| !item.excluded.contains(&c.key))
            .collect();
        if included.is_empty() {
            continue;
        }
        series_count += 1;
        chapters += included.len();
        let p: usize = included.iter().map(|c| c.pages).sum();
        pages += p;
        // A phone or tablet gets the pages as they are: about what they take on disk.
        bytes += if original {
            included.iter().map(|c| c.bytes).sum()
        } else {
            logic::estimate_output_bytes(p, series.settings.output())
        };
    }
    let every_n_valid = opts.every_n.trim().parse::<usize>().is_ok_and(|n| n >= 1);

    rsx! {
        div { class: "px-footer",
            div { class: "summary-line", span { class: "muted", "Series" } span { "{series_count}" } }
            div { class: "summary-line", span { class: "muted", "Chapters" } span { "{chapters}" } }
            div { class: "summary-line", span { class: "muted", "Pages" } span { "{pages}" } }
            div { class: "summary-line", span { class: "muted", "Size, about" } span { "{logic::human_bytes(bytes)}" } }
            button {
                class: "btn primary big", style: "margin-top: 10px",
                disabled: chapters == 0 || busy || (opts.grouping == GroupingChoice::EveryN && !every_n_valid),
                onclick: move |_| start_export_plan(state),
                if exporting { "Exporting…" }
                else if busy { "Something else is running…" }
                else if chapters == 0 { "Nothing to export yet" }
                else { Icon { name: "file-down", size: 18 } "Export {plural(series_count, \"series\")}" }
            }
        }
    }
}
