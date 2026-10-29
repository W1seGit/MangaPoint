# MangaPoint architecture

A component or function that both apps show or decide lives in `crates/ui`, or in
`crates/formats` when it makes or reads file bytes. App folders hold what the other
app cannot use: the desktop engine, OS dialogs and filesystem, or browser routing,
OPFS and workers. Differences go through props, callbacks, slots or `Host` methods.
Do not copy a shared implementation into an app to change it.

## Crates and applications

| Location | Responsibility |
|---|---|
| `crates/ui` (`mangapoint-ui`) | Dioxus components without a renderer feature: art, widgets, chapter table, readers editor, conversion controls, reader chrome, panels, poster cards, action bar and dialogs |
| `crates/ui/src/logic` | Pure reader input/zoom/progress, chapter sorting/ranges, preview layout and number/date wording; tested natively |
| `crates/formats` | Generic CBZ/EPUB/PDF writers, original-quality image helpers, ComicInfo parsing and filename rules |
| `crates/converter` | Decode, prepare, plan and render pages; XTC/XTCH encoding |
| `crates/library` | Persistent models, settings, presets, reading progress, paths and safe library file operations |
| `crates/catalog`, `sources`, `search` | Engine provisioning/client, source management, streamed search and merged results |
| `crates/pipeline` | Desktop import/download/export orchestration, file IO, spool and output targets; thin file wrappers around `formats` |
| `crates/web-engine` | Browser engine API, state merge/fingerprints, chapter guessing and export orchestration; calls `formats` |
| `apps/desktop` | Native shell, engine supervisor, queue runner, OS integration, desktop screens and adapters |
| `apps/web` | Browser shell/routes, library/import/export adapters, browser settings and bridge |
| `apps/web/js` | IndexedDB/OPFS, archive import, page URL lifetime, reader events, workers and browser APIs |
| `apps/site` | The home page (mangapoint.org): plain HTML, CSS and a little JS, with no build step of its own |

The home page uses the app's own `base.css` and fonts (as files, `crates/ui/assets/fonts/web`), the Search page's clouds and rocks, and the
mascot's scenes. Those are SVG files in `apps/site/art` made from the drawings in `crates/ui/src/art` by
`SITE_ART=write cargo test -p mangapoint-ui site_art`; the same test fails when they fall behind.
`tools/build-site.sh` (or `.ps1`) copies it all into `apps/site/dist`, which any static host can serve (its
screenshots are WebP, made by `tools/make-site-images.py`; `_headers` sets how long Cloudflare lets a browser keep
fonts and pictures); it
is on Cloudflare at mangapoint.org (`apps/site/wrangler.jsonc`, and docs/web.md, Hosting).

The look (palette, type, figures, the dock and the corner bar) is described in [design.md](design.md).

Browser adapters are named `Browser*` to distinguish them from shared components and
from the native adapters. A structural check rejects duplicate app component names.

The app-specific screens adapt different domain models: downloaded series and chapter
keys on desktop, imported books and page indexes on the web. Keep their storage and
navigation adapters local; extract reusable presentation and decisions into `ui`.

## Host and shared state

Both `AppState` and `Ctx` contain a `UiState` and provide it as Dioxus context.
`UiState.settings` is the single `library::Settings` signal for readers and conversion
preferences. It also owns unsaved reader edits, the leave prompt, toasts, toast hover
state and the conversion groups. Advanced conversion is a persisted settings field.
The web's separate `choices` signal stores browser-specific export/reading choices.

`UiState` holds an `Rc<dyn Host>` through its host signal. `DesktopHost` and `WebHost`
provide persistence, reader selection/save side effects, opening reader settings,
confirmation actions, sleeps, logging and named scripts. Capabilities hide USB/folder
and Wi-Fi fields where sending to a reader is unavailable. The desktop implementation
also performs Wi-Fi checks. Tasks use Dioxus's shared local executor.

Navigation to an app page is a `Leave::Elsewhere` callback. Choosing another reader is
`Leave::Reader`; both pass through the same unsaved-settings guard. The confirmation
shell takes props and callbacks, while each app keeps its own action enum and modal
visibility. Reader chrome, poster cards, selection bars, chapter range controls, preview zoom and folding panels likewise
receive data and report actions without knowing the app's state.

## File bytes and output ownership

`formats` has no UI or filesystem paths. Its default `write` feature enables the
writers and image codecs. With default features disabled only ComicInfo and filename
readers are built, keeping the browser page's core engine small.

ZIP writers accept `Write + Seek`; PDF accepts `Write`. Finishing returns the output
writer and propagates write/flush errors. Pipeline wrappers use `BufWriter<File>` and
attach the destination path to errors; browser exports use `Cursor<Vec<u8>>` or
`Vec<u8>`. Streaming desktop exports therefore do not accumulate a whole book in RAM.
The web engine retains its memory limits and worker cancellation boundary.
`web-engine/tests/parity.rs` compares desktop and browser exports byte for byte.

## CSS and JavaScript

`crates/ui/css/ORDER` lists every shared stylesheet once, in cascade order. A build
script exposes their text as `mangapoint_ui::CSS`; both web build scripts concatenate
the same list into `css/ui.css`. Desktop adds `apps/desktop/css/desktop.css`.
The browser adds `apps/web/css/ORDER` as `css/web.css`: safe areas, dynamic viewport,
touch targets, narrow layouts and browser-only screens. Shared selector overrides
must be responsive rules or explain the browser requirement in a comment.

Shared classic scripts live in `crates/ui/js`, registering `window.MP.<name>`.
`mangapoint_ui::scripts` maps their names and builds calls. Desktop evaluates the
shared source; the browser build copies it into `js/ui` and its bridge loads/runs it.
Tooltip and zoom scrubbing initialize once; splitters and preview editors bind each
new screen. App-specific reader event transports remain beside their hosts.

## Verification and future work

Run `powershell -NoProfile -File tools/check-all.ps1` from a checkout. It builds both
apps, runs native and wasm Clippy with warnings denied, crate tests, byte parity,
the website build and browser suites at desktop/tablet/phone sizes, and desktop
CDP interactions using a temporary data root. `-Quick` omits browser/UI runs.
Standalone `formats --no-default-features` and wasm conversion checks also guard
feature boundaries. Style dump tools compare computed styles across screen captures.
Before calling cross-platform verification complete, build and inspect the Mac app
on BuildBox as well. An unreachable machine is an outstanding check, not a pass.

Self-hosting remains a separate [plan](plans/self-hosting.md). This cleanup does not
add an engine service, server, CLI, authentication or live sync. The official site
stays static and all book processing stays on the visitor's device.
