#!/usr/bin/env python3
"""Generate the OxyBrush application icon (PNG sizes + Windows .ico).

The mark is `logo_mark.png` (cyan/magenta circuit-brush emblem). Each size composites it onto a dark rounded square so the
window, taskbar and installer icons match the README branding. Pillow is
required: `pip install Pillow`.

Usage: python3 packaging/icons/make_icons.py   (writes into packaging/icons/)
"""
from __future__ import annotations

import struct
import zlib
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent
MARK = OUT / "logo_mark.png"
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

# Charcoal rounded square behind the mark (matches the banner field).
BG = (0x1A, 0x1C, 0x20, 255)


def render(size: int, mark: Image.Image) -> Image.Image:
    """Square RGBA icon at `size` px."""
    icon = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(icon)
    margin = max(1.0, size * 0.02)
    radius = max(2.0, size * 0.22)
    draw.rounded_rectangle(
        [margin, margin, size - 1 - margin, size - 1 - margin],
        radius=radius,
        fill=BG,
    )
    # Leave a little ring of background around the mark.
    pad = max(1, int(round(size * (0.10 if size >= 48 else 0.06))))
    inner = size - 2 * pad
    if inner < 1:
        return icon
    scaled = mark.resize((inner, inner), Image.Resampling.LANCZOS)
    icon.alpha_composite(scaled, (pad, pad))
    return icon


def png_bytes(img: Image.Image) -> bytes:
    """Encode RGBA as a PNG (filter-none rows), no Pillow dependency for the blob."""

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    w, h = img.size
    raw = bytearray()
    pix = img.tobytes("raw", "RGBA")
    stride = w * 4
    for j in range(h):
        raw.append(0)
        raw.extend(pix[j * stride : (j + 1) * stride])
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def dib(img: Image.Image) -> bytes:
    """Uncompressed 32-bit icon image (BITMAPINFOHEADER, BGRA bottom-up, then
    the 1-bit AND mask). Windows draws these at every size and in every
    place (title bar, taskbar, Alt+Tab); PNG entries are only reliable at 256."""
    size = img.size[0]
    pix = img.tobytes("raw", "RGBA")
    stride = size * 4
    pixels = bytearray()
    mask = bytearray()
    mask_stride = ((size + 31) // 32) * 4
    for j in reversed(range(size)):  # bottom-up
        row = pix[j * stride : (j + 1) * stride]
        bits = bytearray(mask_stride)
        for i in range(size):
            r, g, b, a = row[i * 4 : i * 4 + 4]
            pixels += bytes((b, g, r, a))
            if a == 0:
                bits[i // 8] |= 0x80 >> (i % 8)
        mask += bits
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels) + len(mask), 0, 0, 0, 0)
    return header + bytes(pixels) + bytes(mask)


def ico(pngs: dict[int, bytes], imgs: dict[int, Image.Image]) -> bytes:
    """ICO container: uncompressed images below 256 px, PNG at 256 px (the
    layout Windows expects; PNG small sizes can show wrongly)."""
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries, data = b"", b""
    for s in sizes:
        blob = pngs[s] if s >= 256 else dib(imgs[s])
        dim = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main() -> None:
    if not MARK.is_file():
        raise SystemExit(f"missing {MARK}: extract the circular logo mark first")
    mark = Image.open(MARK).convert("RGBA")
    pngs: dict[int, bytes] = {}
    imgs: dict[int, Image.Image] = {}
    for s in SIZES:
        img = render(s, mark)
        imgs[s] = img
        pngs[s] = png_bytes(img)
        (OUT / f"oxybrush-{s}.png").write_bytes(pngs[s])
    (OUT / "oxybrush.ico").write_bytes(ico({s: pngs[s] for s in ICO_SIZES}, imgs))
    print("wrote", ", ".join(f"oxybrush-{s}.png" for s in SIZES), "and oxybrush.ico")


if __name__ == "__main__":
    main()
