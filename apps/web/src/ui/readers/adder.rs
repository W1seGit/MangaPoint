//! Adding a reader: the known ones, or one of your own.

use crate::ui::widgets::Icon;
use crate::app::Ctx;
use device_profiles::{DeviceError, DeviceProfile};
use dioxus::prelude::*;
use super::default_setup_for;

/// Add a known reader to the user's own and choose it. One already added is
/// just chosen.
pub(super) fn add_known(settings: &mut library::Settings, known: DeviceProfile) -> Result<String, DeviceError> {
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
    let state = use_context::<Ctx>();
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
        crate::bridge::send("adder_scroll", serde_json::json!({}));
    });
    let have = state.readers.read().devices();
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
            state.save_readers();
            text.set(String::new());
            on_added.call(id);
        }
        Err(e) => state.error(e.to_string()),
    };
    let mut add_known_now = move |k: DeviceProfile| {
        let mut settings = state.readers;
        let result = add_known(&mut settings.write(), k);
        finish(result);
    };
    let mut add_custom_now = move |name: String| {
        let mut settings = state.readers;
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
                                    crate::bridge::sleep(focus_after_ms as _).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use device_profiles::Firmware;

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
}
