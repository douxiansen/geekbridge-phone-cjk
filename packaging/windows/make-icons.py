#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Draws the package tile icons: the app's dot-matrix keypad glyph (see crates/phone-app/src/icon.rs).

    python make-icons.py <output Assets directory>
"""
import struct
import sys
import zlib
from pathlib import Path


def png(width, height, rgba):
    raw = b"".join(b"\x00" + bytes(rgba[y * width * 4:(y + 1) * width * 4]) for y in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def icon(size):
    """A black rounded square with a 3x4 grid of dots; the bottom middle one is red."""
    s = size / 64.0
    buf = bytearray(size * size * 4)
    radius = 14.0 * s
    for y in range(size):
        for x in range(size):
            cx, cy = x + 0.5, y + 0.5
            dx = max(abs(cx - size / 2) - (size / 2 - radius), 0.0)
            dy = max(abs(cy - size / 2) - (size / 2 - radius), 0.0)
            if (dx * dx + dy * dy) ** 0.5 <= radius:
                i = (y * size + x) * 4
                buf[i:i + 4] = bytes((12, 12, 12, 255))
    for row in range(4):
        for col in range(3):
            px, py = (18.0 + col * 14.0) * s, (12.0 + row * 13.0) * s
            colour = (215, 25, 33, 255) if (row == 3 and col == 1) else (244, 244, 241, 255)
            r = 4.0 * s
            for y in range(max(int(py - r) - 1, 0), min(int(py + r) + 2, size)):
                for x in range(max(int(px - r) - 1, 0), min(int(px + r) + 2, size)):
                    if ((x + 0.5 - px) ** 2 + (y + 0.5 - py) ** 2) ** 0.5 <= r:
                        i = (y * size + x) * 4
                        buf[i:i + 4] = bytes(colour)
    return png(size, size, buf)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, size in {
        "StoreLogo.png": 50,
        "Square44x44Logo.png": 44,
        "Square150x150Logo.png": 150,
    }.items():
        (out / name).write_bytes(icon(size))


if __name__ == "__main__":
    main()
