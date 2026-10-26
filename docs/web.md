# MangaPoint on the web

A website that mirrors the desktop app's look. It does **not** search or
download manga. You import your own books, read them, and export them for an
e-reader. Everything runs on the visitor's device; the host only serves static
files.

* Import: CBZ and ZIP (also a ZIP of chapter folders), EPUB with pictures, a
  folder of pictures, loose pictures (choose, or drop on the page).
  **RAR, 7z and PDF are not supported on the web yet** (the page says so).
* Read: one continuous strip, fit, zoom, chapter picker, keyboard, taps to
  show or hide the bars, and pinch.
* Export: any reader the desktop app knows, plus "Phone or tablet - original
  quality" (CBZ or PDF), in a Web Worker with a progress bar and Cancel.
* Install as an app (PWA); the library and the reader work offline.

## Privacy design

* No upload, no account, no analytics, no third-party requests. The only
  network traffic is the download of the site's own files.
* Books, covers, reading places and settings live in the browser (below). The
  server cannot see them; there is nothing to "clear" server-side.
* The service worker only caches the app's own files (a list generated at
  build time). It never intercepts `blob:` URLs, other origins or anything that
  is not on that list.
* Exports are made in the tab (a Web Worker) and handed to the browser as
  downloads. They are never sent anywhere.
* Settings > "Delete everything" clears IndexedDB, OPFS and the object URLs.

## Storage layers

| layer | holds | notes |
|---|---|---|
| OPFS (`books/<fingerprint>/...`) | the imported file (`book.bin` for an archive, one file per page for pictures) | written as a stream (`Blob.stream().pipeTo(writable)`), read back by `Blob.slice`. A 1.7 GB book is imported in seconds without entering the JS heap. |
| IndexedDB `mangapoint` | `books` (what the library shows), `pages` (page table: offsets and measured sizes), `state` (reading state by fingerprint), `covers` (320 px thumbnails), `meta` (settings, the saved-file handle), `files` (fallback home of the bytes when there is no OPFS) | writes of reading progress are debounced (800 ms) and flushed on `visibilitychange` / `pagehide`. |
| memory | blob URLs of pages on or near the screen | created on demand, revoked ~1.5 s after the page leaves. Memory is O(visible pages). |

If the browser cannot keep a file (quota, private window) the book is read for
the current visit only, is marked "Needs its file" afterwards, and importing the
same file again re-attaches it. Its reading place is stored by fingerprint, so
nothing is lost. `?nostore` in the address simulates this for testing.

A book's identity is the engine's fingerprint: SHA-256 of `"mp1|" + size + "|"`
and the first and last 256 KiB (`book_fingerprint_head_tail`). The same file
has the same identity on every device and browser. A folder or set of pictures
uses a hash of its sorted names and sizes.

## The reading-state file (`.mpstate`)

A small JSON file (a few hundred bytes per book, no pictures). The engine
(`crates/web-engine/src/state.rs`) defines, validates and merges it:

```json
{ "format": "mangapoint-state", "version": 1, "savedAt": "2026-10-19T12:00:00.000Z",
  "books": [ { "fingerprint": "<64 hex>", "size": 1113409, "fileName": "x.zip", "title": "X",
               "chapters": 3,
               "position": { "chapter": 1, "chapterName": "Ch. 3", "page": 3, "scroll": 0 },
               "chaptersRead": [0],
               "prefs": { "zoom": 100 },
               "lastReadAt": 1781000000000 } ],
  "settings": { "device": "xteink-x4-pro", "preset": "manga" } }
```

* **Save reading state** (Settings): where the browser has the File System
  Access API (Chrome, Edge) the file is chosen once and the handle kept; later
  saves write to it directly (the browser may ask permission again after a
  restart). Elsewhere the file is downloaded.
* **Load reading state**: pick the file, or drop it on the page. It is merged
  with what is here (the more recently read record of each book wins, chapters
  read are united). Books present resume at once; the others are listed under
  "Waiting for the file" in the Library and resume automatically when the
  matching file is imported.
* Only books that were opened have a place to remember.

## Limits

* Formats: CBZ/ZIP, EPUB with pictures (pictures in reading order from the
  spine), folders, loose pictures. Pictures: JPEG, PNG, WebP, GIF (and TIFF/AVIF
  where the browser shows them). Not RAR/CBR, 7z/CB7, PDF, password-protected
  archives (reported as such).
* ZIP entries: at most 100,000 pictures per book (engine limit); entries are
  inflated one at a time with `DecompressionStream('deflate-raw')`, capped at
  256 MB each, with a guard against entries that grow past what they declare.
  Names are read as UTF-8 or the DOS code page; names that leave the folder
  (`../`, absolute) are skipped and reported.
