//! The controls for how pages are converted, shared by Settings (your
//! starting defaults) and the Preview tab (this one series).

use crate::state::AppState;
use crate::ui::widgets::{Help, Icon, ZoomDropdown, ZoomPick};
use converter::process::{
    CropMode, Layout, Margins, PageZoom, SpreadMode, StripCut, ZoomMode, ZOOM_FIT, ZOOM_MAX,
};
use converter::xtc::ReadDirection;
use converter::Dither;
use dioxus::prelude::*;
use library::{ConversionSettings, Orientation};
use std::rc::Rc;

type Edit = fn(&mut ConversionSettings, String);

/// A percentage typed or dragged by the user, held to what the converter allows.
fn percent(raw: &str) -> u8 {
    raw.trim()
        .parse::<u8>()
        .unwrap_or(0)
        .min(Margins::MAX_PERCENT)
}

/// The converter's zoom as the zoom menu shows it, and back.
fn zoom_pick(zoom: PageZoom) -> ZoomPick {
    match zoom.clamped() {
        PageZoom::Fit => ZoomPick::Fit,
        PageZoom::Width => ZoomPick::Width,
        PageZoom::Percent(p) => ZoomPick::Percent(p),
    }
}

/// A zoom choice as the text the change handlers pass around, and back.
fn zoom_code(pick: ZoomPick) -> String {
    match pick {
        ZoomPick::Fit => "fit".to_string(),
        ZoomPick::Width => "width".to_string(),
        ZoomPick::Percent(p) => p.to_string(),
    }
}

