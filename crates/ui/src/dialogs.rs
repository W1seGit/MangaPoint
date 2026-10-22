//! What sits over every screen of both apps: the question about unsaved reader settings, and the
//! toasts.

use crate::art;
use crate::host::{use_ui, ToastKind};
use crate::widgets::Icon;
use dioxus::prelude::*;

/// Asked when the user leaves the reader form with changes that are not saved.
#[component]
pub fn UnsavedHost() -> Element {
    let ui = use_ui();
    let Some(to) = ui.leave_prompt.read().clone() else { return rsx! {} };
    let (save_to, discard_to) = (to.clone(), to);
    rsx! {
        div { class: "overlay", id: "unsaved-prompt",
            div { class: "modal",
                h2 { "You have unsaved settings" }
                p { "Would you like to save or discard them?" }
                div { class: "actions",
                    button { class: "btn", id: "unsaved-keep",
                        onclick: move |_| { let mut p = ui.leave_prompt; p.set(None); },
                        "Keep editing"
                    }
                    button { class: "btn danger", id: "unsaved-discard",
                        onclick: move |_| {
                            let mut p = ui.leave_prompt;
                            p.set(None);
                            let mut unsaved = ui.unsaved_reader;
                            unsaved.set(None);
                            ui.leave_now(discard_to.clone());
                        },
                        "Discard"
                    }
                    button { class: "btn primary", id: "unsaved-save",
                        onclick: move |_| {
                            let saved = ui.unsaved_reader.peek().clone().is_none_or(|u| (u.0)());
                            if saved {
                                let mut p = ui.leave_prompt;
                                p.set(None);
                                let mut unsaved = ui.unsaved_reader;
                                unsaved.set(None);
                                ui.leave_now(save_to.clone());
                            }
                        },
                        "Save"
                    }
                }
            }
        }
    }
}

/// The toasts: the newest sits at the bottom with the older ones stacked behind it; pointing at
/// the stack spreads them out to read, and keeps them there.
#[component]
pub fn ToastHost() -> Element {
    let ui = use_ui();
    let toasts = ui.toasts.read().clone();
    if toasts.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {
            class: "toasts",
            onmouseenter: move |_| { let mut h = ui.toasts_hovered; h.set(true); },
            onmouseleave: move |_| { let mut h = ui.toasts_hovered; h.set(false); },
            for (i, toast) in toasts.iter().rev().enumerate() {
                {
                    let id = toast.id;
                    let look = match toast.kind {
                        ToastKind::Success => "success",
                        ToastKind::Warning => "warning",
                        ToastKind::Error => "error",
                    };
                    rsx! {
                        div {
                            key: "{toast.id}",
                            class: if toast.leaving { "toast {look} leaving" } else { "toast {look}" },
                            style: "--i: {i}",
                            role: "status",
                            art::cat::ToastCat { look: look.to_string() }
                            div { class: "toast-text", "{toast.text}" }
                            button { class: "toast-x", title: "Dismiss", "aria-label": "Dismiss", onclick: move |_| ui.dismiss_toast(id),
                                Icon { name: "x", size: 14 }
                            }
                        }
                    }
                }
            }
        }
    }
}
