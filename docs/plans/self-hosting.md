# Plan: a self-hosted MangaPoint server (placeholder)

Status: **plan only, nothing is built.** This file describes where a self-hosted MangaPoint is meant to go,
and what the code has to look like before it can. The code structure it relies on is described in
[`docs/architecture.md`](../architecture.md).

## Goal

Anyone can run their own MangaPoint server (`mangapoint-server`, with a `mangapoint` command-line tool) on a PC,
a NAS or a small VPS. It is theirs alone: there is no account with us and no server of ours in the loop.

- **The server's own site.** The server serves the MangaPoint website with **searching and downloading turned
  on**, using the server's sources and its library. It is the website the official site is, plus the parts only
  a machine with the engine can do.
- **Syncing.** The official website (the static site that has no server) and the desktop app can each be given
  the server's address (an "API link"). On the official site that link is **only for syncing**: the visitor's
  reading places, chapters read, readers, presets and the list of books. Nothing else is fetched through it.
- **Everything on the client.** The official site stays a static site. Every request to a user's server is made
  by the visitor's own browser, straight to their server. The host of the official site never sees, proxies or
  stores any of it.

## Decisions taken

| Question | Decision |
|---|---|
| One user or several? | **One user for now, with sign-in.** Every stored record and every API route carries a user id from the start (always the one user today), so more users can be added later without changing the data or the API. |
| How is it installed? | **Two ways:** a single command (`curl … \| sh` on Linux/macOS, a `winget`/PowerShell one-liner on Windows) that installs the binary and its Java runtime, and a **Docker** image (`docker run -v mangapoint:/data -p 8686:8686 mangapoint/server`, plus a `compose.yaml`). |
| What does deleting do? | **A delete reaches every linked place.** Each book or series has a home: on this device or on the server. Deleting something removes it where it lives and from every client's library list. A copy on a device that is only a download of a server book can be removed from the device without deleting it from the server ("Remove download" against "Forget"). |
| Local and server downloads | **Kept apart, and synced.** Downloads and imports made in the desktop app stay on that computer. Downloads made on the server stay on the server. The desktop app can push its local library list and reading state to the server and pull the server's, so both know about everything. Pages only move when the user asks (download a server book to the device, or upload a local book to the server). |
| The official site and the server | **Sync only, all client-side.** The site never asks the server to search or download. It sends and receives the reading state, from the browser, over HTTPS. |

## What already exists to build on

These crates already have no UI and can be used from a server or a command line as they are:

- `crates/catalog`:
  - fetches the Suwayomi engine and checks it (`provision.rs`);
  - starts and stops it (`RunningServer`);
  - talks to it (`SuwayomiClient`);
  - defines the `Catalog` trait the rest of the code uses, so another engine could stand in.
- `crates/sources`: installing and checking sources, and the stores they come from.
- `crates/search`: asking every source at once (`search_stream`), grouping, ranking and merging chapters.
- `crates/pipeline`: downloading chapters (`download_chapters_as`), resuming, trying other sources, and exporting
  (`export_series`).
- `crates/library`:
  - one data folder (`AppPaths`), with crash-safe JSON files (`JsonFile`);
  - the library and reading progress (`LibraryDb`, `ReadProgress`), and the settings.
- `crates/formats`: the CBZ, EPUB, PDF and ComicInfo writers.
- `crates/ui`: the shared screens. The server's own site uses them through a third `Host` (see below).
- The checks that keep a page request inside the library, in `apps/desktop/src/pages.rs` (series and chapter must
  exist, path must stay inside the library, symlinks resolved), to be moved somewhere both can use.

## What has to move first

Before a server can exist, the parts that run the engine and the downloads have to stop depending on the
desktop window:

1. **A new `crates/engine`**, taken out of the desktop app:
   - **the engine supervisor:** starting, watching and restarting Suwayomi, and the one-copy-per-data-folder lock.
     It moves from `apps/desktop/src/backend.rs`, minus the Dioxus `spawn` and `AppState` signals.
   - **the download queue:** already plain data (`apps/desktop/src/queue/`).
   - **the queue runner:** `apps/desktop/src/ui/work/download.rs` and `ui/work/mod.rs`, which now read and write
     `AppState`.
   - **the save batching:** `apps/desktop/src/state/saver.rs`.

   Together they become one `Engine` service with a stream of events (`JobEvent`, queue changes, engine state).
   The desktop app becomes one client of it, through a thin layer that turns events into its signals. The server
   and the command line are two more.
2. **One shared identity for a book.** Today a book is identified three different ways:
   - **web imports:** a content fingerprint, `web_engine::fingerprint::book_fingerprint_head_tail`;
   - **downloaded series:** an id made from the title, `library::series_id`, with chapter keys such as `c12`;
   - **desktop imports:** a `path|size|mtime` string.

   An `ItemId` covers both cases that matter: a downloaded series (series id plus chapter key) and an imported file
   (content fingerprint). The desktop import switches to the content fingerprint. The reading-state file (`.mpstate`)
   carries `ItemId`s, and `BookState::to_progress`/`set_progress` learn to map desktop chapter keys, not only chapter
   positions.

## The pieces

