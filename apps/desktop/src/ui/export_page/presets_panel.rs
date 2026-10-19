//! The Presets panel: how the series was last exported, the built-in presets and your own.

use super::{active_series, Panel};
use crate::logic;
use crate::state::{AppState, Modal, ModalAction};
use crate::ui::presets;
use crate::ui::widgets::{Chip, Icon};
use dioxus::prelude::*;
use library::ConversionSettings;

/// What a preset in the list is.
#[derive(Clone, Copy, PartialEq)]
enum PresetKind {
    /// How the series was last exported.
    Last,
    /// One that comes with the app: it cannot be changed.
    Builtin,
    /// One the user made: it can be saved to, renamed and deleted.
    Mine,
}

/// One preset in the list. Choosing it opens it, showing what it is for; one of
/// your own shows its name as a box to rename it by, and a save and a delete
/// button while it is chosen or pointed at.
#[component]
fn PresetItem(
    kind: PresetKind,
    /// `None` for how a series was exported without a preset.
    preset_id: Option<String>,
    name: String,
    description: String,
    settings: ConversionSettings,
    on: bool,
    series_id: String,
    series_settings: ConversionSettings,
) -> Element {
    let state = use_context::<AppState>();
    let differs = settings != series_settings;
    let edited = on && differs;
    let class = match (kind, on) {
        (PresetKind::Last, true) => "px-preset last on",
        (PresetKind::Last, false) => "px-preset last",
        (_, true) => "px-preset on",
        (_, false) => "px-preset",
    };
    let load = {
        let (sid, pid, set, label) = (series_id.clone(), preset_id.clone(), settings.clone(), name.clone());
        move |_| {
            if differs {
                state.ask(Modal {
                    title: format!("Load “{label}”?"),
                    body: "Loading a preset replaces this series' current settings with the preset's. Changes you made and did not save to a preset are lost.".into(),
                    confirm: "Load".into(),
                    cancel: "Cancel".into(),
                    danger: false,
                    action: ModalAction::LoadPreset { series_id: sid.clone(), preset_id: pid.clone(), settings: set.clone() },
                });
            } else {
                presets::apply_to_series(state, &sid, pid.clone(), set.clone());
            }
        }
    };
    let (save_id, save_name, save_settings) = (preset_id.clone().unwrap_or_default(), name.clone(), series_settings.clone());
    let (delete_id, delete_name) = (preset_id.clone().unwrap_or_default(), name.clone());
    let rename_id = preset_id.clone().unwrap_or_default();
    rsx! {
        div { class: "{class}", onclick: load,
            div { class: "px-preset-head",
                if kind == PresetKind::Mine {
                    input {
                        class: "px-name", r#type: "text", maxlength: "40", value: "{name}",
                        readonly: !on, "aria-label": "Preset name",
                        title: if on { "Click to rename" } else { "" },
                        // Once chosen, the name is the box to rename it by.
                        onclick: move |e| { if on { e.stop_propagation(); } },
                        onchange: move |e| {
                            let result = state.edit_presets(|p| p.rename(&rename_id, e.value().trim()));
                            match result {
                                Ok(()) => state.save_presets(),
                                Err(err) => state.error(err.to_string()),
                            }
                        },
                    }
                } else {
                    span { class: "grow px-item-name", "{name}" }
                }
                if edited { Chip { text: "edited", kind: Some("warn".to_string()) } }
                if kind == PresetKind::Mine {
                    div { class: "px-actions",
                        button { class: "px-icon", title: "Save this series' settings to this preset",
                            onclick: move |e| {
                                e.stop_propagation();
                                state.ask(Modal {
                                    title: format!("Save to “{save_name}”?"),
                                    body: "The preset is replaced with this series' current settings. Every series that uses the preset follows it.".into(),
                                    confirm: "Save".into(),
                                    cancel: "Cancel".into(),
                                    danger: false,
                                    action: ModalAction::SavePreset { id: save_id.clone(), settings: save_settings.clone() },
                                });
                            },
                            Icon { name: "save", size: 15 }
                        }
                        button { class: "px-icon danger", title: "Delete this preset",
                            onclick: move |e| {
                                e.stop_propagation();
                                state.ask(Modal {
                                    title: format!("Delete “{delete_name}”?"),
                                    body: "The preset is removed. Series that used it keep their settings.".into(),
                                    confirm: "Delete".into(),
                                    cancel: "Keep".into(),
                                    danger: true,
                                    action: ModalAction::DeletePreset { id: delete_id.clone() },
                                });
                            },
                            Icon { name: "trash", size: 15 }
                        }
                    }
                }
                if on { Icon { name: "check", size: 14 } }
            }
            if on && !description.is_empty() {
                p { class: "px-desc", "{description}" }
            }
        }
    }
}

