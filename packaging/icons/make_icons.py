#!/usr/bin/env python3
"""Generate the OxyBrush application icon (PNG sizes + Windows .ico).

The icon is drawn procedurally so it is reproducible and original (CC0):
a terracotta clay sphere, shaded like the app's default matcap, on a dark
graphite rounded square, with the accent-colored brush ring offset to the
lower right. Standard library only (zlib + struct), no image libraries.

Usage: python3 packaging/icons/make_icons.py   (writes into packaging/icons/)
"""
import math
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent
SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]

BG = (0x1E, 0x20, 0x24)
ACCENT = (0xE0, 0x7A, 0x4F)
CLAY = (0.62, 0.26, 0.15)  # linear terracotta, same as the default matcap


def srgb(x: float) -> float:
    x = min(max(x, 0.0), 1.0)
    return x * 12.92 if x <= 0.0031308 else 1.055 * x ** (1 / 2.4) - 0.055


def norm(v):
    l = math.sqrt(sum(c * c for c in v))
    return tuple(c / l for c in v)


KEY = norm((-0.45, 0.55, 0.7))
HALF = norm((KEY[0], KEY[1], KEY[2] + 1.0))


def shade(u: float, v: float, size: int):
    """RGBA (0-255 floats) of one sample; u, v in [0, 1) across the icon."""
    # Rounded-square background with soft edge.
    x, y = u * 2 - 1, v * 2 - 1
    r = 0.36
    qx, qy = max(abs(x) - (1 - r), 0.0), max(abs(y) - (1 - r), 0.0)
    d_box = math.hypot(qx, qy) - r
    if d_box > 0:
        return (0.0, 0.0, 0.0, 0.0)
    col = list(BG)

    # Clay sphere.
    cx, cy, rad = -0.06, -0.06, 0.66
    sx, sy = (x - cx) / rad, -(y - cy) / rad
    rr = sx * sx + sy * sy
    if rr <= 1.0:
        n = (sx, sy, math.sqrt(1 - rr))
        kd = max(sum(a * b for a, b in zip(n, KEY)), 0.0)
        ks = max(sum(a * b for a, b in zip(n, HALF)), 0.0) ** 24 * 0.35
        rim = (1 - n[2]) ** 3 * 0.25
        lin = [c * (0.12 + kd * 0.95) + ks + rim * 0.6 for c in CLAY]
        col = [srgb(c) * 255 for c in lin]

    # Brush ring (lower right), drawn over everything.
    bx, by, br = 0.42, 0.42, 0.34
    ring = abs(math.hypot(x - bx, y - by) - br)
    width = max(0.055, 2.2 / size)
    if ring < width:
        t = 1 - ring / width
        a = min(1.0, t * 2.0)
        col = [c * (1 - a) + acc * a for c, acc in zip(col, ACCENT)]
    if math.hypot(x - bx, y - by) < max(0.05, 1.5 / size):
        col = list(ACCENT)
    return (col[0], col[1], col[2], 255.0)


def render(size: int, ss: int = 4) -> bytes:
    """RGBA rows with ss×ss supersampling."""
    rows = bytearray()
    for j in range(size):
        rows.append(0)  # PNG filter: none
        for i in range(size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sj in range(ss):
                for si in range(ss):
                    u = (i + (si + 0.5) / ss) / size
                    v = (j + (sj + 0.5) / ss) / size
                    p = shade(u, v, size)
                    a = p[3] / 255.0
                    acc[0] += p[0] * a
                    acc[1] += p[1] * a
                    acc[2] += p[2] * a
                    acc[3] += p[3]
            n = ss * ss
            alpha = acc[3] / n
            if alpha > 0:
                k = 255.0 / alpha / n  # un-premultiply
                rows.extend(int(min(255, c * k + 0.5)) for c in acc[:3])
            else:
                rows.extend((0, 0, 0))
            rows.append(int(alpha + 0.5))
    return bytes(rows)


def png(size: int) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(render(size), 9)) + chunk(b"IEND", b"")


def ico(pngs: dict) -> bytes:
    """ICO container with PNG-compressed entries (supported since Windows Vista)."""
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries, data = b"", b""
    for s in sizes:
        blob = pngs[s]
        dim = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return header + entries + data


def main() -> None:
    pngs = {}
    for s in SIZES:
        pngs[s] = png(s)
        (OUT / f"oxybrush-{s}.png").write_bytes(pngs[s])
    (OUT / "oxybrush.ico").write_bytes(ico({s: pngs[s] for s in ICO_SIZES}))
    print("wrote", ", ".join(f"oxybrush-{s}.png" for s in SIZES), "and oxybrush.ico")


if __name__ == "__main__":
    main()
