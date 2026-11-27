"""Draws the app icon and the menu-bar / tray icons from the Lexpad mark (v3).

The mark is read from icons-src/lexpad-icon-v3.svg (or the path in LEXPAD_MARK),
the owner-approved icon of 22 Nov 2026 (uiux/2026-11-22-brand/logo-b/final in the
workspace; the iPhone and Android apps carry the same one): a headphone arch over
a notebook with a spine and a play triangle, white on green, on a 512-unit grid.
Each icon is drawn again at its own size from that geometry, eight times over
and scaled down, rather than one big image shrunk, so every size is sharp; the
smallest sizes get slightly heavier strokes (STROKE_BOOST) so the lines stay lines.

  icons-src/app-1024.png              the app icon on the macOS grid (an 824-point rounded
                                      square on a 1024 canvas); `tauri icon` makes the PNGs
                                      and the Windows files from it
  src-tauri/icons/icon.icns           macOS (Dock, Finder, the App Store): every size drawn
                                      on its own, with the grid's soft shadow, packed by
                                      iconutil (`--icns`, after `tauri icon`)
  src-tauri/icons/tray-template.png   macOS menu bar, 1x (16 x 18 px for a 16 x 18 pt slot)
  src-tauri/icons/tray-template@2x.png  the same at 2x (32 x 36 px)
                                      Template images: black on transparent, so macOS
                                      tints them for light and dark menu bars.
  src-tauri/icons/tray.ico            Windows tray: the coloured mark at 16, 20, 24, 32
                                      and 48 px (100% to 300% scaling); the app picks the
                                      frame for the system's small-icon size.

 src-tauri/icons/icon.ico            the exe's icon, written here after `tauri icon` (which
                                      writes every layer as PNG with zero planes): 32-bit
                                      BMP layers at 16 to 96 px and a PNG at 256 px, planes
                                      1, so Explorer, the desktop and Start draw it at every
                                      size and scale (`--app-ico` alone rewrites only this).

  msix/Assets/                        the Microsoft Store package's logos (Start, tiles, the
                                      taskbar and the Store), at every scale Windows asks for;
                                      scripts/msix.ps1 indexes them with makepri.

Run: pnpm icons   (python3 scripts/icons.py && tauri icon icons-src/app-1024.png
                   && python3 scripts/icons.py --app-ico --icns)
     python3 scripts/icons.py --preview   also writes docs/screenshots/tray-icons.png:
     the template on a light and a dark menu bar at 1x and 2x, and every tray.ico frame,
     and docs/screenshots/app-icon-sizes.png: the macOS icon at 16, 32, 128, 512 and 1024.
"""
import io
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from xml.etree import ElementTree

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
MARK = Path(os.environ.get('LEXPAD_MARK', ROOT / 'icons-src' / 'lexpad-icon-v3.svg'))
SS = 8  # supersampling factor

# The menu-bar slot. tray-icon draws the status item's image 18 pt tall; the
# glyph sits inside it with the same air system icons have (about 1.5 pt
# above and below), and the slot is only as wide as the glyph needs.
TEMPLATE_W, TEMPLATE_H = 16, 18
GLYPH_H = 15.0  # points

# Strokes a little heavier where the mark is tiny: at 15 pt the icon's own
# strokes are 1.2 pt, lighter than the system's menu-bar glyphs; the Android
# notification icon makes the same 1.25x choice at 24 dp.
STROKE_BOOST = 1.25

# Windows small-icon sizes: 100%, 125%, 150%, 200% and 300% scaling.
ICO_SIZES = (16, 20, 24, 32, 48)
TRAY_GLYPH = 0.78  # the mark's height as a share of the tray square

