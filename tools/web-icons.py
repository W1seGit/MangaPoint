"""Make the website's icons from the desktop app's icon (one source of truth).

    python tools/web-icons.py

Writes web/icons/: icon-192.png, icon-512.png, icon-maskable-512.png (the mascot
inside the safe zone on the app's black), apple-touch-icon.png (180) and
favicon-32.png. The icon is pixel art, so it is scaled with nearest-neighbour
steps to stay crisp.
"""
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "apps" / "desktop" / "assets" / "icon.png"
OUT = ROOT / "web" / "icons"
BACKGROUND = (0, 0, 0, 255)  # --bg of style.css


def scaled(image: Image.Image, size: int) -> Image.Image:
    # Whole-number steps keep pixel art sharp; the last step (if any) is smooth.
    whole = max(1, round(size / image.width))
    step = image.resize((image.width * whole, image.height * whole), Image.NEAREST)
    if step.width == size:
        return step
    return step.resize((size, size), Image.LANCZOS)


def on_background(icon: Image.Image, size: int, inner: float) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), BACKGROUND)
    inner_px = round(size * inner)
    glyph = scaled(icon, inner_px)
    canvas.alpha_composite(glyph, ((size - inner_px) // 2, (size - inner_px) // 2))
    return canvas


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    icon = Image.open(SOURCE).convert("RGBA")
    scaled(icon, 192).save(OUT / "icon-192.png")
    scaled(icon, 512).save(OUT / "icon-512.png")
    # Maskable icons may be cropped to a circle: keep the art in the middle 70%.
    on_background(icon, 512, 0.70).save(OUT / "icon-maskable-512.png")
    on_background(icon, 180, 0.88).save(OUT / "apple-touch-icon.png")
    scaled(icon, 32).save(OUT / "favicon-32.png")
    for p in sorted(OUT.glob("*.png")):
        print(p.relative_to(ROOT), Image.open(p).size)


if __name__ == "__main__":
    main()
