//! Choosing the reader pages are made for, and adding your own.

mod adder;
mod form_widgets;
mod reader_form;

pub use adder::DeviceAdder;

use reader_form::ReaderForm;

use crate::ui::widgets::Icon;
use crate::app::Ctx;
use device_profiles::Firmware;
use dioxus::prelude::*;
use library::ReaderSetup;

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

#[component]
pub fn DevicesEditor() -> Element {
    let state = use_context::<Ctx>();
    // Bumped after a save, so the form starts again from what was saved.
    let revision = use_signal(|| 0u32);
    let s = state.readers.read().clone();
    let devices = s.devices();
    let current = s.device_profile();
    let phone_chosen = device_profiles::is_original(&current.id);
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
                                    state.leave(crate::app::Leave::Reader(id.clone()));
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
        if phone_chosen {
            p { class: "sx-note", "\"Phone or tablet · original quality\" is chosen on the Export screen. Pick one of your readers above to change its settings." }
        }
        if devices_empty && !phone_chosen {
            p { class: "sx-note", "No reader yet. Add one so pages are made at the right size for it." }
        }
        // Keyed, so choosing another reader (or saving) starts the form afresh.
        if !devices_empty && !phone_chosen {
            for key in [form_key] {
                ReaderForm { key: "{key}", revision }
            }
        }
    }
}

/// Open Settings on the page to add or change a reader.
fn open_readers_settings(state: Ctx) {
    let mut section = state.settings_section;
    section.set(crate::ui::settings::Section::Readers);
    state.go(crate::app::Page::Settings);
}

/// One reader in the list on the Export screen: its name, and its screen size
/// on the same row. Choosing it makes pages for it. Its name and size are
/// changed in Settings.
#[component]
fn ReaderItem(id: String, on: bool) -> Element {
    let state = use_context::<Ctx>();
    let s = state.readers.read().clone();
    let Some(device) = s.devices().into_iter().find(|d| d.id == id) else {
        return rsx! {};
    };
    rsx! {
        div { class: if on { "px-preset on" } else { "px-preset" },
            onclick: move |_| {
                if !on {
                    crate::ui::export::use_reader(state, &id);
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

/// The built-in entry for a phone or tablet: no reader to make pages for, the
/// pages go out as they were saved, in color.
#[component]
fn PhoneItem(on: bool) -> Element {
    let state = use_context::<Ctx>();
    rsx! {
        div {
            id: "reader-phone-original", class: if on { "px-preset on" } else { "px-preset" },
            role: "button", "aria-pressed": "{on}",
            onclick: move |_| {
                if !on {
                    crate::ui::export::use_reader(state, device_profiles::PHONE_ORIGINAL_ID);
                }
            },
            div { class: "px-preset-head",
                span { class: "grow px-item-name", "Phone or tablet · original quality" }
                if on { Icon { name: "check", size: 14 } }
            }
            div { class: "muted small", "Color pages exactly as downloaded, as a CBZ or a PDF." }
        }
    }
}

/// The readers as a short scrollable list, and a way to Settings to add one.
#[component]
pub fn ReaderPicker() -> Element {
    let state = use_context::<Ctx>();
    let s = state.readers.read().clone();
    let chosen = s.device_profile().id;
    rsx! {
        div { class: "px-list px-scroll",
            PhoneItem { on: device_profiles::is_original(&chosen) }
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