# The macOS app icon grid (Apple's template): an 824-point rounded square,
# corner 185, centred on 1024, with a soft shadow below it.
MAC_CANVAS, MAC_SQUARE, MAC_RADIUS = 1024, 824, 185
MAC_SHADOW = {'dy': 12, 'blur': 14, 'alpha': 0.30}  # at 1024; scaled with the size
# The sizes an .icns holds (iconutil's names), each drawn on its own.
ICNS = (16, 32, 64, 128, 256, 512, 1024)
ICNS_NAMES = {
    16: ['icon_16x16.png'],
    32: ['icon_16x16@2x.png', 'icon_32x32.png'],
    64: ['icon_32x32@2x.png'],
    128: ['icon_128x128.png'],
    256: ['icon_128x128@2x.png', 'icon_256x256.png'],
    512: ['icon_256x256@2x.png', 'icon_512x512.png'],
    1024: ['icon_512x512@2x.png'],
}


def _nums(text: str) -> list[float]:
    return [float(n) for n in re.findall(r'-?\d*\.?\d+', text)]


def load_mark(path: Path) -> dict:
    """The icon's geometry on its grid: ground colour, arch, notebook, spine, play triangle."""
    ns = {'s': 'http://www.w3.org/2000/svg'}
    svg = ElementTree.parse(path).getroot()
    size = _nums(svg.get('viewBox'))[2]
    ground = next(r for r in svg.iter('{%s}rect' % ns['s']) if r.get('fill') not in (None, 'none'))
    group = svg.find('s:g', ns)
    arch, spine, play = None, None, None
    for el in group.findall('s:path', ns):
        d = el.get('d')
        if 'A' in d:
            arch = el
        elif el.get('fill') not in (None, 'none'):
            play = el
        else:
            spine = el
    book = group.find('s:rect', ns)
    # Arch: "M x0 yLeg V yc A rx ry 0 0 1 x1 yc V yLeg"
    a = _nums(arch.get('d'))
    x0, leg, yc, rx, ry, x1 = a[0], a[1], a[2], a[3], a[4], a[8]
    s = _nums(spine.get('d'))
    t = _nums(play.get('d'))
    hexc = ground.get('fill').lstrip('#')
    return {
        'size': size,
        'background': tuple(int(hexc[i : i + 2], 16) for i in (0, 2, 4)) + (255,),
        'arch': {'cx': (x0 + x1) / 2, 'cy': yc, 'rx': rx, 'ry': ry, 'legs': (x0, x1), 'leg_end': leg,
                 'w': float(arch.get('stroke-width'))},
        'book': {'rect': tuple(float(book.get(k)) for k in ('x', 'y', 'width', 'height')),
                 'r': float(book.get('rx')), 'w': float(book.get('stroke-width'))},
        'spine': {'x': s[0], 'y0': s[1], 'y1': s[2], 'w': float(spine.get('stroke-width'))},
        'play': {'points': [(t[i], t[i + 1]) for i in range(0, 6, 2)], 'w': float(play.get('stroke-width'))},
    }


