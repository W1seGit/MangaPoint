//! Starting, stopping and restarting the local catalog server.

use crate::state::{paths, AppState, BootState, Services};
use catalog::provision::Progress;
use catalog::release::{current_asset, VERSION_TAG};
use catalog::server::read_tail;
use catalog::{browser, provision, RunningServer, ServerConfig};
use dioxus::prelude::*;
use library::Choice;
use sources::SourceManager;
use std::cell::Cell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tracing::{error, info, warn};

static SERVER: Mutex<Option<RunningServer>> = Mutex::new(None);

/// Bumped at every boot, so a watchdog from an earlier boot knows to stop.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// One start of the engine at a time: the watchdog's restart, "Try again" and
/// the browser switches must not each start one.
static BOOTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Set once the app is closing, so nothing starts an engine after that.
static CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RESTARTS: Mutex<RestartBudget> = Mutex::new(RestartBudget::new());

/// How often the running engine is checked.
const WATCH_INTERVAL: Duration = Duration::from_secs(5);
/// Automatic restarts allowed within [`RESTART_WINDOW`] before giving up.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(600);

/// How many times the engine may be restarted automatically in a time window.
struct RestartBudget {
    times: Vec<Instant>,
}

impl RestartBudget {
    const fn new() -> Self {
        Self { times: Vec::new() }
    }

    /// Record a restart at `now` if the budget allows one; false if the
    /// engine has already been restarted `max` times within `window`.
    fn allow(&mut self, now: Instant, max: usize, window: Duration) -> bool {
        self.times
            .retain(|t| now.saturating_duration_since(*t) < window);
        if self.times.len() >= max {
            return false;
        }
        self.times.push(now);
        true
    }

    fn clear(&mut self) {
        self.times.clear();
    }
}

/// Bring the server up, showing progress, and publish it to the app.
/// Safe to call again: any server already running is stopped first.
///
/// The first run does not download the engine by itself: with it missing, this
/// stops at [`BootState::NeedsEngine`] until called again with `install` set.
pub async fn boot(state: AppState, install: bool) {
    boot_with(state, install, "Preparing…").await;
}

