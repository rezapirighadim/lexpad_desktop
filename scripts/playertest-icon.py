"""The player test build's app icon: Lexpad's icon with an amber TEST band,
so it never looks like the installed Lexpad in the Dock, Finder or Open With.

  python3 scripts/playertest-icon.py && pnpm tauri icon icons-src/app-1024-playertest.png -o src-tauri/icons-playertest

(then keep only the files tauri.playertest.conf.json lists). Needs Pillow.
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
src = Image.open(ROOT / "icons-src/app-1024.png").convert("RGBA")
w, h = src.size

# The band sits across the lower part of the rounded square (100..924 px of
# the 1024 canvas), clipped to the square's own shape.
band = Image.new("RGBA", src.size, (0, 0, 0, 0))
d = ImageDraw.Draw(band)
d.rectangle([0, 760, w, 900], fill=(245, 158, 11, 255))
font = None
for name in ("/System/Library/Fonts/Supplemental/Arial Bold.ttf", "/System/Library/Fonts/Helvetica.ttc"):
    try:
        font = ImageFont.truetype(name, 112)
        break
    except OSError:
        continue
text = "TEST"
box = d.textbbox((0, 0), text, font=font)
tw, th = box[2] - box[0], box[3] - box[1]
d.text(((w - tw) / 2 - box[0], 830 - th / 2 - box[1]), text, font=font, fill=(255, 255, 255, 255))

mask = src.getchannel("A")
clipped = Image.new("RGBA", src.size, (0, 0, 0, 0))
clipped.paste(band, (0, 0), Image.composite(band.getchannel("A"), Image.new("L", src.size, 0), mask))
out = Image.alpha_composite(src, clipped)
out.save(ROOT / "icons-src/app-1024-playertest.png")
print("icons-src/app-1024-playertest.png")