def mark_mask(mark: dict, size: tuple[int, int], scale: float, ox: float, oy: float, boost: float = 1.0,
              spine: bool = True) -> Image.Image:
    """The mark as an alpha mask of `size` (already supersampled): grid units times `scale` plus (ox, oy).

    `boost` multiplies every stroke width around the same centre lines; without
    `spine` the notebook has no spine (16 px, where it only fills the notebook in).
    """
    def P(x: float, y: float) -> tuple[float, float]:
        return (ox + x * scale, oy + y * scale)

    def layer() -> tuple[Image.Image, ImageDraw.ImageDraw]:
        m = Image.new('L', size, 0)
        return m, ImageDraw.Draw(m)

    parts = []
    # The headphone arch: the upper half of a stroked ellipse, then two legs with round ends.
    a = mark['arch']
    h = a['w'] * boost / 2
    m, d = layer()
    (cx, cy), k = P(a['cx'], a['cy']), scale
    d.ellipse([cx - (a['rx'] + h) * k, cy - (a['ry'] + h) * k, cx + (a['rx'] + h) * k, cy + (a['ry'] + h) * k], fill=255)
    d.ellipse([cx - (a['rx'] - h) * k, cy - (a['ry'] - h) * k, cx + (a['rx'] - h) * k, cy + (a['ry'] - h) * k], fill=0)
    d.rectangle([0, cy, size[0], size[1]], fill=0)
    for lx in a['legs']:
        (px, top), (_, end) = P(lx, a['cy']), P(lx, a['leg_end'])
        d.rectangle([px - h * k, top - 1, px + h * k, end], fill=255)
        d.ellipse([px - h * k, end - h * k, px + h * k, end + h * k], fill=255)
    parts.append(m)
    # The notebook: a stroked rounded rectangle.
    b = mark['book']
    x, y, w, hh = b['rect']
    bw = b['w'] * boost / 2
    m, d = layer()
    (x0, y0), (x1, y1) = P(x, y), P(x + w, y + hh)
    d.rounded_rectangle([x0 - bw * k, y0 - bw * k, x1 + bw * k, y1 + bw * k], radius=(b['r'] + bw) * k, fill=255)
    d.rounded_rectangle([x0 + bw * k, y0 + bw * k, x1 - bw * k, y1 - bw * k], radius=max(0.0, b['r'] - bw) * k, fill=0)
    parts.append(m)
    # The spine: a butt-ended bar from the notebook's top edge to its bottom edge.
    sp = mark['spine']
    sw = sp['w'] * boost / 2
    if spine:
        m, d = layer()
        (sx, sy0), (_, sy1) = P(sp['x'], sp['y0']), P(sp['x'], sp['y1'])
        d.rectangle([sx - sw * k, sy0, sx + sw * k, sy1], fill=255)
        parts.append(m)
    # The play triangle: filled, with a round-joined stroke round it.
    pl = mark['play']
    pw = pl['w'] * boost / 2
    m, d = layer()
    pts = [P(*q) for q in pl['points']]
    d.polygon(pts, fill=255)
    for i, (qx, qy) in enumerate(pts):
        nx, ny = pts[(i + 1) % 3]
        d.line([(qx, qy), (nx, ny)], fill=255, width=max(1, round(2 * pw * k)))
        d.ellipse([qx - pw * k, qy - pw * k, qx + pw * k, qy + pw * k], fill=255)
    parts.append(m)
    out = parts[0]
    for p in parts[1:]:
        out = ImageChops.lighter(out, p)
    return out


def mark_bbox(mark: dict, boost: float, spine: bool = True) -> tuple[float, float, float, float]:
    """The inked box of the mark in grid units (strokes included), measured from a render."""
    n = 2048
    scale = n / mark['size']
    box = mark_mask(mark, (n, n), scale, 0, 0, boost, spine).getbbox()
    return tuple(v / scale for v in box)


def tile(mark: dict, px: int, radius_ratio: float, inset: float = 0.0, mark_share: float | None = None,
         boost: float = 1.0, shadow: dict | None = None, spine: bool = True) -> Image.Image:
    """The icon at `px` pixels: the green rounded square with the white mark.

    Without `mark_share` the mark keeps the icon's own proportions (its grid
    fills the square); with it, the mark's inked height is that share of the square.
    """
    big = px * SS
    img = Image.new('RGBA', (big, big), (0, 0, 0, 0))
    pad = inset * big
    side = big - 2 * pad
    square = Image.new('L', (big, big), 0)
    ImageDraw.Draw(square).rounded_rectangle([pad, pad, big - pad, big - pad], radius=side * radius_ratio, fill=255)
    if shadow:
        sh = square.filter(ImageFilter.GaussianBlur(shadow['blur'] * big / MAC_CANVAS))
        sh = ImageChops.offset(sh, 0, round(shadow['dy'] * big / MAC_CANVAS))
        sh = sh.point(lambda v: round(v * shadow['alpha']))
        img.paste(Image.new('RGBA', img.size, (0, 0, 0, 255)), (0, 0), sh)
    img.paste(Image.new('RGBA', img.size, mark['background']), (0, 0), square)
    if mark_share is None:
        scale = side / mark['size']
        ox = oy = pad
    else:
        bx0, by0, bx1, by1 = mark_bbox(mark, boost, spine)
        scale = mark_share * side / (by1 - by0)
        ox = (big - (bx1 - bx0) * scale) / 2 - bx0 * scale
        oy = (big - (by1 - by0) * scale) / 2 - by0 * scale
    mask = mark_mask(mark, img.size, scale, ox, oy, boost, spine)
    img.paste(Image.new('RGBA', img.size, (255, 255, 255, 255)), (0, 0), mask)
    return img.resize((px, px), Image.BOX)


