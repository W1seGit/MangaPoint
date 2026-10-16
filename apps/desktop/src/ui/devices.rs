//! Choosing the reader pages are made for, and adding your own.

use super::widgets::{Help, Icon};
use crate::state::AppState;
use device_profiles::{DeviceError, DeviceProfile, Firmware};
use dioxus::prelude::*;
use crate::state::{Page, SettingsSection};
use library::ReaderSetup;

/// The file types a reader can be said to open.
const FORMAT_OPTIONS: [&str; 3] = ["XTC", "XTCH", "EPUB"];

/// The formats as a row of pills inside a box: type one and press Enter, or
/// pick from the list that opens under the box.
#[component]
fn FormatTags(value: Vec<String>, on_change: EventHandler<Vec<String>>) -> Element {
    let mut text = use_signal(String::new);
    let mut focused = use_signal(|| false);
    let query = text().trim().to_uppercase();
    let free: Vec<&str> = FORMAT_OPTIONS
        .iter()
        .copied()
        .filter(|o| !value.iter().any(|v| v.eq_ignore_ascii_case(o)) && o.contains(query.as_str()))
        .collect();
    let with = {
        let value = value.clone();
        move |option: &str| {
            let mut next = value.clone();
            next.push(option.to_string());
            next.sort_by_key(|v| FORMAT_OPTIONS.iter().position(|o| o.eq_ignore_ascii_case(v)));
            next
        }
    };
    let first_free = free.first().map(|f| f.to_string());
    let on_enter = with.clone();
    let last_removable = value.last().cloned();
    let value_for_backspace = value.clone();
    rsx! {
        div { class: if focused() { "tags focused" } else { "tags" },
            for v in value.iter() {
                {
                    let name = v.clone();
                    let rest: Vec<String> = value.iter().filter(|x| *x != v).cloned().collect();
                    rsx! {
                        span { class: "tag", key: "{name}", role: "button", title: "Remove {name}",
                            onclick: move |_| on_change.call(rest.clone()),
                            "{name}"
                            span { class: "tag-x", Icon { name: "x", size: 11 } }
                        }
                    }
                }
            }
            input {
                r#type: "text", value: "{text}", spellcheck: "false", "aria-label": "Add a file format",
                placeholder: if value.is_empty() { "Add a format" } else { "" },
                onfocus: move |_| focused.set(true),
                onblur: move |_| focused.set(false),
                oninput: move |e| text.set(e.value()),
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        e.prevent_default();
                        if let Some(option) = &first_free {
                            on_change.call(on_enter(option));
                            text.set(String::new());
                        }
                    } else if e.key() == Key::Backspace && text().is_empty() && last_removable.is_some() {
                        let mut next = value_for_backspace.clone();
                        next.pop();
                        on_change.call(next);
                    }
                },
            }
            if focused() && !free.is_empty() {
                div { class: "tag-options",
                    for option in free.iter().copied() {
                        button {
                            key: "{option}", class: "tag-option", r#type: "button",
                            // Pressed before the box loses focus, so the list stays until it is used.
                            onmousedown: {
                                let with = with.clone();
                                move |e| {
                                    e.prevent_default();
                                    on_change.call(with(option));
                                    text.set(String::new());
                                }
                            },
                            "{option}"
                        }
                    }
                }
            }
        }
    }
}

/// Options of which any number can be on, looking like the one-of-several
/// rows but each with a box that is ticked when it is on.
#[component]
fn MultiSeg(items: Vec<(String, bool)>, on_toggle: EventHandler<usize>) -> Element {
    rsx! {
        div { class: "seg multi", role: "group",
            for (i, (label, on)) in items.iter().enumerate() {
                button {
                    key: "{label}", r#type: "button", role: "checkbox", "aria-checked": "{on}",
                    class: if *on { "on" } else { "" },
                    onclick: move |_| on_toggle.call(i),
                    span { class: "mark",
                        if *on { Icon { name: "check", size: 12 } }
                    }
                    "{label}"
                }
            }
        }
    }
}

