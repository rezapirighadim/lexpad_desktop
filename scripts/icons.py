"""Draws the app icon and the menu-bar / tray icons from the Lexpad mark.

The mark is read from the landing site's favicon (landing/site/assets/favicon-v2.svg,
or the path in LEXPAD_MARK): its notebook rectangle and the spine and lines in
its one path. Each icon is drawn again at its own size from that geometry,
eight times over and scaled down, rather than one big image shrunk, so every
size is sharp.

  icons-src/app-1024.png              the app icon; `pnpm tauri icon` makes every size from it
  src-tauri/icons/tray-template.png   macOS menu bar, 1x (16 x 18 px for a 16 x 18 pt slot)
  src-tauri/icons/tray-template@2x.png  the same at 2x (32 x 36 px)
                                      Template images: black on transparent, so macOS
                                      tints them for light and dark menu bars.
  src-tauri/icons/tray.ico            Windows tray: the coloured mark at 16, 20, 24, 32
                                      and 48 px (100% to 300% scaling); the app picks the
                                      frame for the system's small-icon size.

  msix/Assets/                        the Microsoft Store package's logos (Start, tiles, the
                                      taskbar and the Store), at every scale Windows asks for;
                                      scripts/msix.ps1 indexes them with makepri.

Run: pnpm icons   (python3 scripts/icons.py && tauri icon icons-src/app-1024.png)
     python3 scripts/icons.py --preview   also writes docs/screenshots/tray-icons.png:
     the template on a light and a dark menu bar at 1x and 2x, and every tray.ico frame.
"""
import sys
import os
import re
from pathlib import Path
from xml.etree import ElementTree

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
MARK = Path(os.environ.get('LEXPAD_MARK', ROOT.parent / 'landing' / 'site' / 'assets' / 'favicon-v2.svg'))
SS = 8  # supersampling factor

# The menu-bar slot. tray-icon draws the status item's image 18 pt tall; the
# glyph sits inside it with the same air system icons have (about 1.5 pt
# above and below), and the slot is only as wide as the glyph needs.
TEMPLATE_W, TEMPLATE_H = 16, 18
GLYPH_H = 15.0  # points
TEMPLATE_STROKE = 1.6  # points; the favicon's 2-unit stroke is too heavy at 15 pt
# At 1x a 1.6-pixel stroke is a blur; one whole pixel on pixel centres reads as a line.
TEMPLATE_STROKE_1X = 1.0

# Windows small-icon sizes: 100%, 125%, 150%, 200% and 300% scaling.
ICO_SIZES = (16, 20, 24, 32, 48)
TRAY_GLYPH = 0.75  # the notebook's height as a share of the tray square
TRAY_STROKE_PX = 16  # one pixel of stroke per 16 pixels of icon


def load_mark(path: Path) -> dict:
    """The favicon's geometry: background colour, the notebook rect, the strokes, the stroke width."""
    ns = {'s': 'http://www.w3.org/2000/svg'}
    svg = ElementTree.parse(path).getroot()
    size = float(svg.get('viewBox').split()[2])
    rects = svg.findall('s:rect', ns)
    background = next(r for r in rects if r.get('fill') not in (None, 'none'))
    book = next(r for r in rects if r.get('fill') == 'none')
    path_el = svg.find('s:path', ns)
    lines = []
    x = y = 0.0
    start = None
    for cmd, args in re.findall(r'([MmHhVvLl])([^MmHhVvLl]*)', path_el.get('d')):
        nums = [float(n) for n in re.findall(r'-?\d*\.?\d+', args)]
        if cmd == 'M':
            x, y = nums[0], nums[1]
            start = (x, y)
            continue
        if cmd == 'h':
            x += nums[0]
        elif cmd == 'H':
            x = nums[0]
        elif cmd == 'v':
            y += nums[0]
        elif cmd == 'V':
            y = nums[0]
        elif cmd == 'L':
            x, y = nums[0], nums[1]
        elif cmd == 'l':
            x, y = x + nums[0], y + nums[1]
        lines.append((start, (x, y)))
        start = (x, y)
    hex_colour = background.get('fill').lstrip('#')
    return {
        'size': size,
        'background': tuple(int(hex_colour[i : i + 2], 16) for i in (0, 2, 4)) + (255,),
        'rect': tuple(float(book.get(k)) for k in ('x', 'y', 'width', 'height')),
        'radius': float(book.get('rx', 0)),
        'lines': lines,
        'stroke': float(book.get('stroke-width', 2)),
    }