* Memory: reading is flat. **Export holds the whole result in the tab's
  memory** (and one chapter's pages while converting). The page warns above
  600 pages or ~300 MB of source pictures and offers one file per chapter.
  Phones can run out of memory on big single files. Cancel stops the worker at
  once; nothing partial is kept.
* The converter's progress is per chapter (it cannot report inside a chapter).
* Browser support: needs OPFS or IndexedDB, `DecompressionStream`, ES modules
  in workers, WebAssembly. Current Chrome, Edge, Firefox and Safari 16.4+.
  Safari does not offer the File System Access API (downloads are used).
* The browser may delete a site's data when storage is low unless the visitor
  allows persistence (Settings > "Keep my library safe", installing the app).

## Layout and files

| path | what |
|---|---|
| `apps/web` | the page: Rust, Dioxus 0.7 (`wasm32`), `model.rs` (library, state, sorting), `review.rs` (import review), `ui/*` (screens), `bridge.rs` (calls into the browser) |
| `crates/ui` (`mangapoint-ui`) | what the website and the desktop app share: the art, the small widgets, the chapter table, and the logic behind the screens (reader keys and zoom, chapter sorting and ranges, the page editor's layout, number and date wording) |
| `crates/ui/css/*.css` | the look shared with the desktop app, one file per screen, joined in the order of `crates/ui/css/ORDER` into `css/ui.css` at build time |
| `apps/web/css/*.css` | only what a browser and a phone add (safe areas, 44 px targets, bottom tab bar under 800 px, no sideways scroll), joined in the order of `apps/web/css/ORDER` into `css/web.css` |
| `crates/formats` | shared CBZ/EPUB/PDF byte writers, original-quality page helpers, ComicInfo and filename readers; desktop streams to files, browser writes to memory |
| `crates/web-engine` | the engine: chapter guessing, fingerprints, state file, devices, options (`core`, linked into the page) and the converter/writers (`convert`, its own `.wasm`, loaded only by the export worker) |
| `apps/web/js` | browser glue as plain ES modules: `zip.js` (lazy ZIP reader), `store.js` (IndexedDB/OPFS), `importer.js`, `pages.js` (blob URL lifecycle), `reader.js` (tiles, keys, touch), `fsa.js`, `export-worker.js`, `mp.js` (the bridge) |
| `apps/web/assets/sw.js`, `apps/web/assets/manifest.webmanifest`, `apps/web/assets/icons` | service worker template, manifest, icons (`tools/web-icons.py` makes them from the desktop icon) |

## Build

Needs Rust with `wasm32-unknown-unknown`, `wasm-bindgen-cli` **0.2.129**
(`cargo install wasm-bindgen-cli --version 0.2.129`; `dx` is not needed), Node
18+ and Python (server, icons).

```
tools\build-web.ps1            # or tools/build-web.sh   -> apps/web/dist
tools\build-web.ps1 -NoEngine  # page only (no export), faster
tools\serve-web.ps1            # http://127.0.0.1:8080/
```

`apps/web/dist` is the whole site. The build compiles `mangapoint-web` and
`web-engine` for `wasm32`, runs `wasm-bindgen --target web`, copies `apps/web/assets`, `apps/web/js`, `apps/web/css` and
the desktop style sheets, then `tools/web-precache.mjs` hashes every file and
writes `sw.js` with a versioned precache list (the version is a hash of all
files and of the worker itself, so any change is a new cache and the old one is deleted).
Hosts may redirect a file (Cloudflare sends `/index.html` to `/`); the worker stores what the
redirect led to, since a browser refuses to show a page that a worker answers with a redirect.

Tests: `cargo test -p formats -p mangapoint-ui -p mangapoint-web -p web-engine`,
`node --test apps/web/js/test`,
and the real-browser suites in `tools/web-e2e/` (see its README).

## Hosting

Any static host works: copy `apps/web/dist` to it. Requirements: serve `.wasm` as
`application/wasm` and `.js` as a JavaScript type; HTTPS (or `localhost`) so the
service worker and OPFS are available. All URLs are relative, so a sub-path
such as `https://user.github.io/MangaPoint/` works. No special headers are
needed (no `SharedArrayBuffer`).

The two sites are on Cloudflare (Workers with static assets), on the domain's own
Cloudflare account:

* **The web app, app.mangapoint.org** (`apps/web/wrangler.jsonc`). It needs Rust to
  build, so GitHub builds it: `.github/workflows/web.yml` builds and tests pull
  requests, and on a push to `main` (or "Run workflow" on `main`) it also deploys
  `apps/web/dist` with `wrangler deploy`. That needs two repository secrets:
  `CLOUDFLARE_API_TOKEN` (a token made from the "Edit Cloudflare Workers" template,
  for this account and the mangapoint.org zone) and `CLOUDFLARE_ACCOUNT_ID`.
  Unknown addresses get the app's page (`single-page-application`), so its routes work.
* **The home page, mangapoint.org** (`apps/site/wrangler.jsonc`). It needs no
  compiler, so Cloudflare builds it from the repository: Workers & Pages, Create,
  import the repository, project name `mangapoint`, root directory `apps/site`,
  build command `bash ../../tools/build-site.sh`, deploy command `npx wrangler deploy`.

Both domains are made by the first deploy (`custom_domain` in each file); there must
be no other DNS record for that name.
