#!/usr/bin/env python3
"""Generate the installer artwork the WiX and NSIS bundles embed.

Four bitmaps, all from the shell's own tokens (`ui/src/shell.css`) and the four-point
star of `scripts/make-icon.py`:

  wix-dialog.bmp    493x312  the left strip of the MSI wizard's welcome/finish pages
  wix-banner.bmp    493x58   the top bar of the MSI wizard's other pages
  nsis-sidebar.bmp  164x314  the NSIS welcome/finish sidebar
  nsis-header.bmp   150x57   the NSIS page header

Geometry comes from the WiX UI extension, not from taste: the dialog bitmap is 493 px
wide but only its left 164 px are drawn — the dialog's own white background covers the
rest — and the banner's right end is where the stock WiX logo square sits, with the
page title rendered in black over the light area to its left. So the MSI art is a dark
strip plus a light banner; anything else makes the wizard's own text unreadable. NSIS
draws its page text beside the sidebar and header in its own colors, so those two are
fully dark.

Requires Pillow (pip install pillow), same as scripts/make-icon.py.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

# Straight from ui/src/shell.css, so the installer and the app cannot drift apart.
BG = (0x0B, 0x0C, 0x0E)
SURFACE_TOP = (0x15, 0x18, 0x1D)
TEXT = (0xE6, 0xE8, 0xEB)
MUTED = (0x9B, 0xA1, 0xA9)
FAINT = (0x6B, 0x72, 0x7C)
ACCENT = (0xE9, 0xA2, 0x3B)
PAPER = (0xFF, 0xFF, 0xFF)
INK = (0x0B, 0x0C, 0x0E)

WIX_DIALOG = (493, 312)
WIX_STRIP = 164  # the only part of the dialog bitmap the wizard paints
WIX_BANNER = (493, 58)
NSIS_SIDEBAR = (164, 314)
NSIS_HEADER = (150, 57)

TAGLINE = ("One window for", "several coding agents", "on one project.")
CAPTION = "SESSION LOGS STAY LOCAL"

# The mark, in the 24-unit space of public/rigel.svg: four quadratic tips.
STAR_CURVES = [
    ((12, 4), (12.9, 11.1), (20, 12)),
    ((20, 12), (12.9, 12.9), (12, 20)),
    ((12, 20), (11.1, 12.9), (4, 12)),
    ((4, 12), (11.1, 11.1), (12, 4)),
]

# Inter first because shell.css names it; Segoe UI is what Windows actually has, and its
# semibold carries a caps wordmark better than its regular.
SEMIBOLD_FONTS = [
    "C:/Windows/Fonts/Inter-SemiBold.ttf",
    "C:/Windows/Fonts/seguisb.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
]
REGULAR_FONTS = [
    "C:/Windows/Fonts/Inter-Regular.ttf",
    "C:/Windows/Fonts/segoeui.ttf",
    "C:/Windows/Fonts/arial.ttf",
]
MONO_FONTS = [
    "C:/Windows/Fonts/JetBrainsMono-Regular.ttf",
    "C:/Windows/Fonts/CascadiaMono.ttf",
    "C:/Windows/Fonts/consola.ttf",
    "C:/Windows/Fonts/cour.ttf",
]

OUT = Path(__file__).resolve().parent.parent / "crates/rigel-app/icons/installer"


def pick_font(paths: list[str], size: int) -> ImageFont.FreeTypeFont:
    for path in paths:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    # A machine without any of them still gets art, just in the bundled default face.
    return ImageFont.load_default(size)


def star_points(cx: float, cy: float, size: float, steps: int = 48) -> list[tuple[float, float]]:
    """The four-point mark centred on (cx, cy), as a polygon in device pixels."""
    scale = size / 24.0
    points: list[tuple[float, float]] = []
    origin_x = cx - size / 2
    origin_y = cy - size / 2
    for p0, control, p1 in STAR_CURVES:
        for step in range(steps):
            t = step / steps
            x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * control[0] + t**2 * p1[0]
            y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * control[1] + t**2 * p1[1]
            points.append((origin_x + x * scale, origin_y + y * scale))
    return points


def gradient(size: tuple[int, int], top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    image = Image.new("RGB", size)
    draw = ImageDraw.Draw(image)
    for y in range(size[1]):
        t = y / max(size[1] - 1, 1)
        draw.line(
            [(0, y), (size[0], y)],
            fill=tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)),
        )
    return image


def glow(size: tuple[int, int], cx: float, cy: float, radius: float, alpha: int) -> Image.Image:
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse(
        (cx - radius, cy - radius, cx + radius, cy + radius), fill=ACCENT + (alpha,)
    )
    return layer.filter(ImageFilter.GaussianBlur(radius * 0.55))


def draw_tracked(
    draw: ImageDraw.ImageDraw,
    text: str,
    font: ImageFont.FreeTypeFont,
    tracking: float,
    cx: float,
    cy: float,
    fill: tuple[int, int, int],
) -> float:
    """Letter-spaced text centred on (cx, cy); returns the width it took."""
    widths = [draw.textlength(character, font=font) for character in text]
    total = sum(widths) + tracking * (len(text) - 1)
    left, top, _, bottom = draw.textbbox((0, 0), text, font=font)
    x = cx - total / 2
    y = cy - (top + bottom) / 2
    for character, width in zip(text, widths):
        draw.text((x, y), character, font=font, fill=fill)
        x += width + tracking
    return total


def dark_panel(height: int) -> Image.Image:
    """The left strip: the app's window, in miniature."""
    width = WIX_STRIP
    image = gradient((width, height), SURFACE_TOP, BG).convert("RGBA")
    cx = width / 2
    star_y = 104.0
    shift = height - 312  # the NSIS sidebar is two pixels taller

    image.alpha_composite(glow((width, height), cx, star_y, 54, 46))

    watermark = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    ImageDraw.Draw(watermark).polygon(
        star_points(width - 14, height + 12, 190), fill=ACCENT + (16,)
    )
    image.alpha_composite(watermark)

    draw = ImageDraw.Draw(image)
    draw.polygon(star_points(cx, star_y, 66), fill=ACCENT)

    wordmark = pick_font(SEMIBOLD_FONTS, 20)
    draw_tracked(draw, "RIGEL", wordmark, 5.0, cx, 165 + shift, TEXT)

    draw.rectangle((cx - 11, 184 + shift, cx + 10, 186 + shift), fill=ACCENT)

    tagline = pick_font(REGULAR_FONTS, 11)
    for index, line in enumerate(TAGLINE):
        draw_tracked(draw, line, tagline, 0.0, cx, 205 + shift + index * 15, MUTED)

    caption = pick_font(MONO_FONTS, 8)
    draw_tracked(draw, CAPTION, caption, 1.2, cx, 286 + shift, FAINT)

    # A hairline seam where the strip meets the wizard's white page, so the edge reads as
    # design rather than as a bitmap that ran out.
    seam = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    ImageDraw.Draw(seam).rectangle((width - 1, 0, width - 1, height), fill=ACCENT + (110,))
    image.alpha_composite(seam)
    return image


