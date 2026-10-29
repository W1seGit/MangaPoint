"""Makes the fonts the two websites download: crates/ui/assets/fonts/web/fonts.css and its fonts/*.woff2.

The desktop app keeps its fonts inside crates/ui/css/fonts.css (tools/make-fonts.py), so it needs no files.
A website is better off with real files: a browser fetches them in parallel with the page, keeps them in its
cache, and only downloads the scripts a page uses (the `unicode-range` of each face). Both fonts are
variable fonts from github.com/google/fonts (ofl/inter, ofl/pixelifysans), both SIL OFL (see ../).

    python tools/make-web-fonts.py <Inter.ttf> <PixelifySans.ttf>

Weights 400 to 700 are kept (the style sheets use nothing outside them), and Inter keeps its optical-size
axis, so large headings look as they always did.
"""
import io
import pathlib
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

OUT = pathlib.Path(__file__).resolve().parent.parent / "crates" / "ui" / "assets" / "fonts" / "web"
FILES = OUT / "fonts"  # fonts.css names them as fonts/<name>, so a build copies both next to each other

# One face per script, so a page that only has English text never downloads the others.
SCRIPTS = {
    "latin": "U+0020-007E,U+00A0-00FF,U+0131,U+0152-0153,U+02C6,U+02DA,U+02DC,U+2010-2027,U+2030-203A,U+2044,U+20AC,U+2190-2193,U+2212,U+2215,U+2022,U+00D7,U+2122",
    "latin-ext": "U+0100-0130,U+0132-0151,U+0154-024F,U+1E00-1EFF",
    "cyrillic": "U+0400-04FF",
}
FAMILIES = [("Inter", "inter"), ("Pixelify Sans", "pixelify-sans")]
WEIGHTS = (400, 700)


def woff2(path, unicodes):
    font = TTFont(path)
    opts = subset.Options()
    opts.flavor = "woff2"
    opts.layout_features = ["*"]
    opts.notdef_outline = True
    opts.name_IDs = ["*"]
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=subset.parse_unicodes(unicodes))
    sub.subset(font)
    font = instancer.instantiateVariableFont(font, {"wght": WEIGHTS})
    out = io.BytesIO()
    font.flavor = "woff2"
    font.save(out)
    return out.getvalue()


def main():
    sources = {"Inter": sys.argv[1], "Pixelify Sans": sys.argv[2]}
    FILES.mkdir(parents=True, exist_ok=True)
    css = "/* Inter and Pixelify Sans, SIL Open Font License 1.1 (see ../). Made by tools/make-web-fonts.py. */\n"
    total = 0
    for family, slug in FAMILIES:
        for script, unicodes in SCRIPTS.items():
            data = woff2(sources[family], unicodes)
            name = f"{slug}-{script}.woff2"
            (FILES / name).write_bytes(data)
            total += len(data)
            print(f"{name}: {len(data) / 1024:.1f} KB")
            css += (
                f'@font-face {{ font-family: "{family}"; font-style: normal; font-weight: {WEIGHTS[0]} {WEIGHTS[1]}; '
                f"font-display: swap;\n  src: url(fonts/{name}) format(\"woff2\");\n  unicode-range: {unicodes}; }}\n"
            )
    (OUT / "fonts.css").write_text(css, encoding="utf-8", newline="\n")
    print(f"{total / 1024:.0f} KB in all")


main()
