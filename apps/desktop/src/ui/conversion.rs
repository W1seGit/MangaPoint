//! The controls for how pages are converted, shared by Settings (your
//! starting defaults) and the Preview tab (this one series).

use crate::state::AppState;
use crate::ui::widgets::Help;
use converter::process::{CropMode, Layout, Margins, PanelFinder, PanelView, SpreadMode, StripCut};
use crate::panel_ai::ModelStatus;
use crate::ui::widgets::ProgressBar;
use converter::xtc::ReadDirection;
use converter::Dither;
use dioxus::prelude::*;
use library::ConversionSettings;
use std::rc::Rc;

type Edit = fn(&mut ConversionSettings, String);

/// A percentage typed or dragged by the user, held to what the converter allows.
fn percent(raw: &str) -> u8 {
    raw.trim()
        .parse::<u8>()
        .unwrap_or(0)
        .min(Margins::MAX_PERCENT)
}

fn number(raw: &str, fallback: u8, max: u8) -> u8 {
    raw.trim().parse::<u8>().unwrap_or(fallback).min(max)
}

#[component]
pub fn ConversionControls(
    value: ConversionSettings,
    onchange: EventHandler<ConversionSettings>,
    compact: Option<bool>,
    shading_samples: Option<Vec<String>>,
    on_manual_crop: Option<EventHandler<()>>,
    manual_crop_active: Option<bool>,
    on_crop_mode: Option<EventHandler<CropMode>>,
) -> Element {
    let compact = compact.unwrap_or(false);
    let state = use_context::<AppState>();
    let advanced = state.settings.read().advanced_conversion;
    let c = value.clone();
    // Apply one edit to a copy of the settings and report the result.
    let change = Rc::new(move |edit: Edit, raw: String| {
        let mut v = value.clone();
        edit(&mut v, raw);
        onchange.call(v);
    });
    // A handler that feeds the control's value to `edit`.
    let on = |edit: Edit| {
        let ch = change.clone();
        move |e: Event<FormData>| ch(edit, e.value())
    };
    let on_check = |edit: Edit| {
        let ch = change.clone();
        move |e: Event<FormData>| ch(edit, e.checked().to_string())
    };

    let on_overlap = on(|s, v| s.overlap_percent = number(&v, 8, 30));
    let on_contrast = on(|s, v| s.contrast = number(&v, 55, 100));
    let on_levels = on_check(|s, v| s.auto_levels = v == "true");
    let on_sensitivity = on(|s, v| s.trim_sensitivity = number(&v, 50, 100));
    let on_padding = on(|s, v| s.trim_padding = number(&v, 4, 40));
    let on_top = on(|s, v| s.margins.top = percent(&v));
    let on_bottom = on(|s, v| s.margins.bottom = percent(&v));
    let on_left = on(|s, v| s.margins.left = percent(&v));
    let on_right = on(|s, v| s.margins.right = percent(&v));
    let ch = change.clone();
    let clear_margins = move |_| ch(|s, _| s.margins = Margins::default(), String::new());

    let class = if compact {
        "controls compact"
    } else {
        "controls"
    };
    let trimming = c.crop == CropMode::Margins;
    let strips = c.layout != Layout::Manga;
    let mut manual_local = use_signal(|| false);
    let manual = manual_crop_active.unwrap_or(*manual_local.read()) || !c.margins.is_none();
    let choices = |items: Vec<(&str, &str, &str, bool)>,
                   edit: Edit,
                   samples: Option<Vec<String>>| {
        items.into_iter().enumerate().map(|(i,(label,hint,diagram,selected))| {
            let ch = change.clone();
            let raw = diagram.to_string();
            let sample = samples.as_ref().and_then(|s| s.get(i)).cloned();
            rsx! { VisualChoice { label, hint, diagram, selected, sample, onclick: move |_| ch(edit, raw.clone()) } }
        }).collect::<Vec<Element>>()
    };
    let layout_tiles = choices(
        vec![
            (
                "Automatic",
                "Detect each image",
                "auto",
                c.layout == Layout::Auto,
            ),
            (
                "Manga pages",
                "One image per page",
                "pages",
                c.layout == Layout::Manga,
            ),
            (
                "Long strip",
                "Screen-sized slices",
                "strip",
                c.layout == Layout::Webtoon,
            ),
        ],
        |s, v| {
            s.layout = match v.as_str() {
                "pages" => Layout::Manga,
                "strip" => Layout::Webtoon,
                _ => Layout::Auto,
            }
        },
        None,
    );
    let cut_tiles = choices(
        vec![
            (
                "Repeat rows",
                "Nothing is cut off",
                "cutrepeat",
                c.strip_cut == StripCut::Overlap,
            ),
            (
                "Quiet spot",
                "Cut where little is drawn",
                "cutquiet",
                c.strip_cut == StripCut::Quiet,
            ),
        ],
        |s, v| {
            s.strip_cut = if v == "cutrepeat" {
                StripCut::Overlap
            } else {
                StripCut::Quiet
            }
        },
        None,
    );
    let spread_tiles = choices(
        vec![
            (
                "Split",
                "Two reader pages",
                "split",
                c.spread == SpreadMode::Split,
            ),
            (
                "Rotate",
                "Turn sideways",
                "rotate",
                c.spread == SpreadMode::Rotate,
            ),
            (
                "Fit",
                "Keep spread together",
                "fit",
                c.spread == SpreadMode::Fit,
            ),
        ],
        |s, v| {
            s.spread = match v.as_str() {
                "rotate" => SpreadMode::Rotate,
                "fit" => SpreadMode::Fit,
                _ => SpreadMode::Split,
            }
        },
        None,
    );
    let panel_tiles = choices(
        vec![
            (
                "Whole page",
                "As drawn",
                "panelsoff",
                c.panel_view == PanelView::Off,
            ),
            (
                "Panel by panel",
                "One panel per page",
                "panels",
                c.panel_view == PanelView::Panels,
            ),
            (
                "Page, then panels",
                "Overview first",
                "pagepanels",
                c.panel_view == PanelView::PageThenPanels,
            ),
        ],
        |s, v| {
            s.panel_view = match v.as_str() {
                "panels" => PanelView::Panels,
                "pagepanels" => PanelView::PageThenPanels,
                _ => PanelView::Off,
            }
        },
        None,
    );
    let finder_tiles = choices(
        vec![
            (
                "Gaps",
                "Framed panels, instant",
                "findgaps",
                c.panel_finder == PanelFinder::Gutters,
            ),
            (
                "AI model",
                "Any layout, 40 MB",
                "findai",
                c.panel_finder == PanelFinder::Model,
            ),
        ],
        |s, v| {
            s.panel_finder = if v == "findai" {
                PanelFinder::Model
            } else {
                PanelFinder::Gutters
            }
        },
        None,
    );
    let model_status = state.panel_model.read().clone();
    let direction_tiles = choices(
        vec![
            (
                "Right to left",
                "Manga",
                "rtl",
                c.direction == ReadDirection::RightToLeft,
            ),
            (
                "Left to right",
                "Comics / manhwa",
                "ltr",
                c.direction == ReadDirection::LeftToRight,
            ),
        ],
        |s, v| {
            s.direction = if v == "ltr" {
                ReadDirection::LeftToRight
            } else {
                ReadDirection::RightToLeft
            }
        },
        None,
    );
    let shade_tiles = choices(
        vec![
            (
                "Smooth",
                "Floyd-Steinberg",
                "smooth",
                c.dither == Dither::FloydSteinberg,
            ),
            (
                "High contrast",
                "Atkinson",
                "atkinson",
                c.dither == Dither::Atkinson,
            ),
            (
                "Patterned",
                "Ordered",
                "ordered",
                c.dither == Dither::Ordered,
            ),
            (
                "None",
                "Crisp line art",
                "none",
                c.dither == Dither::Threshold,
            ),
        ],
        |s, v| {
            s.dither = match v.as_str() {
                "atkinson" => Dither::Atkinson,
                "ordered" => Dither::Ordered,
                "none" => Dither::Threshold,
                _ => Dither::FloydSteinberg,
            }
        },
        shading_samples,
    );
    let keep = change.clone();
    let trim = change.clone();
    let reset = change.clone();

    rsx! {
        div { class: "{class} mp-conversion",
            h3 { class: "group", "Layout" }
            div { class: "mp-choice-grid", role: "group", aria_label: "Page layout", {layout_tiles.into_iter()} }
            h4 { class: "mp-control-heading", "Reading direction" }
            div { class: "mp-choice-grid two", role: "group", aria_label: "Reading direction", {direction_tiles.into_iter()} }
            h4 { class: "mp-control-heading", "Double-page spreads" }
            div { class: "mp-choice-grid", role: "group", aria_label: "Double-page spreads", {spread_tiles.into_iter()} }
            if c.layout != Layout::Webtoon {
                h4 { class: "mp-control-heading",
                    "Panels"
                    Help { text: "Cuts each page at the gaps between its panels and shows one panel per page, in reading order, as large as the screen allows. A page whose panels cannot be told apart (a splash page, or panels joined by a drawing) stays whole. Long strips are not cut into panels." }
                }
                div { class: "mp-choice-grid", role: "group", aria_label: "Panels", {panel_tiles.into_iter()} }
                if c.panel_view != PanelView::Off {
                    h4 { class: "mp-control-heading",
                        "How panels are found"
                        Help { text: "Gaps finds framed panels by the white space between them: instant, and right for most manga. The AI model (a 40 MB download, once) also finds panels without frames, odd shapes and panels a drawing runs across. It takes about a second per page, shared over your processor's cores." }
                    }
                    div { class: "mp-choice-grid two", role: "group", aria_label: "How panels are found", {finder_tiles.into_iter()} }
                    if c.panel_finder == PanelFinder::Model {
                        match model_status {
                            ModelStatus::Ready => rsx! { p { class: "muted small", "AI model ready." } },
                            ModelStatus::Loading => rsx! { p { class: "muted small", "Loading the AI model…" } },
                            ModelStatus::Downloading(done) => rsx! {
                                div { class: "field",
                                    span { class: "muted small", "Downloading the AI model…" }
                                    ProgressBar { fraction: Some(done) }
                                }
                            },
                            ModelStatus::Missing => rsx! {
                                div { class: "field",
                                    span { class: "muted small", "Until the model is downloaded, panels are found by the gaps." }
                                    button { class: "btn primary small", onclick: move |_| crate::panel_ai::download(state), "Download the AI model (40 MB)" }
                                }
                            },
                            ModelStatus::Failed(message) => rsx! {
                                div { class: "field",
                                    span { class: "muted small", "{message}" }
                                    button { class: "btn small", onclick: move |_| crate::panel_ai::download(state), "Try again" }
                                }
                            },
                        }
                    }
                }
            }
            if strips {
                h4 { class: "mp-control-heading",
                    "Where long strips are cut"
                    Help { text: if c.strip_cut == StripCut::Quiet {
                        "Each page ends at the quietest row near its bottom: a gap, a panel edge or a flat area. No row is shown twice, but a page can end a little early and cut a drawing off."
                    } else {
                        "Each page ends at the full screen height, and its last rows repeat at the top of the next page, so nothing is cut off."
                    } }
                }
                div { class: "mp-choice-grid two", role: "group", aria_label: "Where long strips are cut", {cut_tiles.into_iter()} }
            }
            if advanced && strips && c.strip_cut == StripCut::Overlap {
                details { class: "mp-tuning",
                summary { "Strip tuning" }
                div { class: "field",
                    label { "Strip overlap ({c.overlap_percent}%)"
                        Help { text: "How much of the last screen repeats at the top of the next, so you don't lose your place." }
                    }
                    input { aria_label: "Strip overlap", r#type: "range", min: "0", max: "30", value: "{c.overlap_percent}", oninput: on_overlap }
                }
            }

            }
            h3 { class: "group",
                "Cropping"
                Help { text: "Manual cuts the edges: with percentages, or with handles on the page in the editor, for one page, a chapter or the whole series." }
            }
            div { class: "mp-choice-grid", role: "group", aria_label: "Cropping",
                VisualChoice { label: "Keep whole", hint: "All original artwork", diagram: "whole", selected: !trimming && !manual,
                    onclick: move |_| {
                        manual_local.set(false);
                        if let Some(on_mode) = on_crop_mode { on_mode.call(CropMode::Full); }
                        keep(|s, _| { s.crop = CropMode::Full; s.margins = Margins::default(); }, String::new());
                    } }
                VisualChoice { label: "Trim margins", hint: "Find blank borders", diagram: "trim", selected: trimming && !manual,
                    onclick: move |_| {
                        manual_local.set(false);
                        if let Some(on_mode) = on_crop_mode { on_mode.call(CropMode::Margins); }
                        trim(|s, _| { s.crop = CropMode::Margins; s.margins = Margins::default(); }, String::new());
                    } }
                VisualChoice { label: "Manual", diagram: "manual", selected: manual,
                    hint: if on_manual_crop.is_some() { "Edit on the canvas" } else { "Set edge percentages" },
                    onclick: move |_| {
                        manual_local.set(true);
                        if let Some(open) = on_manual_crop { open.call(()); }
                    } }
            }
            if advanced && trimming {
                details { class: "mp-tuning",
                summary { "Automatic trim tuning" }
                div { class: "field",
                    label { "Sensitivity ({c.trim_sensitivity})"
                        Help { text: "Higher notices fainter marks, so it trims less. Lower trims harder." }
                    }
                    input { aria_label: "Trim sensitivity", r#type: "range", min: "0", max: "100", value: "{c.trim_sensitivity}", oninput: on_sensitivity }
                }
                div { class: "field",
                    label { "Border kept ({c.trim_padding} px)"
                        Help { text: "Space left around the art after trimming." }
                    }
                    input { aria_label: "Border kept", r#type: "range", min: "0", max: "40", value: "{c.trim_padding}", oninput: on_padding }
                }
            }
            }
            if advanced || (manual && on_manual_crop.is_none()) {
            details { class: "mp-tuning", open: manual,
            summary { "Edge cuts - all pages" }
            div { class: "field",
                label { "Cut from the edges (%)"
                    Help { text: "Removes the same strip from every page before trimming. Handy for watermarks and site banners." }
                }
                div { class: "margins",
                    label { span { "Top" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.top}", oninput: on_top } }
                    label { span { "Bottom" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.bottom}", oninput: on_bottom } }
                    label { span { "Left" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.left}", oninput: on_left } }
                    label { span { "Right" } input { r#type: "number", min: "0", max: "40", value: "{c.margins.right}", oninput: on_right } }
                }
                if !c.margins.is_none() {
                    button { class: "btn ghost small", onclick: clear_margins, "Clear" }
                }
            }
            }
            }

            h3 { class: "group", "Appearance" }
            div { class: "field",
                label { "Contrast ({c.contrast})"
                    Help { text: "50 leaves the page as it is. A little higher (55) usually looks better on e-paper." }
                }
                div { class: "mp-contrast-track",
                    input { aria_label: "Contrast", r#type: "range", min: "0", max: "100", value: "{c.contrast}", oninput: on_contrast }
                    span { class: "mp-neutral-marker", title: "Neutral: 50" }
                }
                div { class: "mp-contrast-labels", span { "Softer" } span { "50 neutral" } span { "Stronger" } }
                button { class: "btn ghost small", disabled: c.contrast == 50, onclick: move |_| reset(|s,_| s.contrast = 50, String::new()), "Reset to neutral" }
            }
            h4 { class: "mp-control-heading",
                "Shading style"
                Help { text: "How grays are drawn with the few shades the reader has. The smoother the shading, the bigger the file, most of all in an EPUB: 'None' makes the smallest EPUB (about a third of Smooth), 'Smooth' the biggest." }
            }
            div { class: "mp-choice-grid two", role: "group", aria_label: "Shading style", {shade_tiles.into_iter()} }
            if advanced {
            details { class: "mp-tuning",
            summary { "Tone tuning" }
            div { class: "field",
                label { "Adjust tones" }
                label { class: "check-line",
                    input { r#type: "checkbox", checked: c.auto_levels, onchange: on_levels }
                    span { class: "muted", "Make paper white and ink black" }
                }
            }
            }

            }
            button { class: "btn ghost small advanced-toggle",
                onclick: move |_| {
                    let mut settings = state.settings;
                    let on = !settings.peek().advanced_conversion;
                    settings.write().advanced_conversion = on;
                    state.save_settings();
                },
                if advanced { "Hide advanced settings" } else { "Show advanced settings" }
            }
        }
    }
}

#[component]
fn VisualChoice(
    label: String,
    hint: String,
    diagram: String,
    selected: bool,
    onclick: EventHandler<MouseEvent>,
    sample: Option<String>,
) -> Element {
    rsx! {
        button { r#type: "button", class: if selected { "mp-choice selected" } else { "mp-choice" }, aria_pressed: selected, onclick,
            if let Some(src) = sample { img { class: "mp-shading-sample", src, alt: "" } }
            else { span { class: "mp-diagram {diagram}", aria_hidden: "true", i {} i {} i {} } }
            strong { "{label}" }
            span { class: "mp-choice-hint", "{hint}" }
        }
    }
}
