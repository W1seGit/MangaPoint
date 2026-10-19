# Browser tests for the website

Real Microsoft Edge (or Chrome), driven over the Chrome DevTools Protocol with
real mouse, touch and keyboard input and real file choosers. No dependencies:
Node 22 (built-in `WebSocket` and `fetch`) and Python (for the static server).

## Run

```
tools\build-web.ps1                      # or tools/build-web.sh; builds web/dist
node --test web/js/test                  # unit tests (ZIP reader, importer with the real engine)
node tools/web-e2e/run-all.mjs           # every suite at desktop, tablet and phone sizes
node tools/web-e2e/e2e-read.mjs phone    # one suite at one size
```

Run the suites from an interactive terminal in the foreground: started detached in the background, Edge never comes up on Windows and the run hangs. `run-all.mjs` is therefore best run from a normal console (it takes a while); one suite takes 30-100 s.

The browser is found automatically (set `MP_BROWSER` to a path to override).
Each run starts `tools/serve-web.py` on a free port and a fresh browser profile.

## Fixtures (not committed)

`MP_FIXTURES` (default `..\import-fixtures` next to the repository): CBZ, ZIP of
chapter folders, EPUB, a folder of pictures, bad archives (`bad/`), a 20,000
entry zip and a 160-page 1.7 GB book. They come from
`.work\tmp\make_fixtures.py` of the maintainer's checkout.

## Suites

| suite | checks |
|---|---|
| `e2e-import` | every format, a folder, loose pictures, 7z/PDF/garbage/truncated/traversal/empty/non-picture files, 20,000 entries, page stays alive, no sideways scroll |
| `e2e-read` | keys, taps, swipes, pinch, RTL, chapter picker, end card, scroll mode, Continue, progress after reload, restoring the scroll position, removing a book |
| `e2e-chrome` | the reader's bars (arrows, centred chapter name, title and percentage, fades), the full-screen reading settings with the one zoom menu, bars hiding on scroll down and showing on scroll up or a tap, the swipe from the left edge |
| `e2e-export` | engine reader list, XTC/EPUB/CBZ per chapter/PDF in the worker, progress bar, downloaded bytes, cancel |
| `e2e-state` | `.mpstate` save, load in a fresh browser, waiting for file, re-import resumes, direct save through a fake File System Access API, offline reload |
| `e2e-big` | the 1.7 GB book: import without loading it, scroll all 160 pages, heap and live blob URLs stay flat |

Screenshots go to `MP_SHOTS` (default `..\web-shots`). `smoke.mjs` and `try.mjs`
are small scratch drivers.

What it cannot do: drive a native file *save* dialog (the File System Access
test replaces `showSaveFilePicker` with a fake backed by OPFS) or install the
app from the browser's menu.