fn zoom_from_code(code: &str) -> PageZoom {
    match code {
        "fit" => PageZoom::Fit,
        "width" => PageZoom::Width,
        percent => PageZoom::Percent(percent.parse().unwrap_or(ZOOM_FIT)).clamped(),
    }
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
    /// The manual crop's own controls, shown in the Crop group when a manual
    /// crop is in use.
    crop_section: Option<Element>,
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

    let ch = change.clone();
    let on_zoom = move |pick: ZoomPick| {
        ch(|s, code| s.zoom = zoom_from_code(&code), zoom_code(pick))
    };
    let ch = change.clone();
    let zoom_pieces = move |_| ch(|s, _| s.zoom_mode = ZoomMode::Tile, String::new());
    let ch = change.clone();
    let zoom_crop = move |_| ch(|s, _| s.zoom_mode = ZoomMode::Crop, String::new());
    let on_overlap = on(|s, v| s.overlap_percent = number(&v, 8, 30));
    let on_contrast = on(|s, v| s.contrast = number(&v, 55, 100));
    let on_levels = on_check(|s, v| s.auto_levels = v == "true");
    let on_arrows = on_check(|s, v| s.guide_arrows = v == "true");
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
    let orientation_tiles = choices(
        vec![
            (
                "Portrait",
                "Reader held upright",
                "portrait",
                c.orientation == Orientation::Portrait,
            ),
            (
                "Landscape",
                "Reader held sideways",
                "landscape",
                c.orientation == Orientation::Landscape,
            ),
        ],
        |s, v| {
            s.orientation = if v == "landscape" {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            };
            // A wide page already fits a sideways screen, so cutting a
            // spread in two is rarely wanted there. Only the choice made
            // here; the spread setting can still be changed afterwards.
            if s.orientation == Orientation::Landscape && s.spread == SpreadMode::Split {
                s.spread = SpreadMode::Fit;
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

    // One group is open at a time, so the others stay as one line each.
    let open_group = use_signal(|| None::<&'static str>);
    let layout_note = layout_summary(&c);
    let spreads_note = spreads_summary(&c);
    let crop_note = crop_summary(&c, manual);
    let look_note = look_summary(&c);
    // Arrows only make sense where a page can carry on onto another.
    let arrows_apply = c.spread == SpreadMode::Split || !c.zoom.is_fit();

    rsx! {
        div { class: "{class} mp-conversion",
            ConversionGroup { title: "Layout", summary: layout_note, open_group,
                div { class: "mp-choice-grid", role: "group", aria_label: "Page layout", {layout_tiles.into_iter()} }
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
                h4 { class: "mp-control-heading", "Reading direction" }
                div { class: "mp-choice-grid two", role: "group", aria_label: "Reading direction", {direction_tiles.into_iter()} }
                h4 { class: "mp-control-heading",
                    "Orientation"
                    Help { text: if c.orientation == Orientation::Landscape {
                        "Pages are made wide, for a reader held sideways. For XTC and XTCH, turn the reader counter-clockwise so the top of the page is on its right. An EPUB or CBZ has wide pictures, and the reader's own rotation shows them."
                    } else {
                        "Pages are made tall, for a reader held upright."
                    } }
                }
                div { class: "mp-choice-grid two", role: "group", aria_label: "Orientation", {orientation_tiles.into_iter()} }
            }
            ConversionGroup { title: "Spreads and zoom", summary: spreads_note, open_group,
                h4 { class: "mp-control-heading", "Double-page spreads" }
                div { class: "mp-choice-grid", role: "group", aria_label: "Double-page spreads", {spread_tiles.into_iter()} }
                h4 { class: "mp-control-heading",
                    "Zoom"
                    Help { text: "Fit page shows the whole page. Fit width fills the screen width, so a tall page takes several screens. Or choose or type a percentage from 100 to 300 to zoom in. Cut into pieces shows the zoomed page as several screens in reading order, so nothing is lost. Crop to centre shows one screen from the middle and cuts the edges off. Long strips always fit the screen width." }
                }
                div { class: "mp-zoom",
                    ZoomDropdown {
                        selected: zoom_pick(c.zoom),
                        percents: vec![125u16, 150, 200, 250, 300],
                        min: ZOOM_FIT,
                        max: ZOOM_MAX,
                        onpick: on_zoom,
                    }
                    if !c.zoom.is_fit() {
                        div { class: "seg", role: "tablist", aria_label: "What zooming does",
                            button { r#type: "button", role: "tab", class: if c.zoom_mode == ZoomMode::Tile { "on" } else { "" }, aria_selected: c.zoom_mode == ZoomMode::Tile, onclick: zoom_pieces, "Cut into pieces" }
                            button { r#type: "button", role: "tab", class: if c.zoom_mode == ZoomMode::Crop { "on" } else { "" }, aria_selected: c.zoom_mode == ZoomMode::Crop, onclick: zoom_crop, "Crop to centre" }
                        }
                    }
                }
                if arrows_apply {
                    h4 { class: "mp-control-heading",
                        "Guide arrows"
                        Help { text: "Draws a small arrow in the bottom corner of a page that carries on onto the next one: the first half of a split double page, or a screen of a zoomed page. It points the way the next page lies, so it is clear how the page was cut. The last page of each has none." }
                    }
                    label { class: "check-line",
                        input { r#type: "checkbox", checked: c.guide_arrows, onchange: on_arrows }
                        span { class: "muted", "Arrow to the next part" }
                    }
                }
            }
            ConversionGroup { title: "Crop", summary: crop_note, open_group, reveal: manual && manual_crop_active.unwrap_or(false),
                h4 { class: "mp-control-heading",
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
                if manual {
                    if let Some(section) = crop_section {
                        div { class: "mp-manual-crop", {section} }
                    }
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
            }
            ConversionGroup { title: "Look", summary: look_note, open_group,
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

/// One folded part of the controls: a header that opens it, and when it is
/// closed a line saying what is chosen inside. Only one is open at a time
/// (`open_group` holds which). `reveal` opens it when something outside asks
/// to (a crop being edited).
#[component]
fn ConversionGroup(
    title: &'static str,
    summary: String,
    open_group: Signal<Option<&'static str>>,
    reveal: Option<bool>,
    children: Element,
) -> Element {
    let mut open_group = open_group;
    let reveal = reveal.unwrap_or(false);
    use_effect(use_reactive!(|reveal| {
        if reveal {
            open_group.set(Some(title));
        }
    }));
    let shown = open_group() == Some(title);
    rsx! {
        section { class: if shown { "mp-group open" } else { "mp-group" },
            button { class: "mp-group-head", r#type: "button", "aria-expanded": "{shown}",
                onclick: move |_| open_group.set(if shown { None } else { Some(title) }),
                Icon { name: "chevron-down", size: 15 }
                span { class: "mp-group-title", "{title}" }
                if !shown {
                    span { class: "mp-group-summary", title: "{summary}", "{summary}" }
                }
            }
            if shown {
                div { class: "mp-group-body", {children} }
            }
        }
    }
}

fn layout_summary(c: &ConversionSettings) -> String {
    let layout = match c.layout {
        Layout::Auto => "Automatic",
        Layout::Manga => "Manga pages",
        Layout::Webtoon => "Long strip",
    };
    let way = if c.orientation == Orientation::Landscape { "Landscape" } else { "Portrait" };
    let direction = if c.direction == ReadDirection::RightToLeft { "Right to left" } else { "Left to right" };
    format!("{layout} · {way} · {direction}")
}

fn spreads_summary(c: &ConversionSettings) -> String {
    let spread = match c.spread {
        SpreadMode::Split => "Split",
        SpreadMode::Rotate => "Rotate",
        SpreadMode::Fit => "Fit",
    };
    let mut text = format!("{spread} · {}", zoom_pick(c.zoom).label());
    if !c.zoom.is_fit() && c.zoom_mode == ZoomMode::Crop {
        text.push_str(" cropped");
    }
    if c.guide_arrows {
        text.push_str(" · arrows");
    }
    text
}

fn crop_summary(c: &ConversionSettings, manual: bool) -> String {
    if manual {
        "Manual crop"
    } else if c.crop == CropMode::Margins {
        "Trim margins"
    } else {
        "Keep whole"
    }
    .to_string()
}

fn look_summary(c: &ConversionSettings) -> String {
    let shading = match c.dither {
        Dither::FloydSteinberg => "Smooth",
        Dither::Atkinson => "High contrast",
        Dither::Ordered => "Patterned",
        Dither::Threshold => "No shading",
    };
    format!("Contrast {} · {shading}", c.contrast)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_group_says_what_is_chosen_inside_it() {
        let mut c = ConversionSettings::default();
        assert_eq!(layout_summary(&c), "Automatic · Portrait · Right to left");
        c.layout = Layout::Webtoon;
        c.orientation = Orientation::Landscape;
        c.direction = ReadDirection::LeftToRight;
        assert_eq!(layout_summary(&c), "Long strip · Landscape · Left to right");
        assert_eq!(spreads_summary(&c), "Split · Fit page");
        c.zoom = PageZoom::Percent(150);
        c.zoom_mode = ZoomMode::Crop;
        c.guide_arrows = true;
        assert_eq!(spreads_summary(&c), "Split · 150% cropped · arrows");
        c.crop = CropMode::Margins;
        assert_eq!(crop_summary(&c, false), "Trim margins");
        assert_eq!(crop_summary(&c, true), "Manual crop");
        c.crop = CropMode::Full;
        assert_eq!(crop_summary(&c, false), "Keep whole");
        assert_eq!(look_summary(&c), "Contrast 55 · Smooth");
    }

    #[test]
    fn the_zoom_menu_and_the_converter_agree_on_every_choice() {
        for zoom in [PageZoom::Fit, PageZoom::Width, PageZoom::Percent(150)] {
            assert_eq!(zoom_from_code(&zoom_code(zoom_pick(zoom))), zoom);
        }
        // A hundred percent is Fit, and a zoom past the limit is held to it.
        assert_eq!(zoom_from_code("100"), PageZoom::Fit);
        assert_eq!(zoom_from_code("900"), PageZoom::Percent(ZOOM_MAX));
        assert_eq!(zoom_pick(PageZoom::Percent(100)), ZoomPick::Fit);
    }
}
