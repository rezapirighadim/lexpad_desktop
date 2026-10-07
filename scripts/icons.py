"""Draws the app icon and the menu-bar icon from the Lexpad mark.

The mark is the favicon's notebook (landing/site/assets/favicon-v2.svg), drawn
again at full size rather than scaled up, so every icon is sharp:

  icons-src/app-1024.png   the app icon; `pnpm tauri icon` makes every size from it
  src-tauri/icons/tray.png the menu-bar / tray icon, a template image (black on
                           transparent; macOS tints it for light and dark bars)

Run: python3 scripts/icons.py && pnpm tauri icon icons-src/app-1024.png
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
GREEN = (46, 123, 78, 255)  # --accent, #2E7B4E
WHITE = (255, 255, 255, 255)


def notebook(draw: ImageDraw.ImageDraw, scale: float, ox: float, oy: float, colour, width: float) -> None:
    """The favicon's 32-unit notebook, scaled and offset."""
    def p(x: float, y: float) -> tuple[float, float]:
        return (ox + x * scale, oy + y * scale)

    w = max(1, round(width * scale))
    draw.rounded_rectangle([p(10, 8), p(22, 24)], radius=2 * scale, outline=colour, width=w)
    draw.line([p(14, 8), p(14, 24)], fill=colour, width=w)
    for y in (12, 15):
        draw.line([p(17, y), p(19, y)], fill=colour, width=w)
        r = w / 2
        for x in (17, 19):
            cx, cy = p(x, y)
            draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=colour)


def app_icon() -> None:
    size = 1024
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # The macOS grid: an 824-point rounded square centred on a 1024 canvas.
    inset = 100
    d.rounded_rectangle([inset, inset, size - inset, size - inset], radius=185, fill=GREEN)
    scale = (size - 2 * inset) / 32
    notebook(d, scale, inset, inset, WHITE, 2)
    out = ROOT / 'icons-src'
    out.mkdir(exist_ok=True)
    img.save(out / 'app-1024.png')


def tray_icon() -> None:
    # 44 px is the 22 pt menu-bar height at 2x; Windows scales it down.
    size = 44
    big = Image.new('RGBA', (size * 8, size * 8), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    scale = size * 8 / 24
    notebook(d, scale, -4 * scale, -4 * scale, (0, 0, 0, 255), 2.4)
    big.resize((size, size), Image.LANCZOS).save(ROOT / 'src-tauri' / 'icons' / 'tray.png')


if __name__ == '__main__':
    app_icon()
    tray_icon()
