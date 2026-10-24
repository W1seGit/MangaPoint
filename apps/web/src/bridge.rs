//! The calls into the page's JavaScript (`web/js/mp.js`) and the events it
//! sends back. Everything crosses as JSON text.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = mp, js_name = call, catch)]
    async fn raw_call(name: &str, args: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_namespace = mp, js_name = send)]
    fn raw_send(name: &str, args: &str) -> String;
}

fn js_message(e: JsValue) -> String {
    if let Some(s) = e.as_string() {
        return s;
    }
    js_sys::Reflect::get(&e, &JsValue::from_str("message"))
        .ok()
        .and_then(|m| m.as_string())
        .unwrap_or_else(|| "The browser reported an error.".to_string())
}

/// Ask the browser to do something and wait for the answer.
pub async fn call<T: DeserializeOwned>(name: &str, args: Value) -> Result<T, String> {
    let v = raw_call(name, &args.to_string()).await.map_err(js_message)?;
    let text = v.as_string().unwrap_or_else(|| "null".into());
    serde_json::from_str(&text).map_err(|e| format!("The browser's answer to {name} was not understood: {e}"))
}

/// Like [`call`], when only success matters.
pub async fn call_ok(name: &str, args: Value) -> Result<(), String> {
    call::<Value>(name, args).await.map(|_| ())
}

/// Start something that must begin inside the click that asked for it.
pub fn send(name: &str, args: Value) {
    raw_send(name, &args.to_string());
}

/// Keep a record soon (the browser merges quick repeats into one write).
pub fn put_soon<T: Serialize>(store: &str, key: &str, value: &T) {
    send("db_put_soon", json!({ "store": store, "key": key, "value": value }));
}

pub fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

pub async fn sleep(ms: u32) {
    let _ = call_ok("sleep", json!({ "ms": ms })).await;
}

/// What the browser tells the page.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "t")]
pub enum Event {
    #[serde(rename = "analyzing")]
    Analyzing { status: String },
    #[serde(rename = "import")]
    Import {
        drafts: Vec<crate::review::Draft>,
        failures: Vec<crate::review::Failure>,
        /// The book the chapters are for, when they were picked from its overview.
        #[serde(default)]
        into: Option<String>,
    },
    #[serde(rename = "commit_progress")]
    CommitProgress { done: usize, total: usize },
    #[serde(rename = "state_file")]
    StateFile { name: String, text: String },
    #[serde(rename = "key")]
    Key { key: String, shift: bool, ctrl: bool },
    /// A tap or a click on the page (it shows or hides the bars).
    #[serde(rename = "tap")]
    Tap {},
    #[serde(rename = "zoom")]
    Zoom { pct: f64 },
    #[serde(rename = "wheel")]
    Wheel { dy: f64 },
    /// The page was scrolled: up shows the bars, down hides them.
    #[serde(rename = "scroll")]
    Scrolled { up: bool },
    /// A swipe from the left edge: back to the library.
    #[serde(rename = "edge_back")]
    EdgeBack,
    #[serde(rename = "pos")]
    Pos {
        chapter: String,
        page: usize,
        off: f64,
        #[serde(rename = "nearEnd")]
        near_end: bool,
    },
    #[serde(rename = "export_status")]
    ExportStatus { message: String },
    #[serde(rename = "export_progress")]
    ExportProgress {
        fraction: f64,
        label: String,
        stage: String,
        chapter: usize,
        total: usize,
        elapsed: f64,
    },
    #[serde(rename = "export_done")]
    ExportDone { files: Vec<ExportFile>, pages: usize },
    #[serde(rename = "export_error")]
    ExportError { message: String },
    #[serde(rename = "export_cancelled")]
    ExportCancelled,
    #[serde(rename = "sw_update")]
    UpdateReady,
    #[serde(rename = "offline_ready")]
    OfflineReady,
    #[serde(rename = "online")]
    Online { online: bool },
    /// The address bar says (at the start, or after Back) which screen to show.
    #[serde(rename = "route")]
    Route { path: String, hash: String },
    /// The pages in view in the Export screen's list: the first, the last and the one at the top.
    #[serde(rename = "editor")]
    Editor { current: i64 },
    /// The window is as narrow as a phone's (the Export screen changes its layout).
    #[serde(rename = "narrow")]
    Narrow { narrow: bool },
    #[serde(rename = "can_install")]
    CanInstall,
    #[serde(rename = "installed")]
    Installed,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ExportFile {
    pub index: usize,
    pub name: String,
    pub size: u64,
}

/// What the browser found at start.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Boot {
    #[serde(default)]
    pub books: Vec<crate::model::Book>,
    #[serde(default)]
    pub states: Vec<web_engine::state::BookState>,
    #[serde(default)]
    pub meta: Value,
    #[serde(default)]
    pub caps: Caps,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Caps {
    /// The browser can keep big files (OPFS).
    #[serde(default)]
    pub opfs: bool,
    /// The browser can save to a file chosen once (File System Access API).
    #[serde(default)]
    pub fsa: bool,
    #[serde(default)]
    pub persist: bool,
    #[serde(default)]
    pub standalone: bool,
    #[serde(default)]
    pub sw: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StorageInfo {
    pub used: f64,
    pub quota: f64,
    pub persisted: bool,
    #[serde(default)]
    pub opfs: bool,
}

pub async fn next_events() -> Vec<Event> {
    match call::<Vec<Event>>("events", json!({})).await {
        Ok(list) => list,
        Err(_) => {
            sleep(500).await;
            Vec::new()
        }
    }
}
