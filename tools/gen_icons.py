"""Draws GBMapping's icons (crates/gbmapping/assets/icon-*.png): a little map of 4x4 tiles
with one of them selected. No dependency: the PNGs are written by hand.

    python tools/gen_icons.py
"""
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "crates" / "gbmapping" / "assets"

GROUND = (27, 27, 27)
GRASS, TALL, WATER, DEEP, PATH, TREE, ROOF, WALL = (120, 200, 120), (72, 160, 88), (96, 160, 232), (64, 120, 208), (224, 200, 144), (40, 112, 64), (216, 88, 72), (240, 232, 208)
MAP = [
    [TREE, GRASS, GRASS, WATER],
    [GRASS, ROOF, PATH, WATER],
    [TALL, WALL, PATH, DEEP],
    [TALL, TALL, PATH, GRASS],
]
SELECTED = (2, 1)
ACCENT = (255, 255, 255)


def draw(size, inset):
    """`inset`: the share of the icon left around the map (maskable icons need more)."""
    pixels = [[GROUND] * size for _ in range(size)]
    tile = size * (1 - 2 * inset) / 4
    gap = max(1, round(tile * 0.06))
    for ty in range(4):
        for tx in range(4):
            x0, y0 = round(size * inset + tx * tile) + gap, round(size * inset + ty * tile) + gap
            x1, y1 = round(size * inset + (tx + 1) * tile) - gap, round(size * inset + (ty + 1) * tile) - gap
            color = MAP[ty][tx]
            shade = tuple(max(0, c - 28) for c in color)
            for y in range(y0, y1):
                for x in range(x0, x1):
                    # a darker edge at the bottom and the right: tiles, not flat squares
                    edge = y >= y1 - 2 * gap or x >= x1 - 2 * gap
                    pixels[y][x] = shade if edge else color
            if (tx, ty) == SELECTED:
                width = max(2, round(tile * 0.09))
                for y in range(y0 - gap, y1 + gap):
                    for x in range(x0 - gap, x1 + gap):
                        if min(x - (x0 - gap), (x1 + gap - 1) - x, y - (y0 - gap), (y1 + gap - 1) - y) < width:
                            pixels[y][x] = ACCENT
    return pixels


def png(path, pixels):
    size = len(pixels)
    raw = b"".join(b"\x00" + bytes(c for pixel in row for c in pixel) for row in pixels)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
    print(path, f"{size}x{size}")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    png(OUT / "icon-192.png", draw(192, 0.08))
    png(OUT / "icon-512.png", draw(512, 0.08))
    png(OUT / "icon-maskable-512.png", draw(512, 0.2))