async fn boot_with(state: AppState, install: bool, first_message: &str) {
    let _one_at_a_time = BOOTING.lock().await;
    if CLOSING.load(Ordering::SeqCst) {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let mut boot = state.boot;
    let mut services = state.services;
    services.set(None);
    crate::covers::set_client(None);
    let installed = provision::locate_install(
        &paths().suwayomi_dir().join(format!("suwayomi-{VERSION_TAG}")),
    )
    .is_ok();
    if !installed && !install && state.settings.peek().setup_step != library::SetupStep::Done {
        boot.set(BootState::NeedsEngine);
        return;
    }
    boot.set(BootState::Starting {
        message: first_message.into(),
        percent: None,
    });
    stop_running().await;
    stop_leftover_servers().await;

    match start(state).await {
        Ok(_) if CLOSING.load(Ordering::SeqCst) => {
            // The app closed while the engine was starting: do not leave it behind.
            if let Some(mut server) = SERVER.lock().unwrap().take() {
                server.kill_now();
            }
        }
        Ok(svc) => {
            crate::covers::set_client(Some(svc.client.clone()));
            services.set(Some(svc));
            // Make sure the default extension store is known.
            if let Some(client) = state.client() {
                let mut sources = state.sources.read().clone();
                let was_added = sources.default_store_added;
                let result = SourceManager::new(&*client)
                    .ensure_default_store(&mut sources)
                    .await;
                let mut s = state.sources;
                s.set(sources);
                state.save_sources();
                match result {
                    Ok(()) if !was_added => info!("Added the default source catalog (Keiyoushi)"),
                    Ok(()) => {}
                    Err(e) => state.error(format!("Could not add the source catalog: {e}")),
                }
            }
            info!("Ready");
            boot.set(BootState::Ready);
            // If the engine dies later (out of memory, a crash), notice and restart it.
            // Boxed because the watchdog can itself call `boot_with`.
            let watcher: Pin<Box<dyn Future<Output = ()>>> =
                Box::pin(watch_engine(state, generation));
            dioxus::core::spawn_forever(watcher);
            // Downloads that were waiting when the app closed carry on.
            crate::ui::work::ensure_queue_running(state);
            crate::ui::work::restore_export(state);
        }
        Err(message) => {
            error!("{message}");
            boot.set(BootState::Failed(message));
        }
    }
}

/// While the engine is up, check every few seconds that its process is still
/// running; if it died, start it again, within the restart budget.
async fn watch_engine(state: AppState, generation: u64) {
    let mut boot = state.boot;
    let mut services = state.services;
    loop {
        tokio::time::sleep(WATCH_INTERVAL).await;
        if GENERATION.load(Ordering::SeqCst) != generation {
            return; // A newer boot (or a deliberate restart) took over.
        }
        let died = {
            let mut guard = SERVER.lock().unwrap();
            let gone = guard.as_mut().is_some_and(|s| s.has_exited());
            if gone {
                guard.take()
            } else {
                None
            }
        };
        let Some(dead) = died else { continue };
        warn!(
            "The catalog engine stopped by itself. End of its log:\n{}",
            dead.log_tail(1500)
        );
        drop(dead);
        let allowed = RESTARTS
            .lock()
            .unwrap()
            .allow(Instant::now(), MAX_RESTARTS, RESTART_WINDOW);
        if !allowed {
            RESTARTS.lock().unwrap().clear();
            services.set(None);
            crate::covers::set_client(None);
            let message = "The catalog engine keeps stopping, so it was not restarted again. \
                           Its log is in the data folder under server-data/logs. Try again, \
                           and if it keeps happening restart the app or your computer."
                .to_string();
            error!("{message}");
            boot.set(BootState::Failed(message));
            return;
        }
        info!("Restarting the catalog engine");
        boot_with(state, true, "The catalog engine stopped; restarting…").await;
        return; // The new boot starts its own watchdog.
    }
}

async fn start(state: AppState) -> Result<Services, String> {
    let mut boot = state.boot;
    let asset = current_asset().map_err(|e| e.to_string())?;

    info!("Checking for the catalog engine (Suwayomi {VERSION_TAG})");
    let mut last_percent = 255u8;
    let mut logged_quarter = 255u64;
    let mut downloaded = false;
    let install = provision(&paths().suwayomi_dir(), &asset, |p| {
        let (message, percent) = match p {
            Progress::Downloading {
                done,
                total: Some(t),
            } if t > 0 => {
                downloaded = true;
                let quarter = done * 4 / t;
                if quarter != logged_quarter {
                    logged_quarter = quarter;
                    info!(
                        "Downloading the catalog engine ({} MB)… {}%",
                        t / 1_000_000,
                        quarter * 25
                    );
                }
                let pct = (done * 100 / t) as u8;
                (format!("Downloading the catalog engine… {pct}%"), Some(pct))
            }
            Progress::Downloading { .. } => ("Downloading the catalog engine…".to_string(), None),
            Progress::Verifying => {
                info!("Download finished; checking its checksum");
                ("Checking the download…".to_string(), None)
            }
            Progress::Extracting => {
                info!("Checksum matches; unpacking (this takes a minute)");
                ("Unpacking…".to_string(), None)
            }
            Progress::Done => return,
        };
        // Redraw only when the percentage actually moves.
        if percent != Some(last_percent) {
            last_percent = percent.unwrap_or(255);
            boot.set(BootState::Starting { message, percent });
        }
    })
    .await
    .map_err(|e| format!("Could not install the catalog engine: {e}"))?;
    if downloaded {
        info!("Catalog engine installed in {}", install.dir.display());
    } else {
        info!("Catalog engine already installed");
    }

    let browser_on = state.settings.read().browser_support && browser::supported();
    let config = ServerConfig {
        install,
        data_dir: paths().server_data_dir(),
        browser_support: browser_on,
    };
    let needs_browser_download = browser_on && !browser::is_installed(&config.data_dir);
    if needs_browser_download {
        info!("Starting the catalog server; browser support is on, so it will download about 270 MB first");
    } else {
        info!(
            "Starting the catalog server (browser support {})",
            if browser_on { "on" } else { "off" }
        );
    }
    boot.set(BootState::Starting {
        message: if needs_browser_download {
            "Starting, and downloading browser support (about 270 MB)…".into()
        } else {
            "Starting the catalog engine…".into()
        },
        percent: None,
    });

    // While the server starts it may download browser support; show how far along it is.
    let log = config.data_dir.join("logs").join("mangapoint-server.log");
    let finished = Rc::new(Cell::new(false));
    {
        let finished = finished.clone();
        spawn(async move {
            let mut logged_quarter = 255u8;
            while !finished.get() {
                tokio::time::sleep(Duration::from_millis(800)).await;
                if let Some(p) = browser::download_progress(&read_tail(&log, 4096)) {
                    if p.percent / 25 != logged_quarter {
                        logged_quarter = p.percent / 25;
                        info!("Downloading browser support… {}%", logged_quarter * 25);
                    }
                    let mut b = boot;
                    b.set(BootState::Starting {
                        message: format!("Downloading browser support… {}%", p.percent),
                        percent: Some(p.percent),
                    });
                }
            }
        });
    }
    let started_at = Instant::now();
    let started = RunningServer::start(&config).await;
    finished.set(true);
    let server = started.map_err(|e| format!("The catalog engine did not start: {e}"))?;
    info!(
        "Catalog server ready on 127.0.0.1:{} after {:.1}s (local only, password protected)",
        server.port,
        started_at.elapsed().as_secs_f32()
    );

    let services = Services {
        client: Arc::new(server.client.clone()),
        port: server.port,
        data_dir: config.data_dir.clone(),
    };
    *SERVER.lock().unwrap() = Some(server);
    Ok(services)
}

async fn stop_running() {
    let server = SERVER.lock().unwrap().take();
    if let Some(server) = server {
        info!("Stopping the catalog server");
        let _ = server.stop().await;
    }
}

/// End the server right now. Used when the window closes or on Ctrl+C.
pub fn shutdown() {
    // Nothing should restart it from here on.
    CLOSING.store(true, Ordering::SeqCst);
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(mut server) = SERVER.lock().unwrap().take() {
        info!("Closing: stopping the catalog server");
        server.kill_now();
    }
    // Only now may another copy of the app start: its engine can use the database.
    release_instance_lock();
}

const ROOT_DIR_ARG: &str = "-Dsuwayomi.tachidesk.config.server.rootDir=";

/// Is this command line a Suwayomi server using `data_dir`?
///
/// Matching on the command line (rather than a remembered process id) finds
/// a server left behind however the last run ended, and cannot be fooled by
/// a recycled process id.
fn is_our_server(cmd: &[String], data_dir: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    cmd.iter().any(|a| a.contains("Suwayomi-Server.jar"))
        && cmd.iter().any(|a| {
            a.strip_prefix(ROOT_DIR_ARG)
                .is_some_and(|dir| norm(dir) == norm(data_dir))
        })
}

/// A catalog server left over from an earlier run (a crash, or Ctrl+C in the
/// terminal) keeps the database locked, so a new one cannot start. Stop any
/// such server and wait until it has really exited.
async fn stop_leftover_servers() {
    let data_dir = paths().server_data_dir().display().to_string();
    let find = || {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let leftovers: Vec<Pid> = sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                let cmd: Vec<String> = p
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                if !is_our_server(&cmd, &data_dir) {
                    return false;
                }
                let parent = p
                    .parent()
                    .and_then(|pp| sys.process(pp).map(|proc| (pp.as_u32(), proc_stem(proc))));
                if belongs_to_other_live_app(parent.as_ref().map(|(i, n)| (*i, n.as_str()))) {
                    warn!("A catalog server belongs to another MangaPoint that is still open; leaving it alone");
                    return false;
                }
                true
            })
            .map(|(pid, p)| {
                p.kill();
                *pid
            })
            .collect();
        leftovers
    };

    let killed = find();
    if killed.is_empty() {
        return;
    }
    warn!(
        "A catalog server from an earlier run was still running (process {}); stopping it",
        killed
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    // Wait for it to let go of the database before starting a new one.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&killed),
            true,
            ProcessRefreshKind::nothing(),
        );
        if killed.iter().all(|pid| sys.process(*pid).is_none()) {
            info!("The earlier server has stopped");
            return;
        }
        if Instant::now() > deadline {
            warn!("The earlier server is slow to stop; continuing anyway");
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn proc_stem(p: &sysinfo::Process) -> String {
    let name = p.name().to_string_lossy().to_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

fn our_exe_stem() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.file_stem()?.to_string_lossy().to_lowercase())
}

