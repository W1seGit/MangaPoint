# Changelog

All notable changes to MangaPoint are written down here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/).

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
