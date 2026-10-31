#!/usr/bin/env python3
"""Draw the Rigel icon once and export every slot the bundle needs.

    python ios/tools/make-icon.py

Writes into `ios/RigelPhone/Resources/Icon/`:

- `AppIcon-1024.png`   the marketing icon (opaque, no alpha, square);
- `AppIcon60x60@2x.png` / `@3x`, `AppIcon40x40@2x/@3x`, `AppIcon29x29@2x/@3x`,
  `AppIcon20x20@2x/@3x` for iPhone, and the iPad pair `AppIcon76x76@2x`,
  `AppIcon83.5x83.5@2x`, plus the shared 29/40 slots;
- `LaunchStar.png`     the mark on its own, for the launch screen.

The mark is the star from Rigel's logo — the same four-point glyph as
`ui/src/mark.tsx` — over a warm bloom on near-black. No text: an icon that
names the app is an icon that fails at 29 px (the /ios-icon rule).

iOS forbids transparency in a shipping icon, so the plate stays; the *in-app*
mark is the same star on transparency, with a soft glow.

Everything is drawn at 4096 px and downsampled, which is the cheapest way to
get clean edges out of PIL's aliased polygon fill.
"""
from __future__ import annotations

import math
import pathlib

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

S = 4096
OUT = pathlib.Path(__file__).resolve().parent.parent / "RigelPhone" / "Resources" / "Icon"

INK = np.array([8, 9, 11], dtype=np.float32)       # #08090b
BLOOM = np.array([233, 162, 59], dtype=np.float32)  # #e9a23b, the desktop accent
AMBER = (233, 162, 59)
AMBER_LIGHT = (255, 214, 140)
CREAM = (255, 246, 226)


def background() -> Image.Image:
    """Near-black, cool at the top, warm where the star burns."""
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float32)
    cx, cy = S * 0.5, S * 0.45

    # Vertical base: a cold navy shoulder fading to true black.
    t = np.clip(yy / S, 0.0, 1.0)
    cold = np.array([14, 18, 27], dtype=np.float32)
    plate = cold[None, None, :] * (1 - t[..., None] ** 0.75) + INK[None, None, :] * (
        t[..., None] ** 0.75
    )

    # A cold pool of light behind everything, then the warm halo on top of it.
    cold_r = np.clip(np.sqrt(((xx - cx) / S) ** 2 + ((yy - S * 0.30) / S) ** 2) / 0.62, 0.0, 1.0)
    plate = plate + (np.array([26, 36, 58], np.float32) - plate) * ((1 - cold_r) ** 3.0 * 0.55)[..., None]

    # A halo around the mark, tight enough to feel lit rather than airbrushed.
    bloom_r = np.clip(np.sqrt(((xx - cx) / S) ** 2 + ((yy - cy) / S) ** 2) / 0.30, 0.0, 1.0)
    bloom = (1.0 - bloom_r) ** 3.4
    plate = plate + (BLOOM - plate) * (bloom[..., None] * 0.20)

    plate += np.random.default_rng(7).normal(0, 0.55, plate.shape).astype(np.float32)
    return Image.fromarray(np.clip(plate, 0, 255).astype(np.uint8), "RGB")


def sparkle(cx: float, cy: float, outer: float, waist: float = 0.30) -> list[tuple[float, float]]:
    """Four points on the axes, four deep waists between them."""
    points = []
    for index in range(8):
        angle = -math.pi / 2 + index * math.pi / 4
        radius = outer if index % 2 == 0 else outer * waist
        points.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    return points