/// Is the server's parent another running copy of this app (not this one)?
/// Such a server is in use and must not be stopped.
fn belongs_to_other_live_app(parent: Option<(u32, &str)>) -> bool {
    let (Some((pid, name)), Some(ours)) = (parent, our_exe_stem()) else {
        return false;
    };
    pid != std::process::id() && name == ours
}

// ---- One copy of the app per data folder -------------------------------------

/// Held while this copy of the app runs; releasing it lets another start.
pub struct InstanceLock {
    path: PathBuf,
    pid: u32,
}

#[derive(Debug)]
pub enum LockError {
    /// Another copy of the app, with this process id, is using the data folder.
    Held(u32),
    Io(std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held(pid) => write!(
                f,
                "MangaPoint is already open (process {pid}) and using this data folder"
            ),
            LockError::Io(e) => write!(f, "could not create the lock file: {e}"),
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // Only remove a file that is still ours.
        let mine = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(self.pid);
        if mine {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

static INSTANCE_LOCK: Mutex<Option<InstanceLock>> = Mutex::new(None);

/// Take the lock for the data folder `root`, or say who has it.
pub fn acquire_instance_lock(root: &Path) -> Result<InstanceLock, LockError> {
    acquire_lock_with(root, std::process::id(), &|pid| {
        pid != std::process::id() && is_app_process(pid)
    })
}

/// Keep `lock` until [`shutdown`] (or the process ends).
pub fn keep_instance_lock(lock: InstanceLock) {
    *INSTANCE_LOCK.lock().unwrap() = Some(lock);
}

fn release_instance_lock() {
    INSTANCE_LOCK.lock().unwrap().take();
}

/// Is `pid` a running copy of this app? (A recycled process id belonging to
/// some other program does not count.)
fn is_app_process(pid: u32) -> bool {
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    match (sys.process(pid), our_exe_stem()) {
        (Some(p), Some(ours)) => proc_stem(p) == ours,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// The lock is a file `mangapoint.lock` holding the owner's process id. A
/// file whose owner is not running (a crash, a power cut) is stale and is
/// taken over.
fn acquire_lock_with(
    root: &Path,
    my_pid: u32,
    owner_running: &dyn Fn(u32) -> bool,
) -> Result<InstanceLock, LockError> {
    use std::io::Write;
    std::fs::create_dir_all(root).map_err(LockError::Io)?;
    let path = root.join("mangapoint.lock");
    let mut empty_reads = 0;
    for _ in 0..6 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(my_pid.to_string().as_bytes())
                    .map_err(LockError::Io)?;
                return Ok(InstanceLock { path, pid: my_pid });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let owner = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok());
                match owner {
                    Some(pid) if owner_running(pid) => return Err(LockError::Held(pid)),
                    None if empty_reads == 0 => {
                        // The other copy may be between creating and filling the file.
                        empty_reads += 1;
                        std::thread::sleep(Duration::from_millis(150));
                        continue;
                    }
                    _ => {}
                }
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => return Err(LockError::Io(e)),
        }
    }
    Err(LockError::Io(std::io::Error::other(
        "the lock file keeps changing",
    )))
}

/// Turn browser support on and restart the server so it downloads and uses it.
pub async fn enable_browser(state: AppState) {
    info!("Turning on browser support; restarting the catalog server");
    let mut settings = state.settings;
    settings.write().browser_support = true;
    state.save_settings();
    boot(state, true).await;
}

/// Turn browser support off and delete the downloaded runtime.
pub async fn disable_browser(state: AppState) {
    info!("Turning off browser support and deleting its files");
    let mut settings = state.settings;
    settings.write().browser_support = false;
    state.save_settings();
    stop_running().await;
    let dir = paths().server_data_dir();
    let freed = browser::size_on_disk(&dir);
    match browser::remove(&dir) {
        Ok(()) => info!("Removed browser support ({} MB freed)", freed / 1_000_000),
        Err(e) => state.error(format!("Could not remove browser support: {e}")),
    }
    boot(state, true).await;
}

/// Install a built-in source. Returns whether it was installed.
pub async fn install_default(state: AppState, id: &str) -> bool {
    let Some(client) = state.client() else {
        return false;
    };
    let Some(default) = sources::defaults::find(id) else {
        return false;
    };
    info!("Installing source {}…", default.name);
    let started = Instant::now();
    let mut sources_state = state.sources.read().clone();
    let result = SourceManager::new(&*client)
        .answer(&mut sources_state, id, Choice::Install)
        .await;
    let mut s = state.sources;
    s.set(sources_state);
    state.save_sources();
    match result {
        Ok(()) => {
            info!(
                "Installed source {} in {:.1}s",
                default.name,
                started.elapsed().as_secs_f32()
            );
            true
        }
        Err(e) => {
            state.error(format!("Could not install {}: {e}", default.name));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    const DATA: &str = r"C:\Users\me\AppData\Roaming\MangaPoint\server-data";

    #[test]
    fn recognises_our_server_by_jar_and_data_folder() {
        let ours = cmd(&[
            r"C:\x\jre\bin\java.exe",
            "--enable-native-access=ALL-UNNAMED",
            &format!("{ROOT_DIR_ARG}{DATA}"),
            "-Dsuwayomi.tachidesk.config.server.port=4567",
            "-jar",
            r"C:\x\bin\Suwayomi-Server.jar",
        ]);
        assert!(is_our_server(&ours, DATA));
    }

    #[test]
    fn ignores_servers_for_other_folders_and_unrelated_programs() {
        let other_folder = cmd(&[
            "java.exe",
            &format!(r"{ROOT_DIR_ARG}C:\other\data"),
            "-jar",
            "Suwayomi-Server.jar",
        ]);
        assert!(
            !is_our_server(&other_folder, DATA),
            "another data folder is not ours"
        );

        let not_suwayomi = cmd(&[
            "java.exe",
            &format!("{ROOT_DIR_ARG}{DATA}"),
            "-jar",
            "other.jar",
        ]);
        assert!(!is_our_server(&not_suwayomi, DATA));

        let no_root = cmd(&["java.exe", "-jar", "Suwayomi-Server.jar"]);
        assert!(!is_our_server(&no_root, DATA));
        assert!(!is_our_server(&[], DATA));
    }

    #[test]
    fn path_spelling_differences_still_match() {
        let c = cmd(&[
            "java.exe",
            &format!("{ROOT_DIR_ARG}c:/users/me/appdata/roaming/mangapoint/server-data/"),
            "-jar",
            "Suwayomi-Server.jar",
        ]);
        assert!(
            is_our_server(&c, DATA),
            "case, slashes and a trailing slash do not matter"
        );
    }

    fn lock_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mangapoint-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn restarts_are_limited_to_three_in_ten_minutes() {
        let mut b = RestartBudget::new();
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        assert!(b.allow(s(0), 3, RESTART_WINDOW));
        assert!(b.allow(s(60), 3, RESTART_WINDOW));
        assert!(b.allow(s(120), 3, RESTART_WINDOW));
        assert!(!b.allow(s(180), 3, RESTART_WINDOW), "a fourth within the window is refused");
        // The first one has aged out of the window.
        assert!(b.allow(s(601), 3, RESTART_WINDOW));
        assert!(!b.allow(s(602), 3, RESTART_WINDOW));
        b.clear();
        assert!(b.allow(s(603), 3, RESTART_WINDOW));
    }

    #[test]
    fn a_second_copy_cannot_take_the_data_folder_until_the_first_lets_go() {
        let dir = lock_dir("second");
        let alive = |_: u32| true;
        let first = acquire_lock_with(&dir, 100, &alive).unwrap();
        match acquire_lock_with(&dir, 200, &alive) {
            Err(LockError::Held(100)) => {}
            other => panic!("expected Held(100), got {:?}", other.map(|_| ())),
        }
        drop(first);
        let second = acquire_lock_with(&dir, 200, &alive).unwrap();
        drop(second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_left_by_a_dead_copy_is_taken_over() {
        let dir = lock_dir("stale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mangapoint.lock"), "4242").unwrap();
        let lock = acquire_lock_with(&dir, 100, &|_| false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("mangapoint.lock")).unwrap(),
            "100"
        );
        drop(lock);
        assert!(!dir.join("mangapoint.lock").exists());
        // A garbled file counts as stale too.
        std::fs::write(dir.join("mangapoint.lock"), "").unwrap();
        assert!(acquire_lock_with(&dir, 100, &|_| true).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_real_process_check_sees_this_process_but_not_a_missing_one() {
        // Test binaries are not named like the app, so only check absence here.
        assert!(!is_app_process(u32::MAX - 1));
        let dir = lock_dir("real");
        let first = acquire_instance_lock(&dir).unwrap();
        // The recorded owner is this very process, which is never "another copy".
        assert!(acquire_instance_lock(&dir).is_ok());
        drop(first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_whose_app_is_gone_is_not_protected() {
        assert!(!belongs_to_other_live_app(None));
        assert!(!belongs_to_other_live_app(Some((std::process::id(), "anything"))));
        assert!(!belongs_to_other_live_app(Some((1, "explorer"))));
        let ours = our_exe_stem().unwrap();
        assert!(belongs_to_other_live_app(Some((std::process::id() + 1, &ours))));
    }
}
