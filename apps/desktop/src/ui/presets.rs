//! Conversion presets: choosing, saving, updating and managing them.

use super::widgets::Icon;
use super::work::schedule_preview;
use crate::state::{AppState, Page};
use dioxus::prelude::*;
use library::{ConversionSettings, Deleted};

/// Give a series a preset's settings and redraw the preview.
pub fn apply_to_series(
    state: AppState,
    series_id: &str,
    preset_id: Option<String>,
    settings: ConversionSettings,
) {
    let mut library = state.library;
    {
        let mut db = library.write();
        db.set_settings(series_id, settings.clone(), Some(&settings));
        db.set_preset(series_id, preset_id);
    }
    state.save_library();
    schedule_preview(state, 0);
}

/// Make every series of the export as `output` (XTC, XTCH, EPUB or CBZ), and
/// draw the preview again.
pub fn set_output(state: AppState, output: library::OutputFormat) {
    let ids = export_series_ids(state);
    set_output_of(state, &ids, |_| output);
}

/// Make every series of the export colour (or, with `black_and_white`, gray
/// pages) where its reader and format allow it, and draw the preview again.
/// Series made as XTC or XTCH keep the choice but never use it.
pub fn set_black_and_white(state: AppState, black_and_white: bool) {
    let mut library = state.library;
    for id in export_series_ids(state) {
        let Some(series) = state.library.peek().find(&id).cloned() else {
            continue;
        };
        let mut settings = series.settings.clone();
        settings.black_and_white = black_and_white;
        let preset = series
            .preset_id
            .as_ref()
            .and_then(|p| state.presets.peek().find(p))
            .map(|e| e.preset.settings);
        library.write().set_settings(&id, settings, preset.as_ref());
    }
    state.save_library();
    schedule_preview(state, 0);
}

/// The series on the Export screen.
fn export_series_ids(state: AppState) -> Vec<String> {
    state
        .export_plan
        .peek()
        .items
        .iter()
        .map(|i| i.series_id.clone())
        .collect()
}

/// Make each of the series `ids` as the format `choose` gives for it, and draw
/// the preview again.
fn set_output_of(
    state: AppState,
    ids: &[String],
    choose: impl Fn(&library::DownloadedSeries) -> library::OutputFormat,
) {
    let mut library = state.library;
    for id in ids {
        let Some(series) = state.library.peek().find(id).cloned() else {
            continue;
        };
        let mut settings = series.settings.clone();
        settings.set_output(choose(&series));
        let preset = series
            .preset_id
            .as_ref()
            .and_then(|p| state.presets.peek().find(p))
            .map(|e| e.preset.settings);
        library.write().set_settings(id, settings, preset.as_ref());
    }
    state.save_library();
    schedule_preview(state, 0);
}

/// After another reader is chosen, or a series is added or loaded with a
/// preset: if a series is made as something the reader does not open (an XTC
/// for a Kindle that takes only EPUB, say), switch it to the first thing the
/// reader does. Returns whether anything was changed.
pub fn fit_output_to_reader(state: AppState) -> bool {
    // A phone or tablet takes the pages as they are: there is no format to fit.
    if device_profiles::is_original(&state.settings.peek().device) {
        return false;
    }
    let setup = state.settings.peek().reader_setup();
    let listed = |o: library::OutputFormat| setup.formats.iter().any(|f| f.eq_ignore_ascii_case(o.label()));
    let Some(first) = library::OutputFormat::ALL.into_iter().find(|o| listed(*o)) else {
        return false;
    };
    let wrong: Vec<String> = export_series_ids(state)
        .into_iter()
        .filter(|id| {
            state
                .library
                .peek()
                .find(id)
                .is_some_and(|s| !s.is_sample() && !listed(s.settings.output()))
        })
        .collect();
    if wrong.is_empty() {
        return false;
    }
    set_output_of(state, &wrong, |_| first);
    true
}

/// Replace one of the user's presets with some settings.
pub fn save_to_preset(state: AppState, id: &str, settings: ConversionSettings) {
    let result = state.edit_presets(|p| p.overwrite(id, settings));
    match result {
        Ok(()) => {
            state.save_presets();
            state.notify("Preset saved.");
        }
        Err(e) => state.error(e.to_string()),
    }
}

/// Delete one of the user's presets, or put a changed built-in back. Series
/// and defaults that pointed at a deleted preset keep their settings but lose
/// the link.
pub fn delete_preset(state: AppState, id: String) {
    let result = state.edit_presets(|p| p.delete(&id));
    match result {
        Ok(Deleted::Removed) => {
            let mut library = state.library;
            {
                let mut db = library.write();
                for s in db
                    .series
                    .iter_mut()
                    .filter(|s| s.preset_id.as_deref() == Some(&id))
                {
                    s.preset_id = None;
                }
            }
            state.save_library();
            let mut settings = state.settings;
            if settings.peek().default_preset_id.as_deref() == Some(&id) {
                settings.write().default_preset_id = None;
                state.save_settings();
            }
            state.save_presets();
            state.notify("Preset deleted.");
        }
        Ok(Deleted::Reset) => {
            state.save_presets();
            state.notify("Back to the original settings.");
        }
        Err(e) => state.error(e.to_string()),
    }
}

