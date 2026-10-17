//! Editing the details of an imported series by hand: its name, description,
//! author, artist, status, genres and cover. (A series that came from a source
//! gets these from the source.)

use crate::state::{AppState, InfoDraft};
use crate::ui::import::InfoFields;
use crate::ui::widgets::Icon;
use crate::ui::work::{load_saved_cover, save_series_details};
use dioxus::prelude::*;

/// The draft a series' current details make.
fn draft_of(state: AppState, series_id: &str) -> InfoDraft {
    let library = state.library.peek();
    let Some(series) = library.find(series_id) else {
        return InfoDraft::default();
    };
    let info = series.info.clone().unwrap_or_default();
    InfoDraft {
        title: series.title.clone(),
        description: info.description,
        author: info.author,
        artist: info.artist,
        status: info.status,
        genres: info.genres,
        cover_url: info
            .has_cover
            .then(|| crate::covers::saved_url(series_id, crate::state::now_secs())),
        cover: None,
    }
}

#[component]
pub fn EditInfo(series_id: String, on_done: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let id = series_id.clone();
    let mut draft = use_signal(move || draft_of(state, &id));
    use_effect({
        let id = series_id.clone();
        move || load_saved_cover(state, &id)
    });
    let save_id = series_id.clone();
    let can_save = !draft.read().title.trim().is_empty();
    rsx! {
        div { class: "edit-info",
            InfoFields {
                info: draft.read().clone(),
                show_title: true,
            show_cover: true,
                first_page: Some(series_id.clone()),
                onchange: move |next: InfoDraft| draft.set(next),
            }
            div { class: "edit-info-actions",
                button { class: "btn primary", disabled: !can_save,
                    onclick: move |_| {
                        save_series_details(state, &save_id, &draft.read());
                        state.notify("Details saved.");
                        on_done.call(());
                    },
                    Icon { name: "save", size: 15 } "Save"
                }
                button { class: "btn ghost", onclick: move |_| on_done.call(()), "Cancel" }
            }
        }
    }
}