/// How a reader starts out: the file types it opens and the firmware it runs,
/// from what is known of it. A reader of your own opens what MangaPoint makes
/// and runs its own firmware until you say otherwise.
pub(super) fn default_setup_for(id: &str) -> ReaderSetup {
    match device_profiles::known_info(id) {
        Some(info) => ReaderSetup {
            formats: info.formats.iter().map(|f| f.to_string()).collect(),
            firmware: if info.crosspoint { Firmware::CrossPoint } else { Firmware::Stock },
            ..ReaderSetup::default()
        },
        None => ReaderSetup {
            firmware: Firmware::Stock,
            ..ReaderSetup::default()
        },
    }
}

/// Add a known reader to the user's own and choose it. One already added is
/// just chosen.
fn add_known(settings: &mut library::Settings, known: DeviceProfile) -> Result<String, DeviceError> {
    let others = settings.devices();
    if !others.iter().any(|d| d.id == known.id) {
        let mut reader = known.clone();
        reader.builtin = false;
        reader.validate(&others)?;
        settings.custom_devices.push(reader);
        settings.reader_setups.insert(known.id.clone(), default_setup_for(&known.id));
    }
    settings.device = known.id.clone();
    Ok(known.id)
}

/// Make a reader of the user's own, with a size to be set afterwards, and
/// choose it.
fn add_custom(settings: &mut library::Settings, name: &str) -> Result<String, DeviceError> {
    let reader = DeviceProfile::custom(&settings.devices(), name, 480, 800)?;
    let id = reader.id.clone();
    settings.custom_devices.push(reader);
    settings.reader_setups.insert(id.clone(), default_setup_for(&id));
    settings.device = id.clone();
    Ok(id)
}

/// What the typed text points at: the known readers not yet added that answer
/// it, the closest first.
fn known_matches(query: &str, have: &[DeviceProfile]) -> Vec<DeviceProfile> {
    let mut found: Vec<(usize, DeviceProfile)> = device_profiles::builtin()
        .into_iter()
        .filter(|k| !have.iter().any(|d| d.id == k.id))
        .filter_map(|k| {
            // The maker counts too: "kindle", "amazon" and "boox" all find theirs.
            let named = format!("{} {}", k.maker, k.name);
            device_profiles::match_rank(query, &named).map(|r| (r, k))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, k)| k).collect()
}