def notebook_mask(size: tuple[int, int], mark: dict, scale: float, ox: float, oy: float, stroke: float, snap: bool) -> Image.Image:
    """The notebook (rect, spine, lines) as an alpha mask at SS times the output size.

    Mark units are scaled by `scale` and offset by (ox, oy), all in supersampled
    pixels; `stroke` is in output pixels. With `snap`, the stroke is a whole
    number of output pixels and every edge lands on a pixel boundary, so a
    one-pixel line stays one crisp pixel instead of two grey ones.
    """
    mask = Image.new('L', size, 0)
    d = ImageDraw.Draw(mask)
    x, y, w, h = mark['rect']
    if snap:
        stroke = max(1, round(stroke))
    sw = stroke * SS
    half = sw / 2

    def at(v: float) -> float:
        # A stroke centre: on a pixel centre for an odd width, on an edge for an even one.
        if not snap:
            return v
        offset = 0.5 if stroke % 2 else 0.0
        return (round(v / SS - offset) + offset) * SS

    def p(px: float, py: float) -> tuple[float, float]:
        return (at(ox + px * scale), at(oy + py * scale))

    def box(x0: float, y0: float, x1: float, y1: float, r: float, fill: int) -> None:
        # Half-open, as an image is: Pillow's right and bottom edges are inclusive.
        d.rounded_rectangle([round(x0), round(y0), round(x1) - 1, round(y1) - 1], radius=max(0, r), fill=fill)

    (x0, y0), (x1, y1) = p(x, y), p(x + w, y + h)
    r = mark['radius'] * scale
    box(x0 - half, y0 - half, x1 + half, y1 + half, r + half, 255)
    box(x0 + half, y0 + half, x1 - half, y1 - half, r - half, 0)
    for a, b in mark['lines']:
        (ax, ay), (bx, by) = p(*a), p(*b)
        # A line with round caps is a capsule.
        box(min(ax, bx) - half, min(ay, by) - half, max(ax, bx) + half, max(ay, by) + half, half, 255)
    return mask


def coloured_mark(mark: dict, px: int, radius_ratio: float, inset: float = 0.0, tray: bool = False) -> Image.Image:
    """The favicon at `px` pixels: the green rounded square with the white notebook.

    `tray` draws it for a 16-48 px tray slot: the notebook fills more of the
    square (TRAY_GLYPH) with a whole-pixel stroke (TRAY_STROKE_PX) on pixel
    edges, because the favicon's proportions leave a 6-pixel notebook at 16 px.
    """
    big = px * SS
    img = Image.new('RGBA', (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = inset * big
    d.rounded_rectangle([pad, pad, big - pad - 1, big - pad - 1], radius=(big - 2 * pad) * radius_ratio, fill=mark['background'])
    if tray:
        stroke = max(1, px // TRAY_STROKE_PX)
        x, y, w, h = mark['rect']
        scale = (TRAY_GLYPH * big - stroke * SS) / h
        ox = (big - w * scale) / 2 - x * scale
        oy = (big - h * scale) / 2 - y * scale
    else:
        scale = (big - 2 * pad) / mark['size']
        stroke = mark['stroke'] * scale / SS
        ox = oy = pad
    mask = notebook_mask(img.size, mark, scale, ox, oy, stroke, snap=tray)
    img.paste(Image.new('RGBA', img.size, (255, 255, 255, 255)), (0, 0), mask)
    return img.resize((px, px), Image.BOX)


def app_icon(mark: dict) -> None:
    # The macOS grid: an 824-point rounded square centred on a 1024 canvas.
    size = 1024
    img = coloured_mark(mark, size, 185 / 824, inset=100 / 1024)
    out = ROOT / 'icons-src'
    out.mkdir(exist_ok=True)
    img.save(out / 'app-1024.png')


def template_icon(mark: dict, factor: int) -> Image.Image:
    """The notebook alone, black on transparent, GLYPH_H points tall, centred in the slot."""
    w, h = TEMPLATE_W * factor * SS, TEMPLATE_H * factor * SS
    img = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    _, _, rw, rh = mark['rect']
    rx, ry = mark['rect'][0], mark['rect'][1]
    stroke = TEMPLATE_STROKE_1X if factor == 1 else TEMPLATE_STROKE
    # Scale so the outside of the stroked rect is GLYPH_H points tall.
    scale = (GLYPH_H - stroke) * factor * SS / rh
    ox = (w - rw * scale) / 2 - rx * scale
    oy = (h - rh * scale) / 2 - ry * scale
    mask = notebook_mask(img.size, mark, scale, ox, oy, stroke * factor, snap=factor == 1)
    img.paste(Image.new('RGBA', img.size, (0, 0, 0, 255)), (0, 0), mask)
    return img.resize((TEMPLATE_W * factor, TEMPLATE_H * factor), Image.BOX)


def tray_icons(mark: dict) -> None:
    out = ROOT / 'src-tauri' / 'icons'
    template_icon(mark, 1).save(out / 'tray-template.png')
    template_icon(mark, 2).save(out / 'tray-template@2x.png')
    # A tray icon is the mark edge to edge, with the favicon's soft corners.
    frames = [coloured_mark(mark, s, 0.22, tray=True) for s in ICO_SIZES]
    frames[-1].save(out / 'tray.ico', format='ICO', sizes=[(s, s) for s in ICO_SIZES], append_images=frames[:-1])


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
    glyph = coloured_mark(mark, side, 0.22, tray=side <= ICO_SIZES[-1])
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


if __name__ == '__main__':
    geometry = load_mark(MARK)
    app_icon(geometry)
    tray_icons(geometry)
    msix_assets(geometry)
    if '--preview' in sys.argv:
        preview()