# The two smallest sizes (Finder's list, the menu's icons) are hinted by hand:
# the mark fills more of the square with heavier strokes, and at 16 px the spine
# goes (it only filled the notebook in). Chosen side by side, 27 Nov 2026.
MAC_SMALL = {16: {'mark_share': 0.82, 'boost': 1.4, 'spine': False}, 32: {'mark_share': 0.72, 'boost': 1.4}}


def mac_icon(mark: dict, px: int, shadow: bool = True) -> Image.Image:
    """The macOS icon at `px`: Apple's grid; the mark's own proportions from 64 px up."""
    return tile(mark, px, MAC_RADIUS / MAC_SQUARE, inset=(MAC_CANVAS - MAC_SQUARE) / 2 / MAC_CANVAS,
                shadow=MAC_SHADOW if shadow else None, **MAC_SMALL.get(px, {}))


def app_icon(mark: dict) -> None:
    # The source for `tauri icon` (PNGs, Windows): the grid without the shadow,
    # which Windows does not draw under its icons.
    out = ROOT / 'icons-src'
    out.mkdir(exist_ok=True)
    mac_icon(mark, 1024, shadow=False).save(out / 'app-1024.png')


def icns(mark: dict) -> None:
    """src-tauri/icons/icon.icns: each size drawn on its own, packed by iconutil."""
    tmp = Path(tempfile.mkdtemp()) / 'icon.iconset'
    tmp.mkdir()
    try:
        for px in ICNS:
            img = mac_icon(mark, px)
            for name in ICNS_NAMES[px]:
                img.save(tmp / name)
        subprocess.run(['iconutil', '-c', 'icns', str(tmp), '-o', str(ROOT / 'src-tauri' / 'icons' / 'icon.icns')], check=True)
    finally:
        shutil.rmtree(tmp.parent)


def template_icon(mark: dict, factor: int) -> Image.Image:
    """The mark alone, black on transparent, GLYPH_H points tall, centred in the slot."""
    w, h = TEMPLATE_W * factor * SS, TEMPLATE_H * factor * SS
    bx0, by0, bx1, by1 = mark_bbox(mark, STROKE_BOOST)
    scale = GLYPH_H * factor * SS / (by1 - by0)
    ox = (w - (bx1 - bx0) * scale) / 2 - bx0 * scale
    oy = (h - (by1 - by0) * scale) / 2 - by0 * scale
    mask = mark_mask(mark, (w, h), scale, ox, oy, STROKE_BOOST)
    img = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    img.paste(Image.new('RGBA', img.size, (0, 0, 0, 255)), (0, 0), mask)
    return img.resize((TEMPLATE_W * factor, TEMPLATE_H * factor), Image.BOX)


def coloured_mark(mark: dict, px: int, tray: bool = False) -> Image.Image:
    """The coloured square edge to edge with soft corners; for a tray slot the mark fills more of it."""
    if tray:
        return tile(mark, px, 0.22, mark_share=TRAY_GLYPH, boost=STROKE_BOOST)
    return tile(mark, px, 0.22)


def tray_icons(mark: dict) -> None:
    out = ROOT / 'src-tauri' / 'icons'
    template_icon(mark, 1).save(out / 'tray-template.png')
    template_icon(mark, 2).save(out / 'tray-template@2x.png')
    frames = [coloured_mark(mark, s, tray=True) for s in ICO_SIZES]
    frames[-1].save(out / 'tray.ico', format='ICO', sizes=[(s, s) for s in ICO_SIZES], append_images=frames[:-1])