/// A search box for adding a reader. The known readers that answer what is
/// typed are listed under it; Enter takes the closest, and when nothing
/// answers it makes a reader of your own with the typed name (shown in the
/// list as a row with a plus).
#[component]
pub fn DeviceAdder(
    on_added: EventHandler<String>,
    /// Called when the box is left empty, so the caller can put its button back.
    #[props(default)]
    on_close: Option<EventHandler<()>>,
    /// Keep the list open (the first-run setup), instead of while the box has focus.
    #[props(default = false)]
    always_open: bool,
    /// Wait this long before putting the cursor in the box. A page that is
    /// still sliding in must not be focused: the browser scrolls it into view
    /// and the page jumps.
    #[props(default = 0)]
    focus_after_ms: u64,
) -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let mut focused = use_signal(|| false);
    // The row that Enter would take: moved with the arrows or Tab, and back to
    // the first whenever the text changes.
    let mut active = use_signal(|| 0usize);
    // Keep the row Enter would take in view when the list is long. Only the
    // list scrolls: scrollIntoView would also scroll every page around it, and
    // the first-run page that is still sliding in sits far off to the side.
    use_effect(move || {
        let _ = active();
        spawn(async move {
            let _ = document::eval(
                "const o = document.querySelector('.adder-opt.active'); \
                 const l = o && o.closest('.adder-list'); \
                 if (l) { \
                   const lr = l.getBoundingClientRect(), r = o.getBoundingClientRect(); \
                   if (r.top < lr.top) l.scrollTop += r.top - lr.top; \
                   else if (r.bottom > lr.bottom) l.scrollTop += r.bottom - lr.bottom; \
                 }",
            )
            .await;
        });
    });
    let have = state.settings.read().devices();
    let query = text().trim().to_string();
    let matches = known_matches(&query, &have);
    let same_as_known = device_profiles::builtin()
        .iter()
        .any(|k| k.name.trim().eq_ignore_ascii_case(&query));
    let show_create = !query.is_empty() && !same_as_known;
    let open = focused() || always_open;
    let known_count = matches.len();
    let rows = known_count + usize::from(show_create);
    let active_now = active().min(rows.saturating_sub(1));

    let mut finish = move |result: Result<String, DeviceError>| match result {
        Ok(id) => {
            state.save_settings();
            text.set(String::new());
            on_added.call(id);
        }
        Err(e) => state.error(e.to_string()),
    };
    let mut add_known_now = move |k: DeviceProfile| {
        let mut settings = state.settings;
        let result = add_known(&mut settings.write(), k);
        finish(result);
    };
    let mut add_custom_now = move |name: String| {
        let mut settings = state.settings;
        let result = add_custom(&mut settings.write(), &name);
        finish(result);
    };
    let chosen = matches.get(active_now).cloned();
    let typed = query.clone();
    let typed_for_row = query.clone();

    rsx! {
        div { class: "adder",
            div { class: if focused() { "adder-box focused" } else { "adder-box" },
                Icon { name: "search", size: 15 }
                input {
                    r#type: "text", value: "{text}", maxlength: "40", spellcheck: "false",
                    placeholder: "Search readers, or type a name",
                    "aria-label": "Add a reader",
                    onmounted: move |e| {
                        if !always_open {
                            spawn(async move {
                                if focus_after_ms > 0 {
                                    tokio::time::sleep(std::time::Duration::from_millis(focus_after_ms)).await;
                                }
                                let _ = e.set_focus(true).await;
                            });
                        }
                    },
                    onfocus: move |_| focused.set(true),
                    onblur: move |_| {
                        focused.set(false);
                        if text.peek().trim().is_empty() {
                            if let Some(close) = on_close { close.call(()); }
                        }
                    },
                    oninput: move |e| { text.set(e.value()); focused.set(true); active.set(0); },
                    onkeydown: move |e| {
                        let key = e.key();
                        if key == Key::Enter {
                            e.prevent_default();
                            if let Some(k) = chosen.clone() {
                                add_known_now(k);
                            } else if !typed.is_empty() {
                                add_custom_now(typed.clone());
                            }
                        } else if rows > 0
                            && (key == Key::ArrowDown || key == Key::ArrowUp || key == Key::Tab)
                        {
                            // Tab and Shift+Tab move through the rows too, and wrap round.
                            e.prevent_default();
                            let back = key == Key::ArrowUp || (key == Key::Tab && e.modifiers().shift());
                            active.set(if back { (active_now + rows - 1) % rows } else { (active_now + 1) % rows });
                        } else if key == Key::Escape {
                            text.set(String::new());
                            focused.set(false);
                            if let Some(close) = on_close { close.call(()); }
                        }
                    },
                }
            }
            if open {
                div { class: "adder-list",
                    for (i, k) in matches.iter().cloned().enumerate() {
                        button {
                            key: "{k.id}", class: if i == active_now { "adder-opt active" } else { "adder-opt" }, r#type: "button",
                            onmouseenter: move |_| active.set(i),
                            // Pressed before the box loses focus, so the list stays until it is used.
                            onmousedown: {
                                let k = k.clone();
                                move |e| { e.prevent_default(); add_known_now(k.clone()); }
                            },
                            Icon { name: "hard-drive", size: 15 }
                            span { class: "grow", "{k.name}" }
                            span { class: "px-size", "{k.inches_label()}" }
                        }
                    }
                    if show_create {
                        button {
                            class: if active_now == known_count { "adder-opt create active" } else { "adder-opt create" }, r#type: "button",
                            onmouseenter: move |_| active.set(known_count),
                            onmousedown: {
                                let name = typed_for_row.clone();
                                move |e| { e.prevent_default(); add_custom_now(name.clone()); }
                            },
                            Icon { name: "plus", size: 15 }
                            span { class: "grow", "Create \u{201c}{typed_for_row}\u{201d} as your own reader" }
                        }
                    }
                    if matches.is_empty() && !show_create {
                        div { class: "adder-empty", "Every known reader is added. Type a name to make your own." }
                    }
                }
            }
        }
    }
}

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
    updated.validate(&others)?;
    if let Some(slot) = settings.custom_devices.iter_mut().find(|d| d.id == base.id) {
        *slot = updated;
    }
    settings.reader_setups.insert(base.id, draft.setup.clone());
    Ok(())
}

