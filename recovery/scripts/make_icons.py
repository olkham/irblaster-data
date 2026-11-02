#!/usr/bin/env python3
"""Draws the app icons from the brand mark (assets/favicon.svg): four infrared bars on a dark tile.

    python recovery/scripts/make_icons.py

Writes recovery/assets/icon.png (1024 px), icon.ico (Windows) and icon.icns (macOS). Needs Pillow.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ASSETS = Path(__file__).resolve().parents[1] / "assets"
TILE = (0x12, 0x16, 0x1D, 255)
BAR = (0xFF, 0x3B, 0x5C, 255)
# The favicon's 32-unit geometry: tile radius 8, bars (x, y, width, height) with radius 1.2.
BARS = [(5, 9, 7, 14), (15, 9, 2.4, 14), (19.6, 9, 2.4, 14), (25, 9, 2.4, 14)]


def draw(size: int) -> Image.Image:
    scale = 4  # draw large, then downsample for smooth edges
    big = size * scale
    unit = big / 32
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    pen = ImageDraw.Draw(image)
    pen.rounded_rectangle((0, 0, big - 1, big - 1), radius=8 * unit, fill=TILE)
    for x, y, w, h in BARS:
        pen.rounded_rectangle((x * unit, y * unit, (x + w) * unit, (y + h) * unit), radius=1.2 * unit, fill=BAR)
    return image.resize((size, size), Image.LANCZOS)


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    master = draw(1024)
    master.save(ASSETS / "icon.png")
    draw(256).save(ASSETS / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    master.save(ASSETS / "icon.icns")
    for name in ("icon.png", "icon.ico", "icon.icns"):
        print(ASSETS / name, (ASSETS / name).stat().st_size, "bytes")


if __name__ == "__main__":
    main()
