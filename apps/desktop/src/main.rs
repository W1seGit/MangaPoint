//! MangaPoint: search manga, convert it for an Xteink reader, export.

// A release build is a window, not a console program.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod assets;
mod backend;
mod covers;
mod logfile;
mod logic;
mod notify;
mod pages;
mod queue;
mod reading_state;
mod sample;
mod state;
mod updates;
mod ui;

use dioxus::desktop::tao::window::Icon;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use tracing_subscriber::fmt::time::SystemTime;
use tracing_subscriber::EnvFilter;

/// What the console shows by default: MangaPoint's own progress messages
/// (installing, searching, exporting…) and real errors from anything else.
/// Set `MANGAPOINT_LOG` to change it, for example
/// `MANGAPOINT_LOG=mangapoint=debug,warn`.
const DEFAULT_LOG: &str = "mangapoint=info,error";

fn init_logging() {
    let filter =
        EnvFilter::try_from_env("MANGAPOINT_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG));
    // The log is kept in the data folder (added to at every start, rolled over
    // at a size limit); a debug build also prints it.
    let file = logfile::LogFile::open(state::paths().root().join("logs")).ok();
    // Installed before Dioxus starts, so its own logger steps aside.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_timer(SystemTime)
        .with_writer(logfile::Sink(file))
        .compact()
        .try_init();
    // A crash leaves its reason in the log.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("The app crashed: {info}");
        previous(info);
    }));
}

fn main() {
    // Developer command: write the app icon files and exit.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--write-icons") {
        let dir = args.next().unwrap_or_else(|| "assets".into());
        match assets::write_icons(std::path::Path::new(&dir)) {
            Ok(()) => println!("wrote icon.png and icon.ico to {dir}"),
            Err(e) => {
                eprintln!("could not write the icons: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    init_logging();
    // Only one copy per data folder: two would share (and fight over) the same
    // files and the same catalog engine.
    match backend::acquire_instance_lock(state::paths().root()) {
        Ok(lock) => backend::keep_instance_lock(lock),
        // A lock file that cannot be written is not worth refusing to start for.
        Err(e @ backend::LockError::Io(_)) => tracing::warn!("Running without a lock: {e}"),
        Err(e) => {
            tracing::error!("Not starting: {e}");
            eprintln!("MangaPoint: {e}");
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("MangaPoint")
                .set_description(
                    "MangaPoint is already open. Look for its window or taskbar icon.",
                )
                .show();
            std::process::exit(1);
        }
    }
    notify::register();
    // Ctrl+C in the terminal would otherwise leave the catalog server running
    // and holding its database, so the next start could not open it.
    let _ = ctrlc::set_handler(|| {
        backend::shutdown();
        std::process::exit(130);
    });
    tracing::info!(
        "MangaPoint {} starting; data folder: {}",
        env!("CARGO_PKG_VERSION"),
        state::paths().root().display()
    );
    // The cat is the window and taskbar icon (Windows and Linux; macOS takes
    // its Dock icon from the app bundle, which the installer provides).
    let (rgba, w, h) = ui::cat::icon_rgba(8);
    let window = WindowBuilder::new()
        .with_window_icon(Icon::from_rgba(rgba, w, h).ok())
        .with_title("MangaPoint")
        .with_inner_size(LogicalSize::new(1480.0, 940.0))
        .with_min_inner_size(LogicalSize::new(1280.0, 720.0));
    // Covers are handed to the page by address, not kept in memory; see `covers`.
    covers::tidy_cache();
    // Scratch files an earlier run left behind when it was closed during an
    // export (they can be gigabytes), swept off the main thread.
    std::thread::spawn(|| {
        let removed = pipeline::sweep_spool(&state::paths().spool_dir(), std::time::Duration::from_secs(24 * 3600));
        if removed > 0 {
            tracing::info!("Removed {removed} leftover scratch file(s) from an earlier export");
        }
    });
    let mut config = Config::new()
        .with_window(window)
        .with_asynchronous_custom_protocol(covers::SCHEME, |_id, request, responder| {
            covers::serve(request, responder)
        })
        // The reader's pages, straight from the library on disk; see `pages`.
        .with_asynchronous_custom_protocol(pages::SCHEME, |_id, request, responder| {
            pages::serve(request, responder)
        });
    // No menu bar on Windows and Linux. On macOS the menu is also where copy,
    // paste, select all, quit and hide come from, so the default one stays.
    #[cfg(not(target_os = "macos"))]
    {
        config = config.with_menu(None);
    }
    // Developer switch: let tools such as an automated UI test connect to the
    // app's web view. Off unless MANGAPOINT_DEBUG_PORT is set; the port only
    // listens on this computer.
    if let Ok(port) = std::env::var("MANGAPOINT_DEBUG_PORT") {
        if port.parse::<u16>().is_ok() {
            tracing::info!("UI debugging enabled on 127.0.0.1:{port}");
            config = config.with_windows_browser_args(format!(
                "--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
            ));
        }
    }
    // A copy run with its own data folder (MANGAPOINT_HOME) also gets its own
    // web view folder. Otherwise it shares one with any MangaPoint already
    // open, which blocks it from starting with different settings.
    if std::env::var_os("MANGAPOINT_HOME").is_some() {
        config = config.with_data_directory(state::paths().root().join("webview"));
    }
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(ui::App);
}
