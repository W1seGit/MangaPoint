//! Which page is showing, and going between pages.

use super::AppState;
use dioxus::prelude::*;

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
}

/// Where the user was going when unsaved reader settings stopped them: asked
/// about, then carried on to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Leave {
    Page(Page),
    Section(SettingsSection),
    /// Another reader, by id.
    Reader(String),
}

/// Writes the reader form's unsaved changes; true if they were saved. Present
/// only while the form has changes that are not saved.
#[derive(Clone)]
pub struct UnsavedReader(pub std::rc::Rc<dyn Fn() -> bool>);

/// The parts of the Settings page, shown in its own sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    #[default]
    Reader,
    Downloads,
    Files,
    Preferences,
    About,
}

impl AppState {
    /// Go to a page. Unsaved reader settings are asked about first.
    pub fn go(&self, page: Page) {
        self.leave(Leave::Page(page));
    }

    /// Go somewhere that would lose the reader form's unsaved changes, asking
    /// what to do with them first; anywhere else, just go.
    pub fn leave(&self, to: Leave) {
        let staying = match &to {
            Leave::Page(page) => *page == Page::Settings,
            Leave::Section(section) => *section == SettingsSection::Reader,
            Leave::Reader(_) => false,
        };
        if !staying && self.unsaved_reader.peek().is_some() {
            let mut prompt = self.leave_prompt;
            prompt.set(Some(to));
            return;
        }
        self.leave_now(to);
    }

    /// Carry on to where the user was going, the changes dealt with.
    pub fn leave_now(&self, to: Leave) {
        match to {
            Leave::Page(page) => self.go_now(page),
            Leave::Section(section) => {
                let mut current = self.settings_section;
                current.set(section);
            }
            Leave::Reader(id) => {
                let mut settings = self.settings;
                settings.write().device = id;
                self.save_settings();
            }
        }
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