/// Write a reader draft over the chosen reader and start the form again from
/// it. False, with the reason shown, if it is not allowed.
fn save_reader(state: AppState, draft: &ReaderDraft, revision: &mut Signal<u32>) -> bool {
    let mut settings = state.settings;
    let result = apply_reader(&mut settings.write(), draft);
    match result {
        Ok(()) => {
            state.save_settings();
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

#[component]
pub fn DevicesEditor() -> Element {
    let state = use_context::<AppState>();
    // Bumped after a save, so the form starts again from what was saved.
    let revision = use_signal(|| 0u32);
    let s = state.settings.read().clone();
    let devices = s.devices();
    let current = s.device_profile();
    let form_key = format!("{}-{}", current.id, revision());
    let mut adding = use_signal(|| false);
    let devices_empty = s.devices().is_empty();
    let chosen = current.id.clone();

    rsx! {
        div { class: "pills", style: "margin-bottom: 18px",
            for d in devices {
                {
                    let id = d.id.clone();
                    let on = d.id == chosen;
                    rsx! {
                        button {
                            key: "{d.id}",
                            class: if on { "pill on" } else { "pill" },
                            onclick: move |_| {
                                if !on {
                                    state.leave(crate::state::Leave::Reader(id.clone()));
                                }
                            },
                            "{d.name}"
                        }
                    }
                }
            }
            if adding() {
                DeviceAdder {
                    on_added: move |_| adding.set(false),
                    on_close: move |_| adding.set(false),
                }
            } else {
                button { class: "pill add", title: "Add a reader",
                    onclick: move |_| adding.set(true),
                    Icon { name: "plus", size: 14 }
                    "Add device"
                }
            }
        }
        if devices_empty {
            p { class: "sx-note", "No reader yet. Add one so pages are made at the right size for it." }
        }
        // Keyed, so choosing another reader (or saving) starts the form afresh.
        if !devices_empty {
            for key in [form_key] {
                ReaderForm { key: "{key}", revision }
            }
        }
    }
}

/// The chosen reader, edited as a draft and written when Save is pressed.
#[component]
fn ReaderForm(revision: Signal<u32>) -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let current = s.device_profile();
    let saved = ReaderDraft {
        name: current.name.clone(),
        width: current.width,
        height: current.height,
        ppi: current.ppi,
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
        ReaderDraft { name: name(), width: draft_w, height: draft_h, ppi: draft_ppi, setup: setup() }
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
                unsaved.set(Some(crate::state::UnsavedReader(std::rc::Rc::new(move || {
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

        div { class: "form-footer",
            button { class: "btn danger",
                title: "Series made for it keep their settings.",
                onclick: {
                    let remove_name = current.name.clone();
                    move |_| state.ask(crate::state::Modal {
                        title: format!("Remove {remove_name}?"),
                        body: "The reader and its settings are taken off your list. Series made for it keep their settings.".into(),
                        confirm: "Remove".into(),
                        cancel: "Keep".into(),
                        danger: true,
                        action: crate::state::ModalAction::RemoveReader { id: remove_id.clone() },
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

/// Open Settings on the page to add or change a reader.
fn open_readers_settings(state: AppState) {
    let mut section = state.settings_section;
    section.set(SettingsSection::Reader);
    state.go(Page::Settings);
}

/// One reader in the list on the Export screen: its name, and its screen size
/// on the same row. Choosing it makes pages for it. Its name and size are
/// changed in Settings.
#[component]
fn ReaderItem(id: String, on: bool) -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let Some(device) = s.devices().into_iter().find(|d| d.id == id) else {
        return rsx! {};
    };
    rsx! {
        div { class: if on { "px-preset on" } else { "px-preset" },
            onclick: move |_| {
                if !on {
                    let mut settings = state.settings;
                    settings.write().device = id.clone();
                    state.save_settings();
                    crate::ui::presets::fit_output_to_reader(state);
                    crate::ui::work::schedule_preview(state, 0);
                }
            },
            div { class: "px-preset-head",
                span { class: "grow px-item-name", "{device.name}" }
                span { class: "px-size", "{device.inches_label()}" }
                if on { Icon { name: "check", size: 14 } }
            }
        }
    }
}

/// The readers as a short scrollable list, and a way to Settings to add one.
#[component]
pub fn ReaderPicker() -> Element {
    let state = use_context::<AppState>();
    let s = state.settings.read().clone();
    let chosen = s.device_profile().id;
    rsx! {
        div { class: "px-list px-scroll",
            for d in s.devices() {
                ReaderItem { key: "{d.id}", id: d.id.clone(), on: d.id == chosen }
            }
        }
        button { class: "px-new", style: "margin-top: 6px; width: 100%", title: "Readers are added in Settings",
            onclick: move |_| open_readers_settings(state),
            Icon { name: "plus", size: 14 } "New device"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str, width: u32, height: u32) -> ReaderDraft {
        ReaderDraft {
            name: name.into(),
            width,
            height,
            ppi: 220,
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
    fn a_reader_starts_out_with_what_is_known_of_it() {
        let x4 = default_setup_for("xteink-x4");
        assert_eq!(x4.formats, ["XTC", "XTCH", "EPUB"]);
        assert_eq!(x4.firmware, Firmware::CrossPoint);
        let kindle = default_setup_for("kindle-paperwhite-12");
        assert_eq!(kindle.formats, ["EPUB"]);
        assert_eq!(kindle.firmware, Firmware::Stock, "CrossPoint does not run on it");
        let mut s = library::Settings::default();
        add_known(&mut s, device_profiles::known("kobo-libra-2").unwrap()).unwrap();
        assert_eq!(s.reader_setup().formats, ["EPUB", "CBZ"], "Kobo opens CBZ itself");
        assert_eq!(s.reader_setup().firmware, Firmware::Stock);
        add_custom(&mut s, "My own").unwrap();
        assert_eq!(s.reader_setup().firmware, Firmware::Stock);
    }

    #[test]
    fn a_known_reader_is_added_once_and_chosen() {
        let mut s = library::Settings::default();
        let id = add_known(&mut s, device_profiles::x4()).unwrap();
        assert_eq!((s.devices().len(), s.device.as_str()), (1, id.as_str()));
        s.device = String::new();
        add_known(&mut s, device_profiles::x4()).unwrap();
        assert_eq!(s.devices().len(), 1, "not added twice");
        assert_eq!(s.device, id);
        assert!(!s.devices()[0].builtin);
    }

    #[test]
    fn typing_lists_the_known_readers_not_yet_added_closest_first() {
        let mut s = library::Settings::default();
        assert_eq!(known_matches("", &s.devices()).len(), device_profiles::builtin().len());
        let names: Vec<_> = known_matches("x4", &s.devices()).into_iter().map(|k| k.name).collect();
        assert_eq!(names, ["Xteink X4 Pro", "Xteink X4"]);
        add_known(&mut s, device_profiles::x4()).unwrap();
        let names: Vec<_> = known_matches("x4", &s.devices()).into_iter().map(|k| k.name).collect();
        assert_eq!(names, ["Xteink X4 Pro"]);
        // The maker is part of the name for searching.
        let kindles = known_matches("kindle", &s.devices());
        assert!(kindles.len() >= 5 && kindles.iter().all(|k| k.maker == "Amazon"));
        assert!(known_matches("amazon", &s.devices()).len() == kindles.len());
        assert!(known_matches("boox palma", &s.devices()).iter().any(|k| k.id == "boox-palma"));
        assert!(known_matches("zzzz", &s.devices()).is_empty());
    }

    #[test]
    fn a_typed_name_makes_a_reader_of_your_own() {
        let mut s = library::Settings::default();
        let id = add_custom(&mut s, "  Kobo Nia ").unwrap();
        assert_eq!(s.device, id);
        let made = s.device_profile();
        assert_eq!((made.name.as_str(), made.width, made.height), ("Kobo Nia", 480, 800));
        assert!(add_custom(&mut s, "kobo nia").is_err(), "names are not shared");
        assert!(add_custom(&mut s, "  ").is_err());
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
