//! The desktop app's side of the shared screens (`mangapoint_ui::host::Host`).

use crate::state::{AppState, Modal, ModalAction, Page, SettingsSection};
use dioxus::prelude::*;
use mangapoint_ui::host::{Ask, Caps, Host, LocalFuture, SharedAction, ToastKind};

pub struct DesktopHost(pub AppState);

impl Host for DesktopHost {
    fn sleep(&self, ms: u64) -> LocalFuture<()> {
        Box::pin(tokio::time::sleep(std::time::Duration::from_millis(ms)))
    }

    fn log(&self, kind: ToastKind, text: &str) {
        match kind {
            ToastKind::Success => tracing::info!("{text}"),
            ToastKind::Warning | ToastKind::Error => tracing::warn!("{text}"),
        }
    }

    fn save_settings(&self) {
        self.0.save_settings();
    }

    /// The reader in use is kept in the settings; series made for it switch to a file type it
    /// opens, and the preview is drawn again for its screen.
    fn use_reader(&self, id: &str) {
        let state = self.0;
        let mut settings = state.ui.settings;
        settings.write().device = id.to_string();
        state.save_settings();
        super::presets::fit_output_to_reader(state);
        super::work::schedule_preview(state, 0);
    }

    fn open_readers(&self) {
        let state = self.0;
        let mut section = state.settings_section;
        section.set(SettingsSection::Readers);
        state.go(Page::Settings);
    }

    fn ask(&self, ask: Ask) {
        let action = match ask.action {
            SharedAction::RemoveReader { id } => ModalAction::RemoveReader { id },
        };
        self.0.ask(Modal {
            title: ask.title,
            body: ask.body,
            confirm: ask.confirm,
            cancel: ask.cancel,
            danger: ask.danger,
            action,
        });
    }

    fn run_script(&self, name: &str, args: String) {
        if let Some(code) = mangapoint_ui::scripts::call(name, &args) {
            spawn(async move {
                let _ = document::eval(&code).await;
            });
        }
    }

    fn check_wifi(&self, url: String) -> LocalFuture<Result<(), String>> {
        Box::pin(async move { pipeline::export::check_webdav(&url).await })
    }

    fn caps(&self) -> Caps {
        Caps { send_to_reader: true }
    }
}
