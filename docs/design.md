# The design

The look of both apps comes from the Figma file "MangaPoint" (pages "New Design" and "Old Design").
The Figma file is the source of truth: when the two disagree, change the code.

## Palette

The names are those of the variables in the Figma frames. They are the custom properties at the top of
`crates/ui/css/base.css`, and every other style sheet uses them (never a loose colour).

| Figma variable | Property | Value | Used for |
|---|---|---|---|
| `primary/dark-background` | `--dark` | `#161210` | the dock, menus, inputs' dark fill, text on the chosen item |
| page | `--bg` | `#1e1916` | the page |
| `primary/sub-background` | `--sub` | `#3b2f2a` | the search bar, cards, dialogs, inputs |
| `primary/thirdenary` | `--third` | `#45362e` | pills, the bottom of a card, buttons |
| `text/icon-color` | `--icon` (`--text`, `--accent`) | `#aa8d7e` | icons, text, the chosen item, the Read button |
| `text/placeholder` | `--placeholder` | `#63534a` | the hint in the search box |
| `border/default-radius` | `--radius-lg` | `25px` | cards, buttons, the search bar |
| (small pills) | `--radius` | `13px` | Sort and Sources |

## Type

* **Pixelify Sans**: the name of the app, the names of things (headings, the dock's label, the Settings list).
  `--font-pixel`.
* **Inter**: everything else. `--font-ui`.

Both are under the SIL Open Font License (see `crates/ui/assets/fonts`). `crates/ui/css/fonts.css` holds them as
small embedded web fonts (Latin, Latin Extended, Cyrillic), so the website and the app need no download. To
make it again from the two variable fonts of `github.com/google/fonts`:

    python tools/make-fonts.py Inter.ttf PixelifySans.ttf crates/ui/css/fonts.css

## Figures

The SVGs drawn in Figma are in `crates/ui/assets/svg` as they were drawn (icons take `currentColor`), and
`crates/ui/src/art/design.rs` serves them by name (`Figure { name: "nav-search" }`). The pixel clouds and rocks
behind Search are `search-cloud-*` and `search-rock-*`.

## The frame

`crates/ui/src/shell.rs` and `css/shell.css`: the dock at the bottom centre (Search, Library, Export; the website
has no Search), the corner bar at the bottom right (Tasks, Settings), and the scenery. The item that is on grows
into a circle with its name under it; the others are bare icons. On a narrow screen the corner bar sits above
the dock. The Sources moved into Settings.

## Motion

Everything that moves is in the style sheets, and stops with `prefers-reduced-motion` and the "Snappy"
animation setting: the dock slides in and its items spring into the circle; the letters of the title come up
one after another; the clouds drift in pixel steps; cards come up one after another and lift when pointed at;
a screen comes up and fades in.