- **`apps/server` (`mangapoint-server`).** A long-running service:
  - the `Engine`;
  - an HTTP API (axum on the tokio the workspace already uses);
  - the server's own site, which is the same website built with a `ServerHost`.

  It binds to `127.0.0.1` unless the user says otherwise.
- **`apps/cli` (`mangapoint`).** The same `Engine` from a terminal. It can:
  - `mangapoint search "title"`;
  - `mangapoint download <series> 1-20`;
  - `mangapoint export <series> --reader "Xteink X4" --to ./out`;
  - `mangapoint serve`, which starts the server;
  - `mangapoint user` / `token` to manage sign-in.
- **`ServerHost`.** A third implementation of `crates/ui`'s `Host`, beside the desktop and web ones. The shared
  screens then work against the API with no change of their own.

## API sketch (v1)

HTTP with JSON, server-sent events (SSE) for progress, versioned under `/api/v1`. Every route except `/health`
needs a token. Every route works for "the signed-in user", so a second user is a data change, not an API change.

| Route | What it does |
|---|---|
| `GET /health` | up, version, engine state (no token) |
| `POST /auth/login`, `POST /auth/tokens`, `DELETE /auth/tokens/{id}` | sign in, make a token for a device, revoke it |
| `GET/POST/DELETE /sources` | list, install, remove sources |
| `GET /search?q=` (SSE) | results as they arrive, grouped |
| `GET /series/{id}`, `GET /series/{id}/chapters` | details, merged chapter list |
| `POST /downloads`, `GET /downloads` (SSE) | queue chapters; watch the queue |
| `GET /library`, `DELETE /library/{item}` | the user's library; forget a book everywhere |
| `GET /pages/{item}/{chapter}/{file}` | one saved page (path checks as in `pages.rs`) |
| `POST /exports`, `GET /exports/{id}` | make an export file on the server, then fetch it |
| `GET/PUT /sync/state` | the reading state, merged (see below) |

## Sync

- **The state.** The data synced is the reading-state file already used by the website (`web_engine::state::State`),
  extended with `ItemId`s, each item's home (`device` or `server`), and deletes.
- **Merging.** It reuses `web_engine::state::merge`: for each book the newer `lastReadAt` wins, chapters read are
  united, and settings come from the newer save.
- **Deletes.** A delete is kept as a dated tombstone. A tombstone newer than the item's last change wins, so a
  forget reaches every client, and an old client cannot bring a deleted book back.
- **Offline first.** Every client works without the server and syncs when it can reach it. A client that has never
  synced sends everything it has; the server merges and sends back the result.
- **From the official site.** In Settings there is "Sync with your server": the visitor types the address and
  signs in once, and the browser keeps the token. From then on the page `GET`s and `PUT`s `/sync/state` by itself
  when it opens, when it is hidden and every few minutes. The site's host is never involved.
- **From the desktop app.** The same setting, plus "Use this server's library", which shows the server's books
  beside the local ones and can download a server book to the computer or upload a local one.

## Security

- **Off by default.** The server listens on `127.0.0.1`; reaching it from other devices is a choice the user makes,
  with a warning.
- **Sign-in.** One user with a password (argon2), set up on first run (`mangapoint user add`). Devices get their
  own tokens, shown once and revocable, so a lost phone can be cut off.
- **The browser.** CORS allows the official site's origin only when the user adds it in the server settings. The
  official site must reach the server over HTTPS (a page on `https://` cannot call `http://`). The docs give
  reverse-proxy recipes (Caddy, nginx, Tailscale) instead of the server handling certificates itself.
- **Limits.** Rate limits on sign-in and search, size caps on uploads and sync, page requests held inside the
  library, and no telemetry.
- **Privacy wording.** The website's privacy note changes when a server is linked: the books and places go to the
  visitor's own server, and nowhere else.

## Installing and running

- **One command.** It installs `mangapoint-server` and the `mangapoint` tool, finds or installs a Java runtime for
  the engine, makes the data folder, asks for the user's password and starts the service (systemd on Linux,
  launchd on macOS, a Windows service). The same command upgrades.
- **Docker.** One image with Java, and a volume for the data folder (`/data`, the `AppPaths` root). A
  `compose.yaml` comes with a reverse proxy commented in.
- **One process per data folder.** The engine's instance lock already enforces it. The desktop app and a server on
  the same computer keep different data folders and sync like any two clients.

## Phases

1. Extract `crates/engine`. The desktop app runs on it with no visible change.
2. Introduce the shared `ItemId`, and the content fingerprint for desktop imports.
3. Write `apps/cli`: search, download and export from a terminal.
4. Write `apps/server`, read-only first: health, sign-in, library, pages.
5. Add search, sources and downloads over the API, and the server's own site through a `ServerHost`.
6. Add sync: `/sync/state`, tombstones, the desktop and official-site settings.
7. Add the one-command install, the Docker image, the docs and the reverse-proxy recipes.

## Still open

- Upload a local book to the server: the whole file, or only the pages?
- Should a server copy of a download count against device storage in the desktop app's numbers?
- How long are tombstones kept?
- Is the Java runtime bundled in the one-command install, or installed from the system's packages?