def lockup(
    size: tuple[int, int],
    star: float,
    wordmark_size: int,
    tracking: float,
    right_margin: float,
    ink: tuple[int, int, int],
    cy: float,
) -> Image.Image:
    """The star + RIGEL lockup, right-aligned inside `size`, on transparency.

    The star stays amber on light and dark alike — it is the one accent — while the
    wordmark takes whichever ink the background needs.
    """
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    font = pick_font(SEMIBOLD_FONTS, wordmark_size)
    text_width = draw.textlength("RIGEL", font=font) + tracking * 4
    gap = star * 0.45
    total = star + gap + text_width
    start = size[0] - right_margin - total
    draw.polygon(star_points(start + star / 2, cy, star), fill=ACCENT)
    draw_tracked(draw, "RIGEL", font, tracking, start + star + gap + text_width / 2, cy, ink)
    return layer


def wix_dialog() -> Image.Image:
    width, height = WIX_DIALOG
    image = Image.new("RGBA", (width, height), PAPER)
    image.alpha_composite(dark_panel(height), (0, 0))
    return image.convert("RGB")


def wix_banner() -> Image.Image:
    width, height = WIX_BANNER
    image = Image.new("RGBA", (width, height), PAPER)
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, height - 2, width - 1, height - 1), fill=ACCENT)
    # The page title is printed in black at the left of this bar; the mark keeps to the
    # right end, where the stock WiX logo sat.
    image.alpha_composite(lockup((width, height), 22, 14, 2.5, 26, INK, (height - 2) / 2))
    return image.convert("RGB")


def nsis_sidebar() -> Image.Image:
    return dark_panel(NSIS_SIDEBAR[1]).convert("RGB")


def nsis_header() -> Image.Image:
    width, height = NSIS_HEADER
    image = Image.new("RGBA", (width, height), INK + (255,))
    ImageDraw.Draw(image).rectangle((0, height - 2, width - 1, height - 1), fill=ACCENT)
    image.alpha_composite(lockup((width, height), 20, 13, 2.5, 16, TEXT, (height - 2) / 2))
    return image.convert("RGB")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    outputs = {
        "wix-dialog.bmp": wix_dialog(),
        "wix-banner.bmp": wix_banner(),
        "nsis-sidebar.bmp": nsis_sidebar(),
        "nsis-header.bmp": nsis_header(),
    }
    for name, image in outputs.items():
        path = OUT / name
        # 24-bit: what MSI bitmap controls and NSIS both expect.
        image.convert("RGB").save(path, format="BMP")
        print(f"wrote {path} ({image.size[0]}x{image.size[1]}, {path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
