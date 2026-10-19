"""Generate Tauri icons (icon.png + icon.ico) from scratch, stdlib only.

Icons are build artifacts, not committed: `npm run dev` / `npm run build`
run this automatically via the predev/prebuild hooks in package.json.
Run it manually if you build with cargo directly:

    python3 app/src-tauri/icons/generate_icons.py
"""
import math
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
W = H = 512


def pixel(x: int, y: int):
    cx, cy = W / 2, H / 2
    d = math.hypot(x - cx, y - cy)
    if d < 190:  # inner circle, warm paper
        return (244, 241, 234, 255)
    if d < 210:  # ring
        return (255, 253, 248, 255)
    return (180, 85, 45, 255)  # brand color


def chunk(typ: bytes, data: bytes) -> bytes:
    c = struct.pack(">I", len(data)) + typ + data
    c += struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF)
    return c


def main() -> None:
    rows = bytearray()
    for y in range(H):
        rows.append(0)  # filter: none
        for x in range(W):
            rows.extend(pixel(x, y))
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows)))
    png += chunk(b"IEND", b"")
    with open(os.path.join(HERE, "icon.png"), "wb") as f:
        f.write(png)
    # ICO wrapping the same PNG (valid for the Windows bundler).
    ico = struct.pack("<HHH", 0, 1, 1)
    ico += struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22)
    ico += png
    with open(os.path.join(HERE, "icon.ico"), "wb") as f:
        f.write(ico)
    print(f"wrote icon.png ({len(png)} bytes), icon.ico ({len(ico)} bytes)")


if __name__ == "__main__":
    main()
