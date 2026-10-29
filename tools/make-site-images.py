"""Makes the home page's screenshots, apps/site/img/*.webp, from the ones in docs/screenshots (JPG, used by the
README). WebP is about half the size at the same look, and every current browser shows it.

    python tools/make-site-images.py
"""
import pathlib

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "apps" / "site" / "img"
OUT.mkdir(parents=True, exist_ok=True)
for name in ("series", "read", "export", "web-phone"):
    source = ROOT / "docs" / "screenshots" / f"{name}.jpg"
    target = OUT / f"{name}.webp"
    image = Image.open(source).convert("RGB")
    image.save(target, "WEBP", quality=80, method=6)
    if image.width == 1440:  # phones get half the width (srcset in index.html)
        image.resize((720, image.height // 2), Image.LANCZOS).save(OUT / f"{name}-720.webp", "WEBP", quality=80, method=6)
    print(f"{target.name}: {source.stat().st_size // 1024} KB -> {target.stat().st_size // 1024} KB")
