# Changelog

All notable changes to MangaPoint are written down here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/).

## [2.0.0] - 2026-10-25

### What's new

- **A new look.** The app follows a new design: a dock at the bottom, a corner bar for Tasks and Settings, pixel-style headings and figures, and calm page-to-page motion.
- **A new mascot** replaces the cat everywhere, and is the new app icon. It sleeps on an empty bookcase, holds up a magnifying glass when a search finds nothing, has a cup of tea when there is nothing to do, walks along a road while the app starts, and goes round a circle while something loads.
- **Read in the app.** Downloaded and imported series open in a reader that shows a chapter as one long strip, with a chapter picker, Previous and Next at the end of a chapter, reading settings, and zoom (Ctrl + and Ctrl −) that keeps your place.
- **Import your own comics and books** from the Library, with its Import button or by dropping them on it: CBZ, CBR, ZIP, 7z, EPUB, PDF, folders of pictures, and a series folder from MangaPoint's own library. You check each book's chapters before they are added.
- **The MangaPoint website.** Read, import and export your own books in the browser, on a computer, a tablet or a phone. It works like the app (Library, a book's overview, the reader, Export and Settings) and keeps everything on your device.
- **Keep colour** in EPUB and CBZ exports for colour e-readers, with a colour preview.
- **Original quality** exports for phones and tablets, as CBZ or PDF, with the pages exactly as they were downloaded.
- **Pause an export** and resume it later, even after closing the app. The running task shows in the corner bar with the series cover.
- **A new Library**: smaller cards with a tick and a menu, a title and views that stay at the top while you scroll, a selection bar in the bottom left, an Information sheet for each series, and Esc goes back from an open series.
- **Your places travel**: the app saves and loads the same reading-state file as the website.

### Changes

- The reader has one mode, a long strip at a width that suits the window. The paged mode and the reading direction setting are gone.
- New settings start with mixing sources off.

### Fixes

- A damaged library file no longer starts the app with an empty library.
- Importing can be cancelled, is all-or-nothing, and is safe against deleted, renamed and oversized files.
- Pressing Pause near the end of an export holds the exports waiting behind it, and deleting a paused series moves the next export up.
- Menus open above everything on the page, and dialogs cover the dock.
- A series page lists the chapters on this computer even when none of its sources answers.
- Install all recommended installs the recommended sources again when the catalog engine has lost them, instead of saying they are installed.

### Changes under the hood

- The app and the website share one set of screens, styles and page scripts (`crates/ui`), and one set of file writers (`crates/formats`).

## [1.1.1] - 2026-10-18

### Fixes

- Exporting to a reader that opens only EPUB (the Kindles) now makes an EPUB. Before, a series added after the reader was chosen could still be written as an XTC file, which a Kindle cannot open.

### Changes under the hood

- split the largest source files into one file per topic

### Build

- update png to 0.18 and stop Dependabot opening pull requests
- update every dependency and group the Dependabot updates

## [1.1.0] - 2026-10-17

### What's new

- **MangaPoint now tells you when there is a new version, on every system.** It checks once a day and shows what's new. It downloads and checks the update for you. Windows installs it and restarts; on macOS and Linux it hands you the file or the command to finish.
- **Choose Portrait or Landscape for every export**, with a Landscape mode that picks Fit for double-page spreads.
- **Zoom past Fit.** Cut a page into pieces or crop to the centre, and drag a handle on the page to set the zoom.
- **Export as CBZ**, a comic archive carrying the series, the author and the chapters, for readers and apps that open comics.
- **Pick a range of chapters** with the new range bar beside "Every chapter".

### Export screen

- A page's source now always shows the original picture, next to every page it makes. A normal page is shown at the reader's page height.
- The pages redraw while you drag the zoom handle.
- Optional guide arrows on pages that carry on to the next one. They are small and sit near the page edge.
- The Edit controls are folded into four groups. Any number can be open, they start closed, and they stay as you leave them.
- The screen says when your reader is found over USB, and notices when it is unplugged.
- "Last export" says "edited" when you have changed settings since your last export. Only a real export clears it.

### Fixes

- Clicking a zoom handle no longer jumps the zoom to 25%.
- Taking the last series out of an export brings back the sample pages.

### Updating

MangaPoint 1.1.0 and later find updates on their own: **Settings → About → Check for updates**.
Versions 1.0.0 and 1.0.1 cannot, so if you are on one of them, update once by hand. It takes a
minute on every system: [how to update by hand](https://github.com/W1seGit/MangaPoint/blob/main/docs/updating.md).
Your library, settings and engine are kept.

**Full changes:** [#24](https://github.com/W1seGit/MangaPoint/pull/24) · [#25](https://github.com/W1seGit/MangaPoint/pull/25) · [#27](https://github.com/W1seGit/MangaPoint/pull/27) · [#28](https://github.com/W1seGit/MangaPoint/pull/28)

## [1.0.1] - 2026-10-15

### Build

- A Windows installer (`MangaPoint-…-windows-x86_64-setup.exe`): it installs for the current user without asking for administrator rights, adds a Start menu entry and an entry in Apps → Installed apps, and leaves your library and settings alone when uninstalled. The program on its own is still there as `…-portable.exe`.
- An `.rpm` for Fedora, openSUSE and RHEL.
- The `.deb` names exactly the libraries it needs.
- A merge to `main` no longer builds every package twice.

## [1.0.0] - 2026-10-15

The first release.

### Features

- Search many sources at once; results for the same series are grouped and the best source is chosen from its chapter list. Covers, descriptions and details come from whichever source answers first.
- Download chapters through a queue with pause, resume, retry and reordering, with a library of what you have.
- Convert pages for the reader's own screen: automatic and manual crop, double-page spreads, long strips cut into slices, contrast and dithering, with a live preview and presets.
- Export as XTC, XTCH or a fixed-layout EPUB 3 (chapters as the table of contents, reading direction, cover), to a folder, a USB drive or SD card, or over Wi-Fi.
- A catalog of 29 readers (Xteink, Amazon, Kobo, Boox, PocketBook, reMarkable) with their sizes, densities and file formats; add your own. Each reader has its own folder, export method, firmware and formats, and the Export screen only offers the formats the reader opens.
- A first-run setup (engine, sources, reader), notifications when a download or an export finishes, keyboard shortcuts, and settings that warn before unsaved changes are lost.

### Performance

- Covers are served on demand from a small disk cache instead of being held in memory, and the chapter table draws only the rows in view, so long series stay light.
- Search shows its results as soon as every source has answered.
- Exports run on their own threads, and the screens no longer redraw for every downloaded page.

### Build

- Continuous integration on Windows, Linux and macOS, and automatic packaging and publishing of releases.
