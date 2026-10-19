//! How the app is starting up, the catalog server it runs, and getting a newer version.

use catalog::SuwayomiClient;
use std::path::PathBuf;
use std::sync::Arc;

/// The steps within an open series.
#[derive(Clone, PartialEq, Debug)]
pub enum BootState {
    Starting {
        message: String,
        percent: Option<u8>,
    },
    Ready,
    /// The first run: the catalog engine is not on this computer, and nothing is
    /// downloaded until the user says so.
    NeedsEngine,
    Failed(String),
}

/// The running catalog server.
#[derive(Clone)]
pub struct Services {
    pub client: Arc<SuwayomiClient>,
    pub port: u16,
    pub data_dir: PathBuf,
}

/// Where getting a newer version has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdatePhase {
    /// The offer is showing and nothing has been started.
    Idle,
    /// Downloading and checking the file (0.0 to 1.0).
    Downloading(f32),
    /// The installer is running and this program is about to close.
    Replacing,
    /// The file was opened or shown; what to do next.
    Handed { path: String, command: Option<String> },
    /// It did not work; this says why.
    Failed(String),
}
