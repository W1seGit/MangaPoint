//! The AI panel finder: downloading its model once, and loading it.
//!
//! The model is about 40 MB, so it is fetched the first time someone chooses
//! it rather than shipped with the app. Once loaded it is handed to the
//! converter, and the preview is drawn again with it.

use crate::state::{paths, spawn_forever, AppState};
use dioxus::prelude::*;
use panel_model::YoloPanels;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ModelStatus {
    /// Not downloaded yet.
    #[default]
    Missing,
    /// Downloading: the share done, 0..=1.
    Downloading(f32),
    Loading,
    Ready,
    Failed(String),
}

/// At start: load the model if it was downloaded before.
pub fn load_if_downloaded(state: AppState) {
    if panel_model::is_downloaded(paths().root()) {
        load(state);
    }
}

/// Download the model, then load it.
pub fn download(state: AppState) {
    let mut status = state.panel_model;
    if matches!(
        *status.peek(),
        ModelStatus::Downloading(_) | ModelStatus::Loading | ModelStatus::Ready
    ) {
        return;
    }
    status.set(ModelStatus::Downloading(0.0));
    spawn_forever(async move {
        let mut shown = 0u32;
        let result = panel_model::download(paths().root(), |done, total| {
            // A whole percent at a time, so the screen is not redrawn for
            // every piece of the download.
            let percent = (done * 100 / total.max(1)) as u32;
            if percent != shown {
                shown = percent;
                status.set(ModelStatus::Downloading(percent as f32 / 100.0));
            }
        })
        .await;
        match result {
            Ok(_) => {
                tracing::info!("AI panel model downloaded");
                load(state);
            }
            Err(e) => {
                tracing::warn!("AI panel model: {e}");
                status.set(ModelStatus::Failed(e.to_string()));
            }
        }
    });
}

fn load(state: AppState) {
    let mut status = state.panel_model;
    status.set(ModelStatus::Loading);
    spawn_forever(async move {
        let root = paths().root().to_path_buf();
        let loaded = tokio::task::spawn_blocking(move || {
            YoloPanels::load(&panel_model::model_path(&root))
        })
        .await;
        match loaded {
            Ok(Ok(model)) => {
                converter::set_panel_model(Some(Arc::new(model)));
                tracing::info!("AI panel model loaded");
                status.set(ModelStatus::Ready);
                // The pages on screen were planned without it.
                if state.workspace.peek().is_some() {
                    crate::ui::work::schedule_preview(state, 0);
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("AI panel model: {e}");
                status.set(ModelStatus::Failed(e.to_string()));
            }
            Err(e) => status.set(ModelStatus::Failed(e.to_string())),
        }
    });
}
