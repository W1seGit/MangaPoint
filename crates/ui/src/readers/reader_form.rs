//! Editing the chosen reader: its name, screen, file types and firmware, and where the app can
//! send files to it (`Caps::send_to_reader`), how they get there.

use crate::widgets::{Help, Icon};
use crate::host::{use_ui, UiState};
use device_profiles::{DeviceError, Firmware};
use dioxus::prelude::*;
use library::ReaderSetup;
use super::default_setup_for;
use super::form_widgets::{FormatTags, MultiSeg};

/// The other edge of a screen when one edge is typed and the shape is kept.
fn keep_shape(typed: f64, old_typed: f64, old_other: f64) -> f64 {
    if old_typed <= 0.0 {
        return old_other;
    }
    typed * old_other / old_typed
}

/// A size in inches as typed text: up to three decimals, no trailing zeros.
fn inches_text(pixels: u32, ppi: u32) -> String {
    let text = format!("{:.3}", f64::from(pixels) / f64::from(ppi.max(1)));
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The pixels a typed size in inches comes to at `ppi`; 0 if it is not a number.
fn pixels_from(inches: &str, ppi: u32) -> u32 {
    let inches = inches.trim().parse::<f64>().unwrap_or(0.0);
    (inches * f64::from(ppi)).round().clamp(0.0, 1_000_000.0) as u32
}

/// What the reader form holds before it is saved.
#[derive(Clone, PartialEq, Debug)]
struct ReaderDraft {
    name: String,
    width: u32,
    height: u32,
    /// Pixels per inch: pages are made at the screen's size in inches times this.
    ppi: u32,
    /// The screen shows colour: an EPUB or CBZ can then be made in colour.
    color: bool,
    /// How the reader receives files, the file types it opens, and its firmware.
    setup: ReaderSetup,
}

/// Save a draft over the chosen reader. Nothing changes if the result is not
/// allowed.
fn apply_reader(settings: &mut library::Settings, draft: &ReaderDraft) -> Result<(), DeviceError> {
    let others = settings.devices();
    let base = settings.device_profile();
    let mut updated = base.clone();
    updated.name = draft.name.trim().to_string();
    updated.width = draft.width;
    updated.height = draft.height;
    updated.ppi = draft.ppi;
    updated.color = draft.color;
    updated.validate(&others)?;
    if let Some(slot) = settings.custom_devices.iter_mut().find(|d| d.id == base.id) {
        *slot = updated;
    }
    settings.reader_setups.insert(base.id, draft.setup.clone());
    Ok(())
}

/// Write a reader draft over the chosen reader and start the form again from
/// it. False, with the reason shown, if it is not allowed.
fn save_reader(state: UiState, draft: &ReaderDraft, revision: &mut Signal<u32>) -> bool {
    let mut settings = state.settings;
    let result = apply_reader(&mut settings.write(), draft);
    match result {
        Ok(()) => {
            state.save_settings();
            // The screens that make pages for this reader follow what was saved (its size, its
            // formats, whether its screen has colour).
            state.host().reader_saved();
            state.notify("Reader saved.");
            let again = *revision.peek() + 1;
            revision.set(again);
            true
        }
        Err(e) => {
            state.error(e.to_string());
            false
        }
    }
}

/// The chosen reader, edited as a draft and written when Save is pressed.
#[component]
pub(super) fn ReaderForm(revision: Signal<u32>) -> Element {
    let state = use_ui();
    let s = state.settings.read().clone();
    let current = s.device_profile();
    let saved = ReaderDraft {
        name: current.name.clone(),
        width: current.width,
        height: current.height,
        ppi: current.ppi,
        color: current.color,
        setup: s.reader_setup(),
    };
    let mut setup = use_signal(|| saved.setup.clone());
    let mut testing = use_signal(|| false);
    let mut name = use_signal(|| saved.name.clone());
    // The size is typed in inches. Until it is touched, the pixels stay exactly
    // what was saved (inches rounded to three places would drift them).
    let mut width = use_signal(|| inches_text(saved.width, saved.ppi));
    let mut height = use_signal(|| inches_text(saved.height, saved.ppi));
    let mut ppi = use_signal(|| saved.ppi.to_string());
    let mut color = use_signal(|| saved.color);
    let mut touched = use_signal(|| false);
    let mut locked = use_signal(|| false);

    // What the form holds now. A memo, so the effect below follows every change.
    let saved_for_draft = saved.clone();
    let form_now = use_memo(move || {
        let draft_ppi = ppi().trim().parse::<u32>().unwrap_or(0);
        let (draft_w, draft_h) = if touched() {
            (pixels_from(&width(), draft_ppi), pixels_from(&height(), draft_ppi))
        } else {
            (saved_for_draft.width, saved_for_draft.height)
        };
        ReaderDraft { name: name(), width: draft_w, height: draft_h, ppi: draft_ppi, color: color(), setup: setup() }
    });
    let draft = form_now();
    let dirty = draft != saved;
    // While there are changes that are not saved, the rest of the app knows, and
    // asks before the user leaves them behind.
    {
        let saved = saved.clone();
        use_effect(move || {
            let draft = form_now();
            let mut unsaved = state.unsaved_reader;
            if draft != saved {
                unsaved.set(Some(crate::host::UnsavedReader(std::rc::Rc::new(move || {
                    let mut revision = revision;
                    save_reader(state, &draft, &mut revision)
                }))));
            } else if unsaved.peek().is_some() {
                unsaved.set(None);
            }
        });
        use_drop(move || {
            let mut unsaved = state.unsaved_reader;
            unsaved.set(None);
        });
    }
    let send_to_reader = state.host().caps().send_to_reader;
    let wifi_url_now = setup().wifi_url.clone().unwrap_or_default();
    let wifi_url_to_test = wifi_url_now.clone();
    let (w0, h0) = (
        f64::from(saved.width) / f64::from(saved.ppi.max(1)),
        f64::from(saved.height) / f64::from(saved.ppi.max(1)),
    );
    let remove_id = current.id.clone();
    let known = device_profiles::known(&current.id);
    let info = device_profiles::known_info(&current.id);
    // CrossPoint runs on the Xteink readers; the others (and any reader that
    // is not on the list, which might be anything) are offered what they came with.
    let crosspoint_runs = info.is_none_or(|i| i.crosspoint);
    let note = info.map(|i| i.note).unwrap_or("");
    let reset_id = current.id.clone();
    let lock_class = if locked() { "btn icon chain on" } else { "btn icon chain" };
    let lock_icon = if locked() { "link" } else { "unlink" };
    let lock_title = if locked() {
        "Width and height are linked: changing one changes the other. Click to unlink."
    } else {
        "Link width and height, so changing one changes the other"
    };
    let pressed = if locked() { "true" } else { "false" };

    rsx! {
        div { class: "reader-line",
            input {
                class: "title-input", r#type: "text", maxlength: "40", value: "{name}",
                title: "Click to rename this reader", "aria-label": "Reader name",
                oninput: move |e| name.set(e.value()),
            }
            div { class: "size-boxes",
                div { class: "unit-box dim-box",
                    span { class: "pre", "W" }
                    input {
                        r#type: "number", step: "0.01", min: "0.5", value: "{width}",
                        "aria-label": "Screen width in inches", title: "Screen width in inches",
                        oninput: move |e| {
                            let typed = e.value();
                            touched.set(true);
                            if locked() {
                                if let Ok(w) = typed.trim().parse::<f64>() {
                                    height.set(format!("{:.3}", keep_shape(w, w0, h0)).trim_end_matches('0').trim_end_matches('.').to_string());
                                }
                            }
                            width.set(typed);
                        },
                    }
                    span { class: "unit", "in" }
                }
                button { class: "{lock_class}", title: "{lock_title}", "aria-pressed": "{pressed}",
                    onclick: move |_| locked.set(!locked()),
                    Icon { name: lock_icon, size: 18 }
                }
                div { class: "unit-box dim-box",
                    span { class: "pre", "H" }
                    input {
                        r#type: "number", step: "0.01", min: "0.5", value: "{height}",
                        "aria-label": "Screen height in inches", title: "Screen height in inches",
                        oninput: move |e| {
                            let typed = e.value();
                            touched.set(true);
                            if locked() {
                                if let Ok(h) = typed.trim().parse::<f64>() {
                                    width.set(format!("{:.3}", keep_shape(h, h0, w0)).trim_end_matches('0').trim_end_matches('.').to_string());
                                }
                            }
                            height.set(typed);
                        },
                    }
                    span { class: "unit", "in" }
                }
                div { class: "unit-box dim-box ppi-box",
                    input {
                        r#type: "number", step: "1", min: "50", value: "{ppi}",
                        "aria-label": "Pixels per inch", title: "How many pixels the screen has per inch",
                        oninput: move |e| { touched.set(true); ppi.set(e.value()); },
                    }
                    span { class: "unit", "ppi" }
                }
                Help { text: "The size of the reader's screen in inches, width and height, and how many pixels it packs into each inch (its density). Pages are made at inches times density, in pixels, so nothing is scaled again on the reader. The Xteink screens are 220 ppi." }
            }
        }

        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Color screen"
                    Help { text: "Pick Color if the reader's screen shows color (Kaleido, Gallery). An EPUB or CBZ for it can then keep the pages in color; the Export screen offers the choice. XTC and XTCH are always gray." }
                }
            }
            div { class: "sx-control",
                div { class: "seg", id: "reader-color", role: "group", "aria-label": "Color screen",
                    button { r#type: "button", id: "reader-color-no", class: if !color() { "on" } else { "" },
                        onclick: move |_| color.set(false), "No color" }
                    button { r#type: "button", id: "reader-color-yes", class: if color() { "on" } else { "" },
                        onclick: move |_| color.set(true), "Color" }
                }
            }
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "File formats"
                    Help { text: "The file types this reader opens. Type one and press Enter, or pick from the list." }
                }
            }
            div { class: "sx-control",
                FormatTags {
                    value: setup().formats.clone(),
                    on_change: move |list: Vec<String>| setup.write().formats = list,
                }
            }
        }
        if !note.is_empty() {
            p { class: "sx-note", "{note}" }
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Firmware"
                    Help { text: "What the reader runs. It decides which file types it can open." }
                }
            }
            div { class: "sx-control",
                select {
                    onchange: move |e| setup.write().firmware = if e.value() == "cross" { Firmware::CrossPoint } else { Firmware::Stock },
                    if crosspoint_runs {
                        option { value: "cross", selected: setup().firmware == Firmware::CrossPoint, "CrossPoint" }
                    }
                    option { value: "stock", selected: setup().firmware == Firmware::Stock || !crosspoint_runs, "Stock (the one it came with)" }
                }
            }
        }

        if send_to_reader {
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Export method"
                    Help { text: "How files can get to this reader. Pick every way that works for it; when you export you choose which one to use. For USB, plug the reader in (drive mode) or put its SD card in this computer, and pick the drive on the Export screen." }
                }
            }
            div { class: "sx-control",
                MultiSeg {
                    items: vec![("USB".to_string(), setup().usb), ("Wi-Fi".to_string(), setup().wifi)],
                    on_toggle: move |i: usize| {
                        // At least one stays on: there has to be a way to reach the reader.
                        let mut s = setup.write();
                        if i == 0 {
                            if !s.usb || s.wifi { s.usb = !s.usb; }
                        } else if !s.wifi || s.usb {
                            s.wifi = !s.wifi;
                        }
                    },
                }
            }
        }
        if setup().wifi {
            p { class: "sx-note warn",
                strong { "Wi-Fi is not recommended. " }
                "The files can be very large, the reader takes them very slowly (a whole series can take an hour or more), and sending too much at once can freeze the reader. "
                "It has not been tested much, so use it at your own risk. USB, or the reader's SD card in your computer, is much safer."
            }
            div { class: "sx-row",
                div { class: "sx-text",
                    div { class: "sx-label", "Reader address"
                        Help { text: "The address the reader shows on its screen when its Wi-Fi file transfer is on, for example http://192.168.3.3" }
                    }
                }
                div { class: "sx-control sx-inline",
                    input {
                        r#type: "text", placeholder: "http://192.168.3.3", value: "{wifi_url_now}",
                        oninput: move |e| {
                            let v = e.value();
                            setup.write().wifi_url = (!v.trim().is_empty()).then(|| v.trim().to_string());
                        },
                    }
                    button { class: "btn", disabled: testing() || wifi_url_now.trim().is_empty(),
                        onclick: move |_| {
                            let url = wifi_url_to_test.clone();
                            spawn(async move {
                                testing.set(true);
                                match state.host().check_wifi(url).await {
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
        }
        div { class: "sx-row",
            div { class: "sx-text",
                div { class: "sx-label", "Folder on the reader"
                    Help { text: "Files go into this folder on the reader. It is made if it is not there." }
                }
            }
            div { class: "sx-control",
                input {
                    r#type: "text", value: "{setup().folder}",
                    oninput: move |e| setup.write().folder = e.value(),
                }
            }
        }
        }

        div { class: "form-footer",
            button { class: "btn danger",
                title: "Series made for it keep their settings.",
                onclick: {
                    let remove_name = current.name.clone();
                    move |_| state.ask(crate::host::Ask {
                        title: format!("Remove {remove_name}?"),
                        body: "The reader and its settings are taken off your list. Series made for it keep their settings.".into(),
                        confirm: "Remove".into(),
                        cancel: "Keep".into(),
                        danger: true,
                        action: crate::host::SharedAction::RemoveReader { id: remove_id.clone() },
                    })
                },
                Icon { name: "trash", size: 15 }
                "Remove this reader"
            }
            if let Some(known) = known {
                button { class: "btn",
                    title: "Put the name, size and file settings back to how this reader comes. Nothing is kept until you press Save.",
                    onclick: move |_| {
                        name.set(known.name.clone());
                        width.set(inches_text(known.width, known.ppi));
                        height.set(inches_text(known.height, known.ppi));
                        ppi.set(known.ppi.to_string());
                        color.set(known.color);
                        touched.set(false);
                        setup.set(default_setup_for(&reset_id));
                    },
                    Icon { name: "rotate-ccw", size: 15 }
                    "Reset to default"
                }
            }
            span { class: "grow" }
            button { class: "btn primary", disabled: !dirty,
                onclick: {
                    let draft = draft.clone();
                    let mut revision = revision;
                    move |_| { save_reader(state, &draft, &mut revision); }
                },
                Icon { name: "check", size: 16 }
                "Save"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::adder::add_known;

    fn draft(name: &str, width: u32, height: u32) -> ReaderDraft {
        ReaderDraft {
            name: name.into(),
            width,
            height,
            ppi: 220,
            color: false,
            setup: ReaderSetup::default(),
        }
    }

    #[test]
    fn a_locked_shape_follows_the_size_typed() {
        assert_eq!(keep_shape(1.0, 2.0, 4.0), 2.0);
        assert!((keep_shape(1.5, 2.18, 3.64) - 2.505).abs() < 0.01);
        assert_eq!(keep_shape(1.0, 0.0, 3.0), 3.0, "no old size to follow");
    }

    #[test]
    fn sizes_in_inches_come_back_as_the_same_pixels() {
        assert_eq!(inches_text(480, 220), "2.182");
        assert_eq!(inches_text(528, 256), "2.062");
        assert_eq!(inches_text(800, 200), "4");
        for (px, ppi) in [(480, 220), (800, 220), (528, 256), (792, 256), (1072, 300), (1448, 300)] {
            assert_eq!(pixels_from(&inches_text(px, ppi), ppi), px, "{px} at {ppi}");
        }
        assert_eq!(pixels_from("abc", 220), 0);
        assert_eq!(pixels_from("3.64", 220), 801);
    }

    #[test]
    fn the_colour_screen_mark_is_saved_with_the_reader() {
        let mut s = library::Settings::default();
        add_known(&mut s, device_profiles::known("kobo-libra-colour").unwrap()).unwrap();
        assert!(s.device_profile().color, "a known colour reader is added with its mark");
        let mut unmarked = draft("Kobo Libra Colour", 1264, 1680);
        unmarked.ppi = 300;
        apply_reader(&mut s, &unmarked).unwrap();
        assert!(!s.device_profile().color, "the box can be unticked");
        unmarked.color = true;
        apply_reader(&mut s, &unmarked).unwrap();
        assert!(s.device_profile().color);
    }

    #[test]
    fn a_reader_is_changed_in_place_and_a_bad_size_changes_nothing() {
        let mut s = library::Settings::default();
        add_known(&mut s, device_profiles::x4_pro()).unwrap();
        apply_reader(&mut s, &draft("  Travel  ", 300, 500)).unwrap();
        let now = s.device_profile();
        assert_eq!(
            (now.id.as_str(), now.name.as_str(), now.width),
            (device_profiles::X4_PRO_ID, "Travel", 300)
        );
        assert_eq!(s.devices().len(), 1);
        let before = s.clone();
        assert!(apply_reader(&mut s, &draft("Travel", 5, 500)).is_err());
        assert_eq!(s.device_profile().width, before.device_profile().width);
        assert!(apply_reader(&mut s, &draft("  ", 300, 500)).is_err());
    }
}
