//! Small controls for the reader form: the file types as pills.

use crate::ui::widgets::Icon;
use dioxus::prelude::*;

/// The file types a reader can be said to open.
const FORMAT_OPTIONS: [&str; 3] = ["XTC", "XTCH", "EPUB"];

/// The formats as a row of pills inside a box: type one and press Enter, or
/// pick from the list that opens under the box.
#[component]
pub(super) fn FormatTags(value: Vec<String>, on_change: EventHandler<Vec<String>>) -> Element {
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
