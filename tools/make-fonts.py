"""Makes crates/ui/css/fonts.css: Inter and Pixelify Sans (both SIL OFL) as small embedded web fonts.

The two variable fonts come from github.com/google/fonts (ofl/inter, ofl/pixelifysans). Run:
    python tools/make-fonts.py <Inter.ttf> <PixelifySans.ttf>
The licenses are kept in crates/ui/assets/fonts. The subset is Latin, Latin Extended, Cyrillic and
the usual punctuation; anything else falls back to the system font.
"""
import base64, io, sys
from fontTools import subset
from fontTools.ttLib import TTFont

UNICODES = "U+0020-007E,U+00A0-024F,U+0400-04FF,U+1E00-1EFF,U+2010-2027,U+2030-203A,U+2044,U+20AC,U+2190-2193,U+2212,U+2215,U+2022,U+00D7"

def woff2(path):
    opts = subset.Options()
    opts.flavor = "woff2"
    opts.layout_features = ["*"]
    opts.notdef_outline = True
    opts.name_IDs = ["*"]
    font = TTFont(path)
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=subset.parse_unicodes(UNICODES))
    sub.subset(font)
    out = io.BytesIO()
    font.flavor = "woff2"
    font.save(out)
    return out.getvalue()

def face(family, data, weights):
    b64 = base64.b64encode(data).decode()
    return (f'@font-face {{ font-family: "{family}"; font-style: normal; font-weight: {weights}; font-display: swap;\n'
            f'  src: url(data:font/woff2;base64,{b64}) format("woff2"); }}\n')

inter, pixel, out = sys.argv[1], sys.argv[2], sys.argv[3]
css = "/* Inter and Pixelify Sans, SIL Open Font License 1.1 (see assets/fonts). Made by tools/make-fonts.py. */\n"
css += face("Inter", woff2(inter), "100 900")
css += face("Pixelify Sans", woff2(pixel), "400 700")
open(out, "w", newline="\n").write(css)
print(len(css) // 1024, "KB")
