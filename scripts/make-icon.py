#!/usr/bin/env python3
"""Generate crates/rigel-app/icons/icon.ico from the single-star mark.

The mark matches public/rigel.svg: an amber four-point star with a faint glow on a
near-black rounded tile. It is drawn at 1024 px and downsampled, so every size in the
.ico is antialiased. Requires Pillow (pip install pillow).
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw

BIG = 1024
BG = (0x0B, 0x0C, 0x0E, 255)
AMBER = (0xE9, 0xA2, 0x3B, 255)
GLOW = (0xE9, 0xA2, 0x3B, 36)  # 0.14 alpha, as in the SVG
SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]

# Coordinates are in the 24-unit SVG space, from public/rigel.svg.
STAR_CURVES = [
    ((12, 4), (12.9, 11.1), (20, 12)),
    ((20, 12), (12.9, 12.9), (12, 20)),
    ((12, 20), (11.1, 12.9), (4, 12)),
    ((4, 12), (11.1, 11.1), (12, 4)),
]


def to_px(x: float, y: float, scale: float) -> tuple[float, float]:
    return x * scale, y * scale


def quad_points(p0, c, p1, scale: float, steps: int = 40):
    out = []
    for i in range(steps):
        t = i / steps
        x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * c[0] + t**2 * p1[0]
        y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * c[1] + t**2 * p1[1]
        out.append(to_px(x, y, scale))
    return out


def render(size: int = BIG) -> Image.Image:
    scale = size / 24.0
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    draw.rounded_rectangle((0, 0, size - 1, size - 1), radius=5 * scale, fill=BG)

    r = 5.5 * scale
    cx = cy = 12 * scale
    draw.ellipse((cx - r, cy - r, cx + r, cy + r), fill=GLOW)

    points: list[tuple[float, float]] = []
    for p0, c, p1 in STAR_CURVES:
        points += quad_points(p0, c, p1, scale)
    draw.polygon(points, fill=AMBER)
    return img


def main() -> None:
    out = Path(__file__).resolve().parent.parent / "crates/rigel-app/icons/icon.ico"
    out.parent.mkdir(parents=True, exist_ok=True)
    render().save(out, format="ICO", sizes=SIZES)
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
