"""Generate the original speech-bubble mark as SVG, PNG, ICO and runtime RGBA.

Python standard library only. All coordinates use a 256-unit design grid.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
ASSETS.mkdir(exist_ok=True)


def rounded(x, y, left, top, right, bottom, radius):
    if not (left <= x <= right and top <= y <= bottom):
        return False
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, top + radius), bottom - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2


def triangle(x, y):
    points = [(78, 168), (78, 207), (121, 171)]
    cross = [(b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
             for a, b in zip(points, points[1:] + points[:1])]
    return all(c >= 0 for c in cross) or all(c <= 0 for c in cross)


def pixel(x, y):
    if not rounded(x, y, 8, 8, 248, 248, 61):
        return None
    color = (57, 61, 71)
    if rounded(x, y, 11, 11, 245, 245, 58):
        color = (26, 28, 34)
    if rounded(x, y, 46, 57, 210, 181, 34) or triangle(x, y):
        color = (255, 107, 94)
    for left, top, right, bottom in [(84, 100, 100, 140), (120, 80, 136, 160), (156, 108, 172, 132)]:
        if rounded(x, y, left, top, right, bottom, 8):
            color = (255, 250, 246)
    return color


def render(size):
    rgba = bytearray()
    samples = 4
    for y in range(size):
        for x in range(size):
            colors = [pixel((x + (sx + .5) / samples) * 256 / size,
                            (y + (sy + .5) / samples) * 256 / size)
                      for sy in range(samples) for sx in range(samples)]
            visible = [c for c in colors if c is not None]
            if visible:
                rgba.extend(round(sum(c[channel] for c in visible) / len(visible)) for channel in range(3))
                rgba.append(round(255 * len(visible) / len(colors)))
            else:
                rgba.extend((0, 0, 0, 0))
    return bytes(rgba)


def png(size, rgba):
    def chunk(kind, content):
        return struct.pack(">I", len(content)) + kind + content + struct.pack(">I", zlib.crc32(kind + content))
    rows = b"".join(b"\0" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
images = []
for size in sizes:
    rgba = render(size)
    image = png(size, rgba)
    images.append(image)
    if size in (32, 128):
        (ASSETS / f"icon-{size}.rgba").write_bytes(rgba)
    if size == 256:
        (ASSETS / "app-icon.png").write_bytes(image)
offset = 6 + 16 * len(sizes)
entries = []
for size, image in zip(sizes, images):
    entries.append(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset))
    offset += len(image)
(ASSETS / "app.ico").write_bytes(struct.pack("<HHH", 0, 1, len(sizes)) + b"".join(entries + images))
(ASSETS / "app-icon.svg").write_text('''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
<title>Vibe Voice Lite — voice and conversation</title>
<rect x="8" y="8" width="240" height="240" rx="61" fill="#393d47"/>
<rect x="11" y="11" width="234" height="234" rx="58" fill="#1a1c22"/>
<g fill="#ff6b5e"><rect x="46" y="57" width="164" height="124" rx="34"/>
<path d="M78 168V207L121 171Z"/></g>
<g fill="#fffaf6"><rect x="84" y="100" width="16" height="40" rx="8"/>
<rect x="120" y="80" width="16" height="80" rx="8"/>
<rect x="156" y="108" width="16" height="24" rx="8"/></g>
</svg>
''', encoding="utf-8")
print("Generated SVG, PNG, nine-resolution ICO, window and tray RGBA")
