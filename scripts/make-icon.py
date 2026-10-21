#!/usr/bin/env python3
"""Generate crates/rigel-app/icons/icon.ico from the star-chain mark.

The mark is a chain of six stars on near-black. This rasterises it at 32x32 with the same
palette as the SVG (#0b0c0e background, #e9a23b stars) so the window icon matches the
favicon without pulling an image library into the build.
"""

from __future__ import annotations

import struct
from pathlib import Path

SIZE = 32
BG = (0x0B, 0x0C, 0x0E)
AMBER = (0xE9, 0xA2, 0x3B)

# Star centres and radii in the 24-unit SVG space, from public/rigel.svg.
STARS = [
    (10.6, 12.4, 0.8),
    (12.6, 11.9, 0.8),
    (14.6, 11.4, 0.8),
    (15.4, 6.2, 0.55),
    (7.2, 5.6, 1.15),
    (17.4, 18.6, 1.05),
]


def build_pixels() -> bytearray:
    scale = SIZE / 24.0
    pixels = bytearray()

    # BMP rows are bottom-up.
    for y in range(SIZE - 1, -1, -1):
        for x in range(SIZE):
            cx, cy = (x + 0.5) / scale, (y + 0.5) / scale
            lit = False
            for sx, sy, r in STARS:
                rr = max(r * scale, 0.9)
                if (cx - sx) ** 2 + (cy - sy) ** 2 <= rr * rr:
                    lit = True
                    break
            r, g, b = AMBER if lit else BG
            pixels += bytes((b, g, r, 255))
    return pixels


def build_ico(pixels: bytearray) -> bytes:
    header = struct.pack("<IiiHHIIiiII", 40, SIZE, SIZE * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    mask = bytes((SIZE // 8) * SIZE)  # all zero: no transparency holes
    image = header + bytes(pixels) + mask

    directory = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", SIZE, SIZE, 0, 0, 1, 32, len(image), 22)
    return directory + entry + image


out = Path(__file__).resolve().parent.parent / "crates/rigel-app/icons/icon.ico"
out.parent.mkdir(parents=True, exist_ok=True)
out.write_bytes(build_ico(build_pixels()))
print(f"wrote {out} ({out.stat().st_size} bytes)")