/// Whether the series is going by how it was last exported. That is so when it
/// is exactly as exported; and when it was changed since, so long as no preset
/// was picked in the meantime (a preset it follows shows "edited" instead). So
/// changes made and not exported are shown as edits to the last export, which
/// stays what the last real export was.
fn last_export_in_use(
    exported_preset: Option<&str>,
    exported: Option<&ConversionSettings>,
    series_preset: Option<&str>,
    series: &ConversionSettings,
) -> bool {
    exported.is_some() && exported_preset == series_preset && (exported == Some(series) || series_preset.is_none())
}

#[component]
pub(super) fn PresetsPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let entries = state.presets.read().all();
    // How this series was last exported, if it has been.
    let last = state
        .exports
        .read()
        .records
        .iter()
        .rev()
        .find(|r| r.series_id == series.id && r.settings.is_some())
        .cloned();
    let last_on = last.as_ref().is_some_and(|r| {
        last_export_in_use(
            r.preset_id.as_deref(),
            r.settings.as_ref(),
            series.preset_id.as_deref(),
            &series.settings,
        )
    });
    let series_id = series.id.clone();
    let settings_now = series.settings.clone();
    let builtins: Vec<_> = entries.iter().filter(|e| e.builtin).cloned().collect();
    let mine: Vec<_> = entries.iter().filter(|e| !e.builtin).cloned().collect();

    // A name for a new preset that no other has.
    let new_name = {
        let mut name = "My preset".to_string();
        let mut n = 2;
        while entries
            .iter()
            .any(|e| e.preset.name.eq_ignore_ascii_case(&name))
        {
            name = format!("My preset {n}");
            n += 1;
        }
        name
    };
    let new_series_id = series_id.clone();
    let new_settings = settings_now.clone();

    rsx! {
        Panel { title: "Presets", icon: "layers", id: "presets", height: 280,
            div { class: "px-list",
                if let Some(record) = last {
                    PresetItem {
                        key: "{series_id}-last-export",
                        kind: PresetKind::Last,
                        preset_id: record.preset_id.clone(),
                        name: "Last export",
                        description: format!(
                            "The settings this series was last exported with ({}), so the next part matches the first. Only an export changes it.",
                            logic::ago(crate::state::now_secs(), record.at)
                        ),
                        settings: record.settings.clone().unwrap_or_default(),
                        on: last_on,
                        series_id: series_id.clone(),
                        series_settings: settings_now.clone(),
                    }
                }
                for e in builtins.iter() {
                    PresetItem {
                        key: "{e.preset.id}",
                        kind: PresetKind::Builtin,
                        preset_id: Some(e.preset.id.clone()),
                        name: e.preset.name.clone(),
                        description: e.preset.description.clone(),
                        settings: e.preset.settings.clone(),
                        on: !last_on && series.preset_id.as_deref() == Some(e.preset.id.as_str()),
                        series_id: series_id.clone(),
                        series_settings: settings_now.clone(),
                    }
                }
                for e in mine.iter() {
                    PresetItem {
                        key: "{e.preset.id}",
                        kind: PresetKind::Mine,
                        preset_id: Some(e.preset.id.clone()),
                        name: e.preset.name.clone(),
                        description: e.preset.description.clone(),
                        settings: e.preset.settings.clone(),
                        on: !last_on && series.preset_id.as_deref() == Some(e.preset.id.as_str()),
                        series_id: series_id.clone(),
                        series_settings: settings_now.clone(),
                    }
                }
                button { class: "px-new", title: "Keep this series' settings as a preset of your own",
                    onclick: move |_| {
                        let result = state.edit_presets(|p| p.save_new(&new_name, new_settings.clone()));
                        match result {
                            Ok(id) => {
                                state.save_presets();
                                let mut library = state.library;
                                library.write().set_preset(&new_series_id, Some(id));
                                state.save_library();
                            }
                            Err(e) => state.error(e.to_string()),
                        }
                    },
                    Icon { name: "plus", size: 14 } "New preset"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_not_exported_show_as_edits_to_the_last_export_and_never_replace_it() {
        let exported = ConversionSettings::default();
        let mut edited = exported.clone();
        edited.contrast = 70;
        // As exported: in use. Changed since, with no preset picked: still the
        // last export, now edited.
        assert!(last_export_in_use(None, Some(&exported), None, &exported));
        assert!(last_export_in_use(None, Some(&exported), None, &edited));
        // A preset the series follows takes the place when it is not exact.
        assert!(last_export_in_use(Some("manga"), Some(&exported), Some("manga"), &exported));
        assert!(!last_export_in_use(Some("manga"), Some(&exported), Some("manga"), &edited));
        // Another preset picked, or nothing exported yet.
        assert!(!last_export_in_use(Some("manga"), Some(&exported), Some("manhwa"), &exported));
        assert!(!last_export_in_use(None, None, None, &exported));
    }
}