def star_image() -> Image.Image:
    """The mark alone, on transparency, for the launch screen."""
    cx, cy = S * 0.5, S * 0.45
    outer = S * 0.315

    # Glow: the mark, blurred, screened underneath itself — two passes, so the
    # falloff is tight at the edge and long in the air.
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(glow).polygon(sparkle(cx, cy, outer * 1.01), fill=AMBER + (120,))
    glow = glow.filter(ImageFilter.GaussianBlur(S * 0.028))
    wide = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(wide).polygon(sparkle(cx, cy, outer * 1.04), fill=AMBER + (44,))
    wide = wide.filter(ImageFilter.GaussianBlur(S * 0.075))
    glow.alpha_composite(wide)

    # The body: a vertical gradient clipped to the star.
    body = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    gradient = Image.new("RGBA", (S, S))
    grad_draw = ImageDraw.Draw(gradient)
    top, bottom = cy - outer, cy + outer
    for y in range(S):
        t = min(max((y - top) / max(bottom - top, 1), 0.0), 1.0)
        colour = tuple(
            int(AMBER_LIGHT[i] * (1 - t) + AMBER[i] * t) for i in range(3)
        ) + (255,)
        grad_draw.line([(0, y), (S, y)], fill=colour)
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).polygon(sparkle(cx, cy, outer), fill=255)
    body.paste(gradient, (0, 0), mask)

    # The core is a smaller star, not a disc: a disc on a star reads as a
    # sticker, while a flare reads as the same light gathered tighter.
    core = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    halo = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(halo).polygon(
        sparkle(cx, cy, outer * 0.34, waist=0.26), fill=CREAM + (200,)
    )
    halo = halo.filter(ImageFilter.GaussianBlur(S * 0.007))
    ImageDraw.Draw(core).polygon(
        sparkle(cx, cy, outer * 0.17, waist=0.22), fill=(255, 253, 244, 255)
    )
    core.alpha_composite(halo)

    mark = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    mark.alpha_composite(glow)
    mark.alpha_composite(body)
    mark.alpha_composite(core)
    return mark


def compose() -> Image.Image:
    """The full square icon: plate, a thin orbit arc, then the mark."""
    plate = background().convert("RGBA")
    cx, cy = S * 0.5, S * 0.44

    # A handful of faint stars, never close enough to crowd the mark. Fixed
    # seed, so every build of the icon is the same icon.
    dust = ImageDraw.Draw(plate)
    rng = np.random.default_rng(11)
    placed = 0
    while placed < 24:
        x = float(rng.uniform(0.06, 0.94)) * S
        y = float(rng.uniform(0.05, 0.93)) * S
        if math.hypot(x - cx, y - cy) < S * 0.34:
            continue
        radius = float(rng.uniform(1.1, 2.8)) * (S / 1024)
        alpha = int(rng.uniform(26, 78))
        dust.ellipse([x - radius, y - radius, x + radius, y + radius], fill=(214, 228, 255, alpha))
        placed += 1

    orbit = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ring = [cx - S * 0.395, cy - S * 0.395, cx + S * 0.395, cy + S * 0.395]
    ring_draw = ImageDraw.Draw(orbit)
    # A full, quiet orbit, and one brighter quarter where the light is.
    ring_draw.ellipse(ring, outline=(210, 226, 255, 34), width=max(int(S * 0.0022), 2))
    ring_draw.arc(ring, start=196, end=344, fill=AMBER + (128,), width=max(int(S * 0.0032), 2))
    orbit = orbit.filter(ImageFilter.GaussianBlur(S * 0.0022))
    plate.alpha_composite(orbit)
    plate.alpha_composite(star_image())

    # Corner shading, so the plate has depth without a visible seam.
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float32)
    edge = np.clip(
        np.sqrt(((xx - S / 2) / (S / 2)) ** 2 + ((yy - S / 2) / (S / 2)) ** 2) / 1.35, 0, 1
    ) ** 2
    shade = Image.fromarray(np.clip(255 - edge * 42, 0, 255).astype(np.uint8), "L")
    plate = Image.composite(plate, Image.new("RGBA", (S, S), (0, 0, 0, 255)), shade.point(lambda v: v))
    return plate.convert("RGB")


SLOTS = {
    "AppIcon60x60@2x.png": 120,
    "AppIcon60x60@3x.png": 180,
    "AppIcon40x40@2x.png": 80,
    "AppIcon40x40@3x.png": 120,
    "AppIcon29x29@2x.png": 58,
    "AppIcon29x29@3x.png": 87,
    "AppIcon20x20@2x.png": 40,
    "AppIcon20x20@3x.png": 60,
    "AppIcon76x76@2x.png": 152,
    "AppIcon83.5x83.5@2x.png": 167,
}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    icon = compose()
    mark = star_image()

    icon.resize((1024, 1024), Image.LANCZOS).save(OUT / "AppIcon-1024.png", optimize=True)
    for name, size in SLOTS.items():
        icon.resize((size, size), Image.LANCZOS).save(OUT / name, optimize=True)

    mark_on_plate = Image.new("RGBA", (S, S), (11, 12, 14, 255))
    mark_on_plate.alpha_composite(mark)
    mark_on_plate.resize((512, 512), Image.LANCZOS).save(
        OUT / "LaunchStar.png", optimize=True
    )

    print("wrote", len(SLOTS) + 2, "files to", OUT)


if __name__ == "__main__":
    main()
