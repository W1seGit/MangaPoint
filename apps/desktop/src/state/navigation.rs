//! Which page is showing, and going between pages.

use super::AppState;
use dioxus::prelude::*;
use mangapoint_ui::host::Leave;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Setup,
    Search,
    /// A series the user opened: chapters, downloads, preview, export.
    Series,
    Library,
    /// Choosing, editing and exporting series, full window.
    Export,
    /// Downloads and exports, running and finished.
    Tasks,
    /// Choosing what to download from one or more library series.
    Plan,
    Sources,
    Settings,
    /// Reading a downloaded series, full window.
    Reader,
}

/// The parts of the Settings page, shown in its own sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    #[default]
    Readers,
    Downloads,
    Files,
    Preferences,
    About,
}

impl AppState {
    /// Go to a page. Unsaved reader settings are asked about first.
    pub fn go(&self, page: Page) {
        let this = *self;
        self.ui.leave(Leave::Elsewhere(Rc::new(move || this.go_now(page))), page == Page::Settings);
    }

    /// Open a part of Settings. Unsaved reader settings are asked about first.
    pub fn open_section(&self, section: SettingsSection) {
        let this = *self;
        let go = move || {
            let mut current = this.settings_section;
            current.set(section);
        };
        self.ui.leave(Leave::Elsewhere(Rc::new(go)), section == SettingsSection::Readers);
    }

    fn go_now(&self, page: Page) {
        let mut p = self.page;
        // The Export screen borrows the workspace for its own series; the
        // series that was open before it comes back when it is left.
        if *p.peek() == Page::Export && page != Page::Export {
            let mut parked = self.parked_workspace;
            let back = parked.write().take();
            let mut workspace = self.workspace;
            workspace.set(back);
        }
        p.set(page);
    }

    /// Leave the open series for the page it was opened from.
    pub fn close_series(&self, back: Page) {
        let mut workspace = self.workspace;
        workspace.set(None);
        self.go(back);
    }
}
