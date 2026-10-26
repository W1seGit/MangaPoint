//! System notifications for finished downloads and exports (Windows, macOS
//! and Linux), which can be switched off in Settings.
//!
//! A notification has to say whose it is, or the system picks for it: on
//! Windows an unregistered sender is filed under PowerShell, and on a Mac an
//! unbundled one under the Terminal. [`register`] gives MangaPoint a name and a
//! picture of its own to be shown under.

use crate::state::AppState;
use dioxus::prelude::*;

/// What Windows knows MangaPoint's notifications by.
#[cfg(target_os = "windows")]
const APP_ID: &str = "MangaPoint.App";

/// Tell the system what MangaPoint's notifications are called and look like.
/// Quick, and done from its own thread; safe to call every time the app starts.
pub fn register() {
    std::thread::spawn(register_now);
}

#[cfg(target_os = "windows")]
fn register_now() {
    use std::os::windows::process::CommandExt;
    // The mascot, as a picture file the system can show beside the name.
    let icon = crate::state::paths().root().join("notification-icon.png");
    if !icon.is_file() {
        let (rgba, w, h) = crate::ui::mascot::icon_rgba(4);
        if let Some(image) = image::RgbaImage::from_raw(w, h, rgba) {
            let _ = std::fs::create_dir_all(crate::state::paths().root());
            let _ = image.save(&icon);
        }
    }
    let key = format!(r"HKCU\Software\Classes\AppUserModelId\{APP_ID}");
    for (name, value) in [
        ("DisplayName", "MangaPoint".to_string()),
        ("IconUri", icon.display().to_string()),
        ("IconBackgroundColor", "FF000000".to_string()),
    ] {
        // No console window may flash up for this.
        let result = std::process::Command::new("reg")
            .args(["add", &key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"])
            .creation_flags(0x0800_0000)
            .output();
        if let Err(e) = result {
            tracing::warn!("Could not register the notification name: {e}");
            return;
        }
    }
}

#[cfg(target_os = "macos")]
fn register_now() {
    // Shown as MangaPoint (the bundle the installer makes) instead of the Terminal.
    let _ = notify_rust::set_application("com.mangapoint.app");
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn register_now() {}

/// Tell the user something has finished, unless they turned that off. Shown
/// from its own thread, because some systems make the caller wait.
pub fn finished(state: AppState, title: &str, body: &str) {
    if !state.ui.settings.peek().notifications {
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("MangaPoint").summary(&title).body(&body);
        #[cfg(target_os = "windows")]
        notification.app_id(APP_ID);
        if let Err(e) = notification.show() {
            tracing::warn!("Could not show a notification: {e}");
        }
    });
}
