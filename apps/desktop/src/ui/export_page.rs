//! The Export screen: full window. Editing the series is part of exporting
//! them, so this is where it happens. Presets, the editing controls and the
//! list of series being exported are panels on the left, the pages in the
//! middle, and the chapters and where to send them on the right. Every panel
//! folds away to give the others room.

use super::conversion::ConversionControls;
use super::devices::ReaderPicker;
use super::presets;
use super::widgets::{pick_folder, Chip, Cover, Help, Icon, NoCover};
use super::work::{
    activate_export_series, add_export_series, load_saved_cover, open_sample, reconcile_export_plan, remove_export_series,
    saved_cover_key, schedule_preview, start_export_plan,
};
use super::workspace::crop_editor::CropInspector;
use super::workspace::preview::{switch_chapter, PreviewTab};
use crate::logic::{self, plural, TargetChoice};
use crate::state::{AppState, GroupingChoice, JobKind, Modal, ModalAction};
use dioxus::prelude::*;
use library::{ConversionSettings, CropRect, DownloadedSeries, OutputFormat};
use pipeline::export::removable_drives;
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
                    PresetsPanel {}
                    Split { target: "presets", dir: 1 }
                    EditPanel {}
                    Split { target: "series", dir: -1 }
                    SeriesPanel { on_add: move |_| adding.set(true) }
                    div { class: "px-split-v", "data-split": "left", title: "Drag to make this column wider or narrower" }
                }
                main { class: "export-center", PreviewTab {} }
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
const SPLIT_SCRIPT: &str = include_str!("../js/export-split.js");

// ---- left: presets -------------------------------------------------------

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

