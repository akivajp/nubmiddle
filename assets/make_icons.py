# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=10"]
# ///
"""Generates the nubmiddle icons (a red TrackPoint nub between four keycaps).

Writes multi-size .ico files for the normal and the paused state, plus a preview sheet.
Small sizes (up to 24 px) use a simplified drawing so that the keys stay apart and the
nub stays readable in the notification area.

Usage:
    uv run assets/make_icons.py [--out-dir assets] [--preview PATH]
"""

from __future__ import annotations

import argparse
import io
import struct
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw

# Sizes stored in the .ico files (Windows picks the closest one for each use)
SIZES: list[int] = [16, 20, 24, 32, 40, 48, 64, 256]
# Sizes up to this use the simplified drawing
SMALL_MAX: int = 24
# Supersampling canvas size
CANVAS: int = 1024

RGB = tuple[int, int, int]


@dataclass(frozen=True)
class Palette:
    """Colors of one icon state."""

    key: RGB
    key_top: RGB
    nub: RGB
    nub_rim: RGB
    nub_dot: RGB


NORMAL = Palette(
    key=(58, 61, 66),
    key_top=(84, 88, 95),
    nub=(226, 35, 26),
    nub_rim=(150, 18, 12),
    nub_dot=(170, 22, 15),
)
PAUSED = Palette(
    key=(120, 122, 126),
    key_top=(150, 152, 156),
    nub=(170, 170, 170),
    nub_rim=(110, 110, 110),
    nub_dot=(130, 130, 130),
)


def draw(palette: Palette, small: bool) -> Image.Image:
    """Draws the icon on the supersampled canvas.

    Args:
        palette: Colors to use.
        small: Use the simplified drawing for small sizes (wider gaps, bigger nub, no texture).

    Returns:
        RGBA image of CANVAS x CANVAS pixels.
    """
    s = CANVAS
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    # Margin and key size: the gap between the keys is s - 2 * (margin + key)
    margin, key = (0.02, 0.41) if small else (0.05, 0.405)
    radius = 0.07 if small else 0.08
    for x0 in (margin, 1 - margin - key):
        for y0 in (margin, 1 - margin - key):
            box = [x0 * s, y0 * s, (x0 + key) * s, (y0 + key) * s]
            # Small sizes use the lighter color so the keys still show on a dark taskbar
            fill = palette.key_top if small else palette.key
            d.rounded_rectangle(box, radius=radius * s, fill=fill)
            if not small:
                # Key top: a lighter, slightly raised face
                top = [box[0] + 0.03 * s, box[1] + 0.02 * s, box[2] - 0.03 * s, box[3] - 0.06 * s]
                d.rounded_rectangle(top, radius=0.06 * s, fill=palette.key_top)
    # The nub sits in the gap between the four keys
    c = s / 2
    r = (0.25 if small else 0.205) * s
    d.ellipse([c - r, c - r, c + r, c + r], fill=palette.nub_rim)
    r2 = r * (0.80 if small else 0.86)
    d.ellipse([c - r2, c - r2, c + r2, c + r2], fill=palette.nub)
    if not small:
        # Dotted texture of the TrackPoint cap
        step, dot = r * 0.32, r * 0.07
        for i in range(-3, 4):
            for j in range(-3, 4):
                x, y = c + i * step, c + j * step
                if (x - c) ** 2 + (y - c) ** 2 < (r2 * 0.8) ** 2:
                    d.ellipse([x - dot, y - dot, x + dot, y + dot], fill=palette.nub_dot)
    return im


def render(palette: Palette) -> dict[int, Image.Image]:
    """Renders every size in SIZES.

    Args:
        palette: Colors to use.

    Returns:
        Mapping from size to RGBA image.
    """
    big, small = draw(palette, small=False), draw(palette, small=True)
    return {
        size: (small if size <= SMALL_MAX else big).resize((size, size), Image.LANCZOS)
        for size in SIZES
    }


def write_ico(path: Path, images: dict[int, Image.Image]) -> None:
    """Writes a .ico file with one PNG-compressed entry per size (supported since Windows Vista).

    Args:
        path: Output file.
        images: Mapping from size to RGBA image.
    """
    blobs: list[tuple[int, bytes]] = []
    for size, im in sorted(images.items()):
        buf = io.BytesIO()
        im.save(buf, format="PNG")
        blobs.append((size, buf.getvalue()))
    # ICONDIR, then one ICONDIRENTRY per image, then the image data
    header = struct.pack("<HHH", 0, 1, len(blobs))
    offset = 6 + 16 * len(blobs)
    entries, data = b"", b""
    for size, blob in blobs:
        dim = 0 if size >= 256 else size  # 0 means 256
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
        data += blob
    path.write_bytes(header + entries + data)


def preview(path: Path, sets: list[dict[int, Image.Image]]) -> None:
    """Writes a sheet showing all sizes on light and dark backgrounds.

    Args:
        path: Output PNG file.
        sets: Rendered icon sets, one row pair each.
    """
    row_h, pad = 280, 16
    width = pad + sum(s + pad for s in SIZES)
    sheet = Image.new("RGB", (width, row_h * 2 * len(sets)), (255, 255, 255))
    y = 0
    for images in sets:
        for bg in ((243, 243, 243), (32, 32, 32)):
            ImageDraw.Draw(sheet).rectangle([0, y, width, y + row_h], fill=bg)
            x = pad
            for size in SIZES:
                im = images[size]
                sheet.paste(im, (x, y + (row_h - size) // 2), im)
                x += size + pad
            y += row_h
    sheet.save(path)


def main() -> None:
    """Parses the command line and writes the icons."""
    parser = argparse.ArgumentParser(description="Generate the nubmiddle icons.")
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path(__file__).resolve().parent,
        help="folder for nubmiddle.ico and nubmiddle-paused.ico (default: this script's folder)",
    )
    parser.add_argument("--preview", type=Path, help="also write a preview sheet (PNG) here")
    args = parser.parse_args()

    normal, paused = render(NORMAL), render(PAUSED)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    write_ico(args.out_dir / "nubmiddle.ico", normal)
    write_ico(args.out_dir / "nubmiddle-paused.ico", paused)
    print(f"wrote {args.out_dir / 'nubmiddle.ico'} and {args.out_dir / 'nubmiddle-paused.ico'}")
    if args.preview:
        preview(args.preview, [normal, paused])
        print(f"wrote {args.preview}")


if __name__ == "__main__":
    main()