# The exe's icon (src-tauri/icons/icon.ico): the shell's sizes at 100% to
# 200% scaling (small icons, Start's list, the taskbar, the desktop's
# medium icons, Explorer's large ones) as uncompressed 32-bit BMP layers,
# which every Windows icon path reads, and 256 px as PNG, as Windows expects.
APP_ICO_BMP = (16, 20, 24, 32, 40, 48, 64, 72, 96)
APP_ICO_PNG = (256,)


def ico_bmp(img: Image.Image) -> bytes:
    """One icon layer as a DIB: BITMAPINFOHEADER (height doubled), BGRA rows bottom up, then the AND mask."""
    w, h = img.size
    rgba = img.convert('RGBA')
    header = struct.pack('<IiiHHIIiiII', 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    px = rgba.load()
    xor = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            xor += bytes((b, g, r, a))
    stride = ((w + 31) // 32) * 4
    mask = bytearray()
    for y in range(h - 1, -1, -1):
        row = bytearray(stride)
        for x in range(w):
            if px[x, y][3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        mask += row
    return header + bytes(xor) + bytes(mask)


def write_ico(path: Path, layers: list[tuple[int, bytes]]) -> None:
    """An ICO from (size, layer bytes): planes 1 and 32 bits in every directory entry."""
    out = struct.pack('<HHH', 0, 1, len(layers))
    offset = 6 + 16 * len(layers)
    body = b''
    for size, data in layers:
        side = 0 if size >= 256 else size
        out += struct.pack('<BBBBHHII', side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        body += data
    path.write_bytes(out + body)


def app_ico() -> None:
    """src-tauri/icons/icon.ico from icons-src/app-1024.png, the picture `tauri icon` used."""
    src = Image.open(ROOT / 'icons-src' / 'app-1024.png').convert('RGBA')
    layers = []
    for size in APP_ICO_BMP:
        layers.append((size, ico_bmp(src.resize((size, size), Image.LANCZOS))))
    for size in APP_ICO_PNG:
        buf = io.BytesIO()
        src.resize((size, size), Image.LANCZOS).save(buf, format='PNG', optimize=True)
        layers.append((size, buf.getvalue()))
    write_ico(ROOT / 'src-tauri' / 'icons' / 'icon.ico', layers)


# The MSIX logos: name -> (width, height) at scale-100, and the mark's share
# of the shorter side. Start's list and the Store show the mark edge to edge;
# tiles keep air around it, as Windows' own tiles do.
MSIX_LOGOS = {
    'Square44x44Logo': ((44, 44), 1.0),
    'Square150x150Logo': ((150, 150), 0.6),
    'Wide310x150Logo': ((310, 150), 0.6),
    'SmallTile': ((71, 71), 0.6),
    'LargeTile': ((310, 310), 0.6),
    'StoreLogo': ((50, 50), 1.0),
}
MSIX_SCALES = (100, 125, 150, 200, 400)
# Square44x44Logo's target sizes: Start's list, the taskbar, Explorer and
# Alt+Tab pick from these, unplated (no tile colour behind them).
MSIX_TARGET_SIZES = (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256)


def msix_logo(mark: dict, w: int, h: int, share: float) -> Image.Image:
    side = max(1, round(min(w, h) * share))
    glyph = coloured_mark(mark, side, tray=side <= ICO_SIZES[-1])
    img = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    img.paste(glyph, ((w - side) // 2, (h - side) // 2), glyph)
    return img


def msix_assets(mark: dict) -> None:
    out = ROOT / 'msix' / 'Assets'
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob('*.png'):
        old.unlink()
    for name, ((w, h), share) in MSIX_LOGOS.items():
        for scale in MSIX_SCALES:
            sw, sh = round(w * scale / 100), round(h * scale / 100)
            msix_logo(mark, sw, sh, share).save(out / f'{name}.scale-{scale}.png')
    for size in MSIX_TARGET_SIZES:
        img = msix_logo(mark, size, size, 1.0)
        img.save(out / f'Square44x44Logo.targetsize-{size}.png')
        img.save(out / f'Square44x44Logo.targetsize-{size}_altform-unplated.png')
        img.save(out / f'Square44x44Logo.targetsize-{size}_altform-lightunplated.png')


def preview() -> None:
    """The menu-bar icon as macOS would tint it, and the Windows frames, enlarged 4x without smoothing."""
    icons = ROOT / 'src-tauri' / 'icons'
    zoom = 4
    bars = [((236, 236, 236, 255), (0, 0, 0, 216)), ((40, 40, 40, 255), (255, 255, 255, 255))]
    tiles = []
    for bg, ink in bars:
        for name, factor in (('tray-template.png', 1), ('tray-template@2x.png', 2)):
            glyph = Image.open(icons / name).convert('RGBA')
            pad = 4 * factor
            tile = Image.new('RGBA', (glyph.width + 2 * pad, 24 * factor), bg)
            tinted = Image.new('RGBA', glyph.size, ink)
            tile.paste(tinted, (pad, (tile.height - glyph.height) // 2), glyph.split()[3])
            # Same on-screen size for both: a 1x pixel is two 2x pixels.
            tiles.append(tile.resize((tile.width * zoom * 2 // factor, tile.height * zoom * 2 // factor), Image.NEAREST))
    ico = Image.open(icons / 'tray.ico')
    for size in ICO_SIZES:
        ico.size = (size, size)
        ico.load()
        frame = ico.convert('RGBA')
        tile = Image.new('RGBA', (size + 8, size + 8), (243, 243, 243, 255))
        tile.paste(frame, (4, 4), frame)
        tiles.append(tile.resize((tile.width * zoom, tile.height * zoom), Image.NEAREST))
    gap = 16
    width = sum(t.width for t in tiles) + gap * (len(tiles) + 1)
    height = max(t.height for t in tiles) + 2 * gap
    sheet = Image.new('RGBA', (width, height), (255, 255, 255, 0))
    x = gap
    for t in tiles:
        sheet.paste(t, (x, (height - t.height) // 2))
        x += t.width + gap
    out = ROOT / 'docs' / 'screenshots'
    out.mkdir(parents=True, exist_ok=True)
    sheet.save(out / 'tray-icons.png')


def app_icon_sizes(mark: dict) -> None:
    """The macOS icon at 16, 32, 128, 512 and 1024 px on light and dark, each at its real pixels."""
    sizes = (16, 32, 128, 512, 1024)
    gap = 24
    width = sum(sizes) + gap * (len(sizes) + 1)
    height = max(sizes) + 2 * gap
    sheet = Image.new('RGBA', (width, height * 2), (255, 255, 255, 255))
    ImageDraw.Draw(sheet).rectangle([0, height, width, 2 * height], fill=(30, 30, 30, 255))
    for row in range(2):
        x = gap
        for px in sizes:
            img = mac_icon(mark, px)
            sheet.alpha_composite(img, (x, row * height + (height - px) // 2))
            x += px + gap
    out = ROOT / 'docs' / 'screenshots'
    out.mkdir(parents=True, exist_ok=True)
    sheet.save(out / 'app-icon-sizes.png')


if __name__ == '__main__':
    if '--app-ico' in sys.argv or '--icns' in sys.argv:
        if '--app-ico' in sys.argv:
            app_ico()
        if '--icns' in sys.argv:
            icns(load_mark(MARK))
        sys.exit(0)
    geometry = load_mark(MARK)
    app_icon(geometry)
    tray_icons(geometry)
    msix_assets(geometry)
    if '--preview' in sys.argv:
        preview()
        app_icon_sizes(geometry)
