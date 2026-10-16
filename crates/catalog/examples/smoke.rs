//! Live end-to-end check against a real Suwayomi (needs network).
//!
//!   cargo run -p catalog --example smoke -- <work-dir> [--browser] [--only uk.zenko,uk.honeymanga]
//!
//! Provisions Suwayomi into `<work-dir>/suwayomi`, starts it with data in
//! `<work-dir>/data`, installs the starter sources and, for each, runs the
//! checks a source needs to pass: popular list, search, chapter list, page
//! list, and fetching one real page image.

use catalog::browser;
use catalog::release::current_asset;
use catalog::{provision, Browse, CatalogError, Progress, RunningServer, ServerConfig};
use std::path::PathBuf;
use std::time::Instant;

/// Keiyoushi's extension store index, added with `addExtensionStore`.
/// (The old `extensionRepos` setting is deprecated and rewrites GitHub URLs
/// to a stub `index.min.json` with two entries.)
const STORE: &str = "https://github.com/keiyoushi/extensions/raw/repo/index.pb";

/// (display name, extension package suffix)
const STARTERS: &[(&str, &str)] = &[
    ("MangaDex", "all.mangadex"),
    ("MangaFire", "all.mangafire"),
    ("MangaPlus", "all.mangaplus"),
    ("Webtoons", "all.webtoons"),
    ("MangaPill", "en.mangapill"),
    ("MangaBuddy", "en.mangabuddy"),
    ("MangaKatana", "en.mangakatana"),
    ("Asura Scans", "en.asurascans"),
    ("Flame Comics", "en.flamecomics"),
    ("HoneyManga", "uk.honeymanga"),
    ("MANGA/in/UA", "uk.mangainua"),
    ("Zenko", "uk.zenko"),
    ("Mangarama", "uk.pureskill"),
    ("DGManga", "uk.dgmanga"),
    ("Faust", "uk.faust"),
    ("ComixTopia", "uk.comixtopia"),
    // Expected to need the browser / FlareSolverr:
    ("Comikey", "all.comikey"),
    ("Manganato", "en.manganelo"),
    ("Mangakakalot", "en.mangakakalot"),
];

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nFAILED: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let work: PathBuf = args
        .first()
        .ok_or("usage: smoke <work-dir> [--browser] [--only a,b]")?
        .into();
    let browser_on = args.iter().any(|a| a == "--browser");
    // `--only uk.zenko,uk.honeymanga`: check just these starters.
    let only: Option<Vec<&str>> = args
        .iter()
        .position(|a| a == "--only")
        .and_then(|i| args.get(i + 1))
        .map(|list| list.split(',').collect());
    let starters: Vec<(&str, &str)> = STARTERS
        .iter()
        .copied()
        .filter(|(_, suffix)| only.as_ref().is_none_or(|o| o.contains(suffix)))
        .collect();

    println!("== provisioning Suwayomi");
    let asset = current_asset()?;
    let mut last_pct = 101;
    let install = provision(&work.join("suwayomi"), &asset, |p| match p {
        Progress::Downloading {
            done,
            total: Some(t),
        } => {
            let pct = (done * 100 / t) as i32;
            if pct / 10 != last_pct / 10 {
                println!("   downloading {pct}%");
                last_pct = pct;
            }
        }
        Progress::Verifying => println!("   verifying checksum"),
        Progress::Extracting => println!("   extracting"),
        _ => {}
    })
    .await?;
    println!("   java: {}", install.java.display());

    println!("== starting server (browser support: {browser_on})");
    let config = ServerConfig {
        install,
        data_dir: work.join("data"),
        browser_support: browser_on,
    };
    let t = Instant::now();
    let server = RunningServer::start(&config).await?;
    let info = server.client.about().await?;
    println!(
        "   {} {} on 127.0.0.1:{} ready in {:.1}s",
        info.name,
        info.version,
        server.port,
        t.elapsed().as_secs_f32()
    );

    println!("== security checks");
    let anon = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/api/graphql", server.port))
        .json(&serde_json::json!({"query": "{ aboutServer { name } }"}))
        .send()
        .await?;
    println!("   request without password -> HTTP {}", anon.status());
    assert_eq!(
        anon.status().as_u16(),
        401,
        "server must reject unauthenticated requests"
    );
    println!(
        "   kcefEnabled in server = {}",
        server.client.kcef_enabled().await?
    );
    println!(
        "   browser runtime on disk: {}",
        browser::is_installed(&config.data_dir)
    );

    println!("== extensions");
    server.client.add_extension_store(STORE).await?;
    let extensions = server.client.fetch_extensions().await?;
    println!("   {} extensions available from the repo", extensions.len());
    // Suwayomi reports a repo it cannot read as an empty list, not an error.
    assert!(
        extensions.len() > 100,
        "the extension repo did not load; see the server log"
    );
    for &(name, suffix) in &starters {
        let pkg = format!("eu.kanade.tachiyomi.extension.{suffix}");
        if extensions.iter().any(|e| e.pkg_name == pkg) {
            server.client.install_extension(&pkg).await?;
        } else {
            println!("   {name}: NOT IN REPO ({pkg})");
        }
    }
    let sources = server.client.list_sources().await?;

    println!("== per-source check (popular -> search -> chapters -> pages -> image)");
    let mut passed = 0;
    for &(name, suffix) in &starters {
        // Match the source by the extension that provides it.
        let pkg = format!("eu.kanade.tachiyomi.extension.{suffix}");
        let lang = suffix.split('.').next().unwrap_or("all");
        let Some(source) = sources
            .iter()
            .filter(|s| s.extension_pkg.as_deref() == Some(pkg.as_str()))
            .find(|s| s.lang == lang || s.lang == "en" || s.lang == "all")
        else {
            println!("{name:<14} no matching source after install");
            continue;
        };
        match check_source(&server, &source.id).await {
            Ok(summary) => {
                passed += 1;
                println!("{name:<14} PASS  {summary}");
            }
            Err(e) => println!("{name:<14} FAIL  {}", describe(&e)),
        }
    }
    println!("\n{passed} sources passed");

    if browser_on {
        println!(
            "browser runtime on disk after run: {} ({:.0} MB)",
            browser::is_installed(&config.data_dir),
            browser::size_on_disk(&config.data_dir) as f64 / 1e6
        );
    } else {
        println!(
            "browser runtime on disk after run: {}",
            browser::is_installed(&config.data_dir)
        );
    }
    server.stop().await?;
    println!("server stopped");
    Ok(())
}

