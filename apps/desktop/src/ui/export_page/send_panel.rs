//! The Send to panel: the reader or folder the files go to, their format and how they are split.

use super::{active_series, Panel};
use crate::logic::{self, TargetChoice};
use crate::state::{AppState, GroupingChoice};
use crate::ui::devices::ReaderPicker;
use crate::ui::presets;
use crate::ui::widgets::{pick_folder, Help, Icon};
use dioxus::prelude::*;
use library::OutputFormat;
use pipeline::export::removable_drives;
use std::time::Duration;

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
pub(super) fn SendPanel() -> Element {
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