#[component]
fn PresetsPanel() -> Element {
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
    // The last export is the one chosen while the series is as it was exported
    // (and no other preset has been picked since).
    let last_on = last.as_ref().is_some_and(|r| {
        r.settings.as_ref() == Some(&series.settings) && r.preset_id == series.preset_id
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
                        description: "The settings this series was last exported with, so the next part matches the first.",
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

// ---- left: editing the series -----------------------------------------

#[component]
fn EditPanel() -> Element {
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

// ---- left: the series being exported ----------------------------------

#[component]
fn SeriesPanel(on_add: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    rsx! {
        Panel {
            title: "Series",
            icon: "book-open",
            id: "series",
            height: 150,
            actions: rsx! {
                button { class: "btn ghost small", title: "Add another series to this export", onclick: move |_| on_add.call(()),
                    Icon { name: "plus", size: 14 } "Add"
                }
            },
            div { class: "px-list",
                for (i, item) in plan.items.iter().enumerate() {
                    if let Some(series) = library.find(&item.series_id) {
                        {
                        let included = series.chapters.iter().filter(|c| !item.excluded.contains(&c.key)).count();
                        let has_cover = series.info.as_ref().is_some_and(|i| i.has_cover);
                        let on = i == plan.active;
                        let off = item.off;
                        rsx! {
                            div {
                                key: "{item.series_id}",
                                class: if on { "px-layer on" } else { "px-layer" },
                                onclick: move |_| { if !on { activate_export_series(state, i); } },
                                input {
                                    r#type: "checkbox", checked: !off, title: "Include this series in the export",
                                    onclick: move |e| e.stop_propagation(),
                                    onchange: move |e| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(i) { item.off = !e.checked(); }
                                    },
                                }
                                div { class: "px-layer-cover",
                                    if has_cover { Cover { key_id: saved_cover_key(&series.id) } } else { NoCover {} }
                                }
                                div { class: "grow px-layer-text",
                                    strong { title: "{series.title}", "{series.title}" }
                                    span { class: "muted small", "{included} of {series.chapters.len()} chapters" }
                                }
                                button {
                                    class: "btn ghost small", title: "Take it out of this export",
                                    onclick: move |e| { e.stop_propagation(); remove_export_series(state, i); },
                                    Icon { name: "x", size: 14 }
                                }
                            }
                        }
                        }
                    }
                }
            }
        }
    }
}

// ---- right: which chapters --------------------------------------------

/// The bar at the top of the Chapters panel: Every chapter, and a range to
/// export. Its own component, so the panel's function stays small enough for the
/// optimiser (a very large one crashed the compiler in release builds).
#[component]
fn ChapterRangeBar(active: usize, all_on: bool, keys: Vec<String>, numbers: Vec<Option<f64>>) -> Element {
    let state = use_context::<AppState>();
    let mut range_text = use_signal(String::new);
    let mut range_error = use_signal(|| None::<String>);
    let all_keys = keys.clone();
    let range_keys = keys;
    rsx! {
    div { class: "px-sticky",
            div { class: "px-all-row",
                label { class: "px-all",
                    input {
                        r#type: "checkbox", checked: all_on,
                        onchange: move |_| {
                            let mut plan = state.export_plan;
                            let mut guard = plan.write();
                            if let Some(item) = guard.items.get_mut(active) {
                                if all_on { item.excluded = all_keys.iter().cloned().collect(); } else { item.excluded.clear(); }
                            }
                            range_error.set(None);
                        },
                    }
                    span { "Every chapter" }
                }
                form {
                    class: "px-range",
                    onsubmit: move |e| {
                        e.prevent_default();
                        match logic::select_by_text(&range_text.read(), &numbers) {
                            Ok(found) if found.is_empty() => {
                                // Nothing changes: leaving the export empty by accident is worse.
                                range_error.set(Some("no chapter of this series has a number in that range".into()));
                            }
                            Ok(found) => {
                                range_error.set(None);
                                let mut plan = state.export_plan;
                                let mut guard = plan.write();
                                if let Some(item) = guard.items.get_mut(active) {
                                    item.excluded = range_keys
                                        .iter()
                                        .enumerate()
                                        .filter(|(i, _)| !found.contains(i))
                                        .map(|(_, k)| k.clone())
                                        .collect();
                                }
                            }
                            Err(msg) => range_error.set(Some(msg)),
                        }
                    },
                    input {
                        r#type: "text", placeholder: "Range: 1-20, 25, 30-",
                        title: "Export only these chapters. Press Enter or Select.",
                        value: "{range_text}",
                        oninput: move |e| range_text.set(e.value()),
                    }
                    button { class: "btn small", r#type: "submit", "Select" }
                }
            }
            if let Some(msg) = range_error() {
                p { class: "px-range-error", "{msg}" }
            }
        }
    }
}

#[component]
fn ChaptersPanel() -> Element {
    let state = use_context::<AppState>();
    let Some(series) = active_series(state) else {
        return rsx! {};
    };
    let active = state.export_plan.read().active;
    let excluded = state
        .export_plan
        .read()
        .items
        .get(active)
        .map(|i| i.excluded.clone())
        .unwrap_or_default();
    let included = series
        .chapters
        .iter()
        .filter(|c| !excluded.contains(&c.key))
        .count();
    let all_on = included == series.chapters.len();
    let all_keys: Vec<String> = series.chapters.iter().map(|c| c.key.clone()).collect();
    let numbers: Vec<Option<f64>> = series.chapters.iter().map(|c| c.number).collect();
    let shown = state.preview.read().chapter_key.clone();
    // Chapters of an earlier export are marked.
    let exported = state.exports.read().exported_keys(&series.id);

    rsx! {
        Panel {
            title: "Chapters",
            icon: "list-checks",
            id: "chapters",
            actions: rsx! { span { class: "muted small", "{included} of {series.chapters.len()}" } },
            // Stays at the top while the chapters scroll under it.
            ChapterRangeBar { active, all_on, keys: all_keys.clone(), numbers }
            div { class: "px-chapters",
                for c in series.chapters.iter() {
                    {
                        let key = c.key.clone();
                        let show_key = c.key.clone();
                        let on = !excluded.contains(&c.key);
                        let showing = shown.as_deref() == Some(c.key.as_str());
                        rsx! {
                            div { key: "{c.key}",
                                class: match (on, showing) {
                                    (true, true) => "px-chapter on showing",
                                    (true, false) => "px-chapter on",
                                    (false, true) => "px-chapter showing",
                                    _ => "px-chapter",
                                },
                                input {
                                    r#type: "checkbox", checked: on, title: "Include this chapter in the export",
                                    onchange: move |_| {
                                        let mut plan = state.export_plan;
                                        let mut guard = plan.write();
                                        if let Some(item) = guard.items.get_mut(active) {
                                            if !item.excluded.remove(&key) { item.excluded.insert(key.clone()); }
                                        }
                                    },
                                }
                                button {
                                    class: "px-chapter-open", title: "Show this chapter in the preview",
                                    onclick: move |_| switch_chapter(state, show_key.clone()),
                                    span { class: "px-chapter-no", { c.number.map(logic::format_number).unwrap_or_else(|| "–".into()) } }
                                    span { class: "grow px-chapter-name", title: "{c.name}", "{c.name}" }
                                }
                                if exported.contains(&c.key) {
                                    span { class: "chip tiny", title: "Already in an export", "exported" }
                                }
                                span { class: "muted small", "{logic::human_bytes(c.bytes)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- right: where it goes and how it is split ------------------------

/// A drive that was found goes into the box that says where the reader is, but
/// only when the place chosen is not there (nothing chosen yet, or it was
/// unplugged): a drive picked by hand is never replaced by what turns up.
fn adopt_found(state: AppState, found: &[pipeline::export::Drive]) {
    let chosen_is_there = state
        .settings
        .peek()
        .reader_setup()
        .reader_dir
        .as_ref()
        .is_some_and(|d| d.exists());
    if chosen_is_there {
        return;
    }
    if let Some(drive) = found.first() {
        let mount = drive.mount.clone();
        let mut settings = state.settings;
        settings.write().update_reader_setup(|r| r.reader_dir = Some(mount));
        state.save_settings();
    }
}

/// What to tell about the place the reader is expected at.
#[derive(Debug, PartialEq, Eq)]
enum UsbStatus {
    /// It is one of the drives plugged in now (the index in the list).
    Found(usize),
    /// A folder or drive chosen by hand that is there, but not a plugged-in drive.
    Elsewhere,
    /// Chosen, but not there: unplugged, or not mounted.
    Gone,
    /// Nothing chosen yet and nothing found.
    Looking,
}

fn usb_status(chosen: Option<&std::path::Path>, found: &[pipeline::export::Drive]) -> UsbStatus {
    match chosen {
        None => UsbStatus::Looking,
        Some(path) => match found.iter().position(|d| d.mount == path) {
            Some(i) => UsbStatus::Found(i),
            None if path.exists() => UsbStatus::Elsewhere,
            None => UsbStatus::Gone,
        },
    }
}

const USB_HELP: &str = "Plug the reader in with USB (drive mode), or put its SD card in this computer. It shows up in the box by itself. With more than one drive, Choose picks the one to use.";
const WIFI_HELP: &str = "Switch on the reader's Wi-Fi file transfer, then type the address it shows. Test checks that it answers.";

/// What a format is, shown behind its question mark.
fn format_help(output: OutputFormat) -> &'static str {
    match output {
        OutputFormat::Xtc => "Black and white pages, made for the Xteink readers. The smallest files, and the quickest to open and turn. The one to pick if you are not sure.",
        OutputFormat::Xtch => "Four grays instead of two, also for the Xteink readers. Files are twice the size, and the reader is slower and laggier with them, so it is the less recommended choice.",
        OutputFormat::Epub => "An ordinary e-book that nearly every reader opens: Kindle (through Send to Kindle), Kobo, Boox, PocketBook, and the Xteink too. Each page is a picture the size of the reader's screen, made as small as it can be. Long series are cut into parts of 800 pages.",
        OutputFormat::Cbz => "A comic book archive. It carries the series, the author, the genres, the reading direction and a list of chapters, which comic readers show. Kobo, PocketBook, many Boox readers and KOReader open it; Kindle does not. Pages are pictures the size of the reader's screen.",
    }
}

#[component]
fn SendPanel() -> Element {
    let state = use_context::<AppState>();
    let starts_on_usb = matches!(state.export_opts.peek().target, TargetChoice::Usb);
    let mut drives = use_signal(move || if starts_on_usb { removable_drives() } else { Vec::new() });
    // Drives are only looked for while the reader is sent to by USB, every
    // couple of seconds, and not at all once this screen is left (the loop ends
    // with it).
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if !matches!(state.export_opts.peek().target, TargetChoice::Usb) {
                continue;
            }
            if let Ok(now) = tokio::task::spawn_blocking(removable_drives).await {
                adopt_found(state, &now);
                if *drives.peek() != now {
                    drives.set(now);
                }
            }
        }
    });
    // A drive that is already plugged in when the screen opens is taken at once.
    use_hook(move || adopt_found(state, &drives.peek()));
    let mut testing = use_signal(|| false);
    let mut choosing = use_signal(|| false);
    let opts = state.export_opts.read().clone();
    let settings = state.settings.read().clone();
    let mut export_opts = state.export_opts;
    let folder = settings
        .export
        .output_dir
        .clone()
        .unwrap_or_else(|| crate::state::paths().default_export_dir());
    let folder_text = folder.display().to_string();
    let setup = settings.reader_setup();
    let reader_dir = setup.reader_dir.clone();
    let reader_text = reader_dir.as_ref().map(|d| d.display().to_string());
    // Read here, not only in the menu, so the screen follows a drive being
    // plugged in or pulled out.
    let found = drives.read().clone();
    let usb = usb_status(reader_dir.as_deref(), &found);
    let on_reader = !matches!(opts.target, TargetChoice::Folder);
    // The way of reaching the reader: the one chosen if this reader takes it.
    let via = match opts.target {
        TargetChoice::WiFi if setup.wifi => library::ExportMethod::WiFi,
        _ if setup.usb => library::ExportMethod::Usb,
        _ => library::ExportMethod::WiFi,
    };
    let both = setup.usb && setup.wifi;
    let reader_folder = setup.folder.clone();
    // The book is made as something the reader opens: if what it was made as is
    // not listed (the list was changed), it switches to the first that is.
    use_effect(move || {
        let _ = state.settings.read().reader_setup().formats.len();
        presets::fit_output_to_reader(state);
    });
    // Another reader may not take the way that was chosen: follow what it does take.
    use_effect(move || {
        let setup = state.settings.read().reader_setup();
        let target = state.export_opts.read().target.clone();
        let fixed = match target {
            TargetChoice::Usb if !setup.usb => Some(TargetChoice::WiFi),
            TargetChoice::WiFi if !setup.wifi => Some(TargetChoice::Usb),
            _ => None,
        };
        if let Some(fixed) = fixed {
            let mut opts = state.export_opts;
            opts.write().target = fixed;
        }
    });
    let wifi_url = setup.wifi_url.clone().unwrap_or_default();
    let wifi_url_to_test = wifi_url.clone();
    let every_n_valid = opts.every_n.trim().parse::<usize>().is_ok_and(|n| n >= 1);
    // What the book is made as, and which of the three the reader lists.
    let current_output = active_series(state).map_or(OutputFormat::Xtc, |s| s.settings.output());
    let reader_formats = setup.formats.clone();

    rsx! {
        Panel { title: "Send to", icon: "file-down", id: "send", height: 740,
            div { class: "px-label", "Reader" }
            ReaderPicker {}
            div { class: "px-label", style: "margin-top: 16px", "Send to" }
            div { class: "seg stack",
                button {
                    class: if matches!(opts.target, TargetChoice::Folder) { "on" } else { "" },
                    onclick: move |_| export_opts.write().target = TargetChoice::Folder,
                    Icon { name: "folder-open", size: 16 } "Folder on this computer"
                }
                button {
                    class: if on_reader { "on" } else { "" },
                    onclick: move |_| {
                        if !on_reader {
                            export_opts.write().target = if setup.usb { TargetChoice::Usb } else { TargetChoice::WiFi };
                            spawn(async move {
                                if let Ok(now) = tokio::task::spawn_blocking(removable_drives).await {
                                    adopt_found(state, &now);
                                    drives.set(now);
                                }
                            });
                        }
                    },
                    Icon { name: "hard-drive", size: 16 } "On my reader"
                }
            }
            if matches!(opts.target, TargetChoice::Folder) {
                div { class: "folder-pick", style: "margin-top: 10px",
                    span { class: "grow folder-path", title: "{folder_text}", "{folder_text}" }
                    button { class: "btn small",
                        onclick: move |_| {
                            let start = Some(folder.clone());
                            spawn(async move {
                                if let Some(dir) = pick_folder("Where should the files go?", start).await {
                                    let mut s = state.settings;
                                    s.write().export.output_dir = Some(dir);
                                    state.save_settings();
                                }
                            });
                        },
                        "Choose folder…"
                    }
                }
            }
            if on_reader {
                div { class: "reader-way",
                    if both {
                        div { class: "seg",
                            button {
                                class: if via == library::ExportMethod::Usb { "on" } else { "" },
                                onclick: move |_| export_opts.write().target = TargetChoice::Usb,
                                "USB"
                                Help { text: USB_HELP }
                            }
                            button {
                                class: if via == library::ExportMethod::WiFi { "on" } else { "" },
                                onclick: move |_| export_opts.write().target = TargetChoice::WiFi,
                                "Wi-Fi"
                                Help { text: WIFI_HELP }
                            }
                        }
                    } else {
                        div { class: "px-label way-label",
                            if via == library::ExportMethod::Usb { "USB" } else { "Wi-Fi" }
                            Help { text: if via == library::ExportMethod::Usb { USB_HELP } else { WIFI_HELP } }
                        }
                    }
                    {match via {
                        library::ExportMethod::Usb => rsx! {
                            div { class: "menu-anchor",
                                div { class: "folder-pick",
                                    span { class: "grow folder-path", title: "{reader_text.clone().unwrap_or_default()}",
                                        if let Some(text) = &reader_text { "{text}" } else { "No drive found yet" }
                                    }
                                    button { class: "btn small", onclick: move |_| choosing.set(!choosing()), "Choose…" }
                                }
                                if choosing() {
                                    div { class: "menu-scrim", onclick: move |_| choosing.set(false) }
                                    div { class: "menu down",
                                        for d in drives.read().iter() {
                                            {
                                                let mount = d.mount.clone();
                                                let on = reader_text.as_deref() == Some(mount.display().to_string().as_str());
                                                let name = if d.name.trim().is_empty() { "Drive".to_string() } else { d.name.clone() };
                                                let label = format!("{} · {name} · {} free", mount.display(), logic::human_bytes(d.available_bytes));
                                                rsx! {
                                                    button {
                                                        key: "{label}", class: "menu-item",
                                                        onclick: move |_| {
                                                            let mut s = state.settings;
                                                            s.write().update_reader_setup(|r| r.reader_dir = Some(mount.clone()));
                                                            state.save_settings();
                                                            choosing.set(false);
                                                        },
                                                        Icon { name: "hard-drive", size: 15 }
                                                        span { class: "grow", "{label}" }
                                                        if on { Icon { name: "check", size: 14 } }
                                                    }
                                                }
                                            }
                                        }
                                        if drives.read().is_empty() {
                                            div { class: "menu-empty", "No drive found yet" }
                                        }
                                        button { class: "menu-item",
                                            onclick: move |_| {
                                                choosing.set(false);
                                                let start = reader_dir.clone();
                                                spawn(async move {
                                                    if let Some(dir) = pick_folder("Where is your reader?", start).await {
                                                        let mut s = state.settings;
                                                        s.write().update_reader_setup(|r| r.reader_dir = Some(dir));
                                                        state.save_settings();
                                                    }
                                                });
                                            },
                                            Icon { name: "folder-open", size: 15 }
                                            "Browse for a folder…"
                                        }
                                    }
                                }
                            }
                            {match &usb {
                                UsbStatus::Found(i) => {
                                    let d = &found[*i];
                                    let name = if d.name.trim().is_empty() { "your reader".to_string() } else { d.name.clone() };
                                    rsx! { div { class: "reader-note good",
                                        Icon { name: "check", size: 14 }
                                        " Found {name} ({logic::human_bytes(d.available_bytes)} free). It is selected for this export."
                                    } }
                                }
                                UsbStatus::Gone => rsx! { div { class: "reader-note bad", "The reader is not connected. Plug it in (drive mode), or choose another place." } },
                                UsbStatus::Looking => rsx! { div { class: "reader-note", "Looking for your reader. Plug it in with USB (drive mode) and it will be picked up here." } },
                                UsbStatus::Elsewhere => rsx! {},
                            }}
                        },
                        library::ExportMethod::WiFi => rsx! {
                            label { class: "px-field",
                                span { "Reader address" }
                                div { class: "row",
                                    input {
                                        class: "grow", r#type: "text", placeholder: "http://192.168.3.3", value: "{wifi_url}",
                                        onchange: move |e| {
                                            let v = e.value().trim().to_string();
                                            let mut settings = state.settings;
                                            settings.write().update_reader_setup(|r| r.wifi_url = (!v.is_empty()).then_some(v));
                                            state.save_settings();
                                        },
                                    }
                                    button { class: "btn small", disabled: testing() || wifi_url.trim().is_empty(),
                                        onclick: move |_| {
                                            let url = wifi_url_to_test.clone();
                                            spawn(async move {
                                                testing.set(true);
                                                match pipeline::export::check_webdav(&url).await {
                                                    Ok(()) => state.notify("The reader answered."),
                                                    Err(message) => state.error(message),
                                                }
                                                testing.set(false);
                                            });
                                        },
                                        if testing() { "Looking…" } else { "Test" }
                                    }
                                }
                            }
                            div { class: "reader-note warn",
                                Icon { name: "triangle-alert", size: 14 }
                                " Wi-Fi is slow: a whole series can take an hour or more to send. We recommend USB, or Wi-Fi for a few chapters at a time."
                            }
                        },
                    }}
                    label { class: "px-field",
                        span { "Folder on the reader" }
                        input {
                            r#type: "text", value: "{reader_folder}",
                            onchange: move |e| {
                                let v = e.value().trim().to_string();
                                if !v.is_empty() {
                                    let mut settings = state.settings;
                                    settings.write().update_reader_setup(|r| r.folder = v);
                                    state.save_settings();
                                }
                            },
                        }
                    }
                }
            }

            // What the book is made as: only the formats this reader lists (all
            // three if it lists none, so there is always something to pick). Each
            // has its explanation behind a question mark.
            div { class: "px-label", style: "margin-top: 16px", "Format" }
            div { class: "seg",
                for output in OutputFormat::ALL
                    .into_iter()
                    .filter(|o| reader_formats.is_empty() || reader_formats.iter().any(|f| f.eq_ignore_ascii_case(o.label())))
                {
                    {
                        let class = if output == current_output { "on" } else { "" };
                        rsx! {
                            button {
                                key: "{output.label()}", class,
                                onclick: move |_| presets::set_output(state, output),
                                "{output.label()}"
                                Help { text: format_help(output) }
                            }
                        }
                    }
                }
            }

            div { class: "px-label", style: "margin-top: 16px", "Files" }
            div { class: "seg stack",
                button {
                    class: if opts.grouping == GroupingChoice::Single { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::Single,
                    Icon { name: "book-open", size: 16 } "One file per series"
                }
                button {
                    class: if opts.grouping == GroupingChoice::PerChapter { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::PerChapter,
                    Icon { name: "layers", size: 16 } "One file per chapter"
                }
                button {
                    class: if opts.grouping == GroupingChoice::EveryN { "on" } else { "" },
                    onclick: move |_| export_opts.write().grouping = GroupingChoice::EveryN,
                    Icon { name: "list-checks", size: 16 } "One file every N chapters"
                }
            }
            if opts.grouping == GroupingChoice::EveryN {
                div { class: "row", style: "margin-top: 8px",
                    span { class: "muted small", "Every" }
                    input {
                        r#type: "text", style: "width: 64px; text-align: center", value: "{opts.every_n}",
                        oninput: move |e| export_opts.write().every_n = e.value(),
                    }
                    span { class: "muted small", "chapters" }
                    if !every_n_valid { span { class: "small", style: "color: var(--bad)", "Type a number" } }
                }
            }
        }
    }
}

#[component]
fn ExportFooter() -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    let opts = state.export_opts.read().clone();
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
        bytes += logic::estimate_output_bytes(p, series.settings.output());
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

// ---- adding a series --------------------------------------------------

#[component]
fn AddSeries(on_close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let plan = state.export_plan.read().clone();
    let library = state.library.read().clone();
    let available: Vec<&DownloadedSeries> = library
        .series
        .iter()
        .filter(|s| !s.chapters.is_empty() && !s.is_sample())
        .filter(|s| !plan.items.iter().any(|i| i.series_id == s.id))
        .collect();
    // Their covers are on disk, small: read them as soon as the list opens.
    let ids: Vec<String> = available.iter().map(|s| s.id.clone()).collect();
    use_effect(move || {
        for id in &ids {
            load_saved_cover(state, id);
        }
    });
    rsx! {
        div { class: "modal-head",
            h2 { "Add a series to this export" }
            button { class: "btn ghost small", title: "Close", onclick: move |_| on_close.call(()), Icon { name: "x", size: 16 } }
        }
        if available.is_empty() {
            p { class: "muted", "Every downloaded series is already in this export." }
        }
        for s in available {
            {
                let id = s.id.clone();
                let has_cover = s.info.as_ref().is_some_and(|i| i.has_cover);
                rsx! {
                    button { key: "{s.id}", class: "px-add-row",
                        onclick: move |_| { add_export_series(state, id.clone()); on_close.call(()); },
                        div { class: "px-layer-cover",
                            if has_cover { Cover { key_id: saved_cover_key(&s.id) } } else { NoCover {} }
                        }
                        div { class: "grow px-layer-text",
                            strong { "{s.title}" }
                            span { class: "muted small", "{plural(s.chapters.len(), \"chapter\")} · {logic::human_bytes(s.bytes())}" }
                        }
                        Icon { name: "plus", size: 16 }
                    }
                }
            }
        }
        div { class: "actions",
            button { class: "btn primary", onclick: move |_| on_close.call(()), "Done" }
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


#[cfg(test)]
mod tests {
    use super::*;
    use pipeline::export::Drive;
    use std::path::PathBuf;

    fn drive(mount: &std::path::Path) -> Drive {
        Drive {
            mount: mount.to_path_buf(),
            name: "XTEINK".into(),
            total_bytes: 8,
            available_bytes: 4,
        }
    }

    #[test]
    fn the_reader_place_is_told_apart_as_found_elsewhere_gone_or_not_yet_looked_for() {
        let here = std::env::temp_dir();
        let gone = PathBuf::from("/mangapoint-no-such-reader");
        assert_eq!(usb_status(None, &[]), UsbStatus::Looking);
        assert_eq!(usb_status(Some(&here), &[drive(&gone), drive(&here)]), UsbStatus::Found(1));
        assert_eq!(usb_status(Some(&here), &[]), UsbStatus::Elsewhere);
        assert_eq!(usb_status(Some(&gone), &[]), UsbStatus::Gone);
        // Unplugged: the drive is no longer listed and its place is not there.
        assert_eq!(usb_status(Some(&gone), &[drive(&here)]), UsbStatus::Gone);
    }
}