async fn check_source(server: &RunningServer, source_id: &str) -> Result<String, CatalogError> {
    let c = &server.client;
    let t = Instant::now();
    let popular = c.browse(source_id, &Browse::Popular, 1).await?;
    let first = popular
        .mangas
        .first()
        .ok_or_else(|| CatalogError::BadResponse("popular list is empty".into()))?;
    let found = c
        .browse(source_id, &Browse::Search(first.title.clone()), 1)
        .await?;
    if !found.mangas.iter().any(|m| m.title == first.title) {
        return Err(CatalogError::BadResponse(format!(
            "search did not return '{}'",
            first.title
        )));
    }
    // Prefer a series with several chapters so the page check is meaningful.
    let mut chapters = Vec::new();
    let mut chosen = first;
    for m in popular.mangas.iter().take(5) {
        let ch = c.fetch_chapters(m.id).await?;
        if ch.len() > chapters.len() {
            chapters = ch;
            chosen = m;
        }
        if chapters.len() >= 5 {
            break;
        }
    }
    let chapter = chapters
        .last()
        .ok_or_else(|| CatalogError::BadResponse("no chapters".into()))?;
    let pages = c.chapter_pages(chapter.id).await?;
    let first_page = pages
        .first()
        .ok_or_else(|| CatalogError::BadResponse("chapter has no pages".into()))?;
    let bytes = c.get_bytes(first_page).await?;
    let kind = image_kind(&bytes).ok_or_else(|| {
        CatalogError::BadResponse(format!(
            "first page is not an image ({} bytes)",
            bytes.len()
        ))
    })?;
    Ok(format!(
        "{} popular, '{}' {} chapters, {} pages, page 1 = {} {} KB  ({:.1}s)",
        popular.mangas.len(),
        chosen.title.chars().take(24).collect::<String>(),
        chapters.len(),
        pages.len(),
        kind,
        bytes.len() / 1024,
        t.elapsed().as_secs_f32()
    ))
}

fn describe(e: &CatalogError) -> String {
    match e {
        CatalogError::NeedsBrowser => "needs browser support (as designed)".into(),
        CatalogError::NeedsFlareSolverr => "needs FlareSolverr (as designed)".into(),
        other => other.to_string(),
    }
}

fn image_kind(b: &[u8]) -> Option<&'static str> {
    match b {
        [0xFF, 0xD8, 0xFF, ..] => Some("JPEG"),
        [0x89, b'P', b'N', b'G', ..] => Some("PNG"),
        [b'G', b'I', b'F', ..] => Some("GIF"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("WebP"),
        _ => None,
    }
}
