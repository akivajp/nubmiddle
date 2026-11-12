# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=10"]
# ///
"""Generates the nubmiddle icons (a red TrackPoint nub between four keycaps).

Writes multi-size .ico files for the normal and the paused state, a 256 px PNG for the README,
the images of the MSIX package (packaging/msix/Assets, picked by makepri for each scale and
size), the Store's 1:1 box art, and optionally a preview sheet.
Small sizes (up to 24 px) use a simplified drawing so that the keys stay apart and the
nub stays readable in the notification area.

Usage:
    uv run assets/make_icons.py [--out-dir assets] [--msix-dir packaging/msix/Assets] [--preview PATH]
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


def icon(palette: Palette, size: int) -> Image.Image:
    """Renders the icon at one size (the simplified drawing up to SMALL_MAX).

    Args:
        palette: Colors to use.
        size: Width and height in pixels.

    Returns:
        RGBA image of size x size pixels.
    """
    return draw(palette, small=size <= SMALL_MAX).resize((size, size), Image.LANCZOS)


def tile(palette: Palette, width: int, height: int, fill: float) -> Image.Image:
    """A transparent tile with the icon centered, `fill` of the tile's height tall.

    Args:
        palette: Colors to use.
        width: Tile width in pixels.
        height: Tile height in pixels.
        fill: Icon height as a fraction of the tile height.

    Returns:
        RGBA image.
    """
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    size = round(height * fill)
    canvas.paste(icon(palette, size), ((width - size) // 2, (height - size) // 2))
    return canvas


def write_msix_assets(out: Path) -> int:
    """Writes the images the MSIX manifest refers to, one file per scale and target size.

    Args:
        out: The Assets folder (existing PNGs are replaced).

    Returns:
        Number of images written.
    """
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob("*.png"):
        old.unlink()
    written = 0

    def save(image: Image.Image, name: str) -> None:
        nonlocal written
        image.save(out / name, optimize=True)
        written += 1

    for scale in (100, 125, 150, 200, 400):
        f = scale / 100
        # The app list, the taskbar and the title bar: the icon itself
        save(icon(NORMAL, round(44 * f)), f"Square44x44Logo.scale-{scale}.png")
        # The Start menu tiles and the Store: the icon with some room around it
        save(tile(NORMAL, round(150 * f), round(150 * f), 0.66), f"Square150x150Logo.scale-{scale}.png")
        save(tile(NORMAL, round(310 * f), round(150 * f), 0.66), f"Wide310x150Logo.scale-{scale}.png")
        save(icon(NORMAL, round(50 * f)), f"StoreLogo.scale-{scale}.png")
    # Exact sizes for the taskbar and Explorer; "unplated" ones get no colored plate behind them
    for size in (16, 24, 32, 48, 256):
        for suffix in ("", "_altform-unplated", "_altform-lightunplated"):
            save(icon(NORMAL, size), f"Square44x44Logo.targetsize-{size}{suffix}.png")
    # The Store's 1:1 box art (shown in the Store's lists): the icon on a dark plate
    art = Image.new("RGBA", (1080, 1080), (38, 40, 44, 255))
    big = icon(NORMAL, 760)
    art.paste(big, (160, 160), big)
    art.convert("RGB").save(out.parent / "StoreBoxArt-1080.png", optimize=True)
    return written + 1


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
        help="folder for nubmiddle.ico, nubmiddle-paused.ico and nubmiddle.png "
        "(default: this script's folder)",
    )
    parser.add_argument(
        "--msix-dir",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "packaging" / "msix" / "Assets",
        help="folder for the MSIX package's images (default: packaging/msix/Assets)",
    )
    parser.add_argument("--preview", type=Path, help="also write a preview sheet (PNG) here")
    args = parser.parse_args()

    normal, paused = render(NORMAL), render(PAUSED)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    write_ico(args.out_dir / "nubmiddle.ico", normal)
    write_ico(args.out_dir / "nubmiddle-paused.ico", paused)
    normal[256].save(args.out_dir / "nubmiddle.png")
    print(f"wrote nubmiddle.ico, nubmiddle-paused.ico and nubmiddle.png in {args.out_dir}")
    n = write_msix_assets(args.msix_dir)
    print(f"wrote {n} MSIX images in {args.msix_dir} (and the Store box art next to it)")
    if args.preview:
        preview(args.preview, [normal, paused])
        print(f"wrote {args.preview}")


if __name__ == "__main__":
    main()
