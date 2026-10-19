//! The Edit panel: the conversion controls for the series on screen.

use super::{active_series, Panel};
use crate::state::AppState;
use crate::ui::conversion::ConversionControls;
use crate::ui::work::schedule_preview;
use crate::ui::workspace::crop_editor::CropInspector;
use dioxus::prelude::*;
use library::CropRect;

#[component]
pub(super) fn EditPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let p = state.preview.read().clone();
    let view = p.view;
    let chapter = p.chapter_key.clone().unwrap_or_default();
    let total = p.pages.len();
    let selected = p.selected.min(total.saturating_sub(1));
    let key = p
        .pages
        .get(selected)
        .map(|page| page.crop_key.clone())
        .unwrap_or_default();
    let saved = series.crops.scope_for(&chapter, &key);
    let draft = state.crop_draft;
    let id = series.id.clone();
    let (mode_id, mode_key, mode_chapter) = (id.clone(), key.clone(), chapter.clone());
    let preset_base = series
        .preset_id
        .as_ref()
        .and_then(|id| state.presets.read().find(id))
        .map(|e| e.preset.settings);

    rsx! {
        Panel { title: "Edit", icon: "sliders-horizontal", id: "edit",
            ConversionControls {
                crop_section: rsx! { CropSection { identity: "{chapter}/{key}", draft, crop_key: key.clone(), saved } },
                value: series.settings.clone(),
                compact: true,
                shading_samples: p.shading_samples.clone(),
                manual_crop_active: series.crops.effective(&chapter, &key).unwrap_or_default() != CropRect::default() || view.editing_crop,
                on_manual_crop: move |_| {
                    let mut pv = state.preview;
                    let mut w = pv.write();
                    w.view.editing_crop = true;
                    w.view.show_original = true;
                    w.view.source_focus = true;
                },
                on_crop_mode: move |_| {
                    let mut library = state.library;
                    if let Some(series) = library.write().series.iter_mut().find(|s| s.id == mode_id) {
                        series.crops.set(library::CropScope::Page, &mode_chapter, &mode_key, Some(CropRect::default()));
                    }
                    let mut pv = state.preview;
                    let mut w = pv.write();
                    w.draft_crop = None;
                    w.view.editing_crop = false;
                    drop(w);
                    state.save_library();
                },
                onchange: move |v| {
                    let mut library = state.library;
                    library.write().set_settings(&id, v, preset_base.as_ref());
                    state.save_library();
                    schedule_preview(state, 180);
                },
            }
        }
    }
}

/// The crop controls; they start over for each page.
#[component]
fn CropSection(
    identity: String,
    draft: Signal<CropRect>,
    crop_key: String,
    saved: Option<library::CropScope>,
) -> Element {
    rsx! { CropInspector { key: "{identity}", draft, crop_key, saved } }
}