/// Choose a preset for some settings. Advanced mode also lets you update the
/// preset, save the settings as a new one, or go and manage them. Used on the
/// Preview tab, for one series.
#[component]
pub fn PresetPicker(
    value: ConversionSettings,
    preset_id: Option<String>,
    /// The user chose a preset: its id and settings.
    on_pick: EventHandler<(String, ConversionSettings)>,
    /// The current settings were saved as a new preset with this id.
    on_saved: EventHandler<String>,
) -> Element {
    let state = use_context::<AppState>();
    let mut naming = use_signal(|| false);
    let mut name = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let advanced = state.settings.read().advanced_conversion;

    let entries = state.presets.read().all();
    let current = preset_id
        .as_ref()
        .and_then(|id| entries.iter().find(|e| &e.preset.id == id))
        .cloned();
    let changed = current.as_ref().is_some_and(|e| e.preset.settings != value);
    let selected = current
        .as_ref()
        .map(|e| e.preset.id.clone())
        .unwrap_or_default();

    let pick_entries = entries.clone();
    let on_select = move |e: Event<FormData>| {
        let id = e.value();
        if let Some(entry) = pick_entries.iter().find(|p| p.preset.id == id) {
            on_pick.call((entry.preset.id.clone(), entry.preset.settings.clone()));
        }
    };

    let update_value = value.clone();
    let update_id = current.as_ref().map(|e| e.preset.id.clone());
    let update_preset = move |_| {
        if let Some(id) = &update_id {
            let result = state.edit_presets(|p| p.overwrite(id, update_value.clone()));
            match result {
                Ok(()) => {
                    state.save_presets();
                    state.notify("Preset updated.");
                }
                Err(e) => state.error(e.to_string()),
            }
        }
    };
    let undo = current.as_ref().map(|e| {
        let (id, settings) = (e.preset.id.clone(), e.preset.settings.clone());
        move |_| on_pick.call((id.clone(), settings.clone()))
    });

    let save_value = value.clone();
    let save_new = move |_| {
        let result = state.edit_presets(|p| p.save_new(&name.read(), save_value.clone()));
        match result {
            Ok(id) => {
                state.save_presets();
                naming.set(false);
                name.set(String::new());
                error.set(None);
                on_saved.call(id);
                state.notify("Preset saved.");
            }
            Err(e) => error.set(Some(e.to_string())),
        }
    };

    rsx! {
        div { class: "panel-section",
            label { class: "panel-label", "Preset" }
            select { value: "{selected}", onchange: on_select,
                option { value: "", selected: current.is_none(), disabled: true, "Custom settings" }
                for e in entries.iter() {
                    option {
                        key: "{e.preset.id}",
                        value: "{e.preset.id}",
                        selected: e.preset.id == selected,
                        "{e.preset.name}"
                        if e.builtin && e.modified { " (yours)" }
                    }
                }
            }
            if let Some(entry) = &current {
                p { class: "muted small panel-note",
                    if !entry.preset.description.is_empty() { "{entry.preset.description}" }
                }
                if changed {
                    p { class: "small panel-note", style: "color: var(--warn)",
                        "You changed some settings from {entry.preset.name}."
                    }
                }
            } else {
                p { class: "muted small panel-note", "Pick Manga, Manhwa or Manhua to start from, or keep your own settings." }
            }
            details { class: "panel-details",
                summary { "How do the presets differ?" }
                for e in entries.iter().filter(|e| e.builtin) {
                    p { class: "small panel-note", key: "{e.preset.id}",
                        strong { "{e.preset.name}. " }
                        "{e.preset.description}"
                    }
                }
            }
            if advanced {
                if changed {
                    div { class: "row wrap", style: "margin-top: 8px",
                        button { class: "btn small", onclick: update_preset,
                            Icon { name: "check", size: 14 }
                            "Save changes to the preset"
                        }
                        if let Some(undo) = undo {
                            button { class: "btn ghost small", onclick: undo,
                                Icon { name: "rotate-ccw", size: 14 }
                                "Undo changes"
                            }
                        }
                    }
                }
                if naming() {
                    div { class: "row", style: "margin-top: 8px",
                        input {
                            r#type: "text", class: "grow", placeholder: "Name your preset",
                            maxlength: "40", autofocus: true, value: "{name}",
                            oninput: move |e| name.set(e.value()),
                        }
                        button { class: "btn primary small", onclick: save_new, "Save" }
                        button { class: "btn ghost small", onclick: move |_| { naming.set(false); error.set(None); }, "Cancel" }
                    }
                    if let Some(message) = error() {
                        p { class: "small", style: "color: var(--bad); margin: 6px 0 0", "{message}" }
                    }
                } else {
                    div { class: "row wrap", style: "margin-top: 8px",
                        button { class: "btn ghost small", onclick: move |_| naming.set(true),
                            Icon { name: "plus", size: 14 }
                            "Save as a new preset"
                        }
                        button { class: "btn ghost small", onclick: move |_| state.go(Page::Settings),
                            "Manage presets"
                        }
                    }
                }
            }
        }
    }
}
