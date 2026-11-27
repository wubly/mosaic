#!/usr/bin/env python3
"""Generate the NSIS installer bitmaps (header.bmp, sidebar.bmp) from native/assets/icon.svg.

Pure standard library: the tiles and the play triangle are read from the SVG and rasterised
with 3x3 supersampling, then written as 24-bit BMPs (what NSIS wants).

    python native/packaging/windows/make_bitmaps.py

The page background colour here must match MUI_BGCOLOR in installer.nsi.
"""
import re
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent
SVG = HERE.parent.parent / "assets" / "icon.svg"

BG_TOP = (0x1E, 0x1B, 0x24)
BG = (0x14, 0x12, 0x18)  # == MUI_BGCOLOR 141218
SS = 3  # supersampling per axis


def parse_svg():
    svg = SVG.read_text()
    tiles = []
    for m in re.finditer(r"<rect ([^>]*)/>", svg):
        a = dict(re.findall(r'([\w-]+)="([^"]*)"', m.group(1)))
        if "x" not in a:  # the background tile has no x/y
            continue
        c = a["fill"].lstrip("#")
        tiles.append((float(a["x"]), float(a["y"]), float(a["width"]), float(a["rx"]),
                      tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))))
    tri = re.search(r'<path d="M([\d.]+) ([\d.]+)L([\d.]+) ([\d.]+)L([\d.]+) ([\d.]+)Z" fill="#(\w+)"', svg)
    nums = [float(tri.group(i)) for i in range(1, 7)]
    c = tri.group(7)
    tri = ([(nums[0], nums[1]), (nums[2], nums[3]), (nums[4], nums[5])],
           tuple(int(c[i:i + 2], 16) for i in (0, 2, 4)))
    return tiles, tri


def in_round_rect(px, py, x, y, s, r):
    if not (x <= px <= x + s and y <= py <= y + s):
        return False
    cx = min(max(px, x + r), x + s - r)
    cy = min(max(py, y + r), y + s - r)
    return (px - cx) ** 2 + (py - cy) ** 2 <= r * r


def in_tri(px, py, t):
    (x1, y1), (x2, y2), (x3, y3) = t
    d1 = (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2)
    d2 = (px - x3) * (y2 - y3) - (x2 - x3) * (py - y3)
    d3 = (px - x1) * (y3 - y1) - (x3 - x1) * (py - y1)
    neg = d1 < 0 or d2 < 0 or d3 < 0
    pos = d1 > 0 or d2 > 0 or d3 > 0
    return not (neg and pos)


def lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def render(w, h, logo_box, tiles, tri, gradient, fade=1.0):
    """logo_box = (left, top, size): where the 512-unit icon grid goes (icon coords 84..428)."""
    left, top, size = logo_box
    k = size / 344.0  # grid spans 84..428
    rows = []
    for py in range(h):
        row = []
        base = lerp(BG_TOP, BG, py / (h - 1)) if gradient else BG
        for pxl in range(w):
            acc = [0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    fx = pxl + (sx + 0.5) / SS
                    fy = py + (sy + 0.5) / SS
                    ix = (fx - left) / k + 84
                    iy = (fy - top) / k + 84
                    col = base
                    for (x, y, s, r, c) in tiles:
                        if in_round_rect(ix, iy, x, y, s, r):
                            col = lerp(base, c, fade)
                            break
                    if in_tri(ix, iy, tri[0]):
                        col = lerp(base, tri[1], fade)
                    for i in range(3):
                        acc[i] += col[i]
            row.append(tuple(v // (SS * SS) for v in acc))
        rows.append(row)
    return rows


def write_bmp(path, rows):
    h, w = len(rows), len(rows[0])
    pad = (4 - (w * 3) % 4) % 4
    data = bytearray()
    for row in reversed(rows):
        for r, g, b in row:
            data += bytes((b, g, r))
        data += b"\0" * pad
    hdr = b"BM" + struct.pack("<IHHI", 54 + len(data), 0, 0, 54)
    dib = struct.pack("<IiiHHIIiiII", 40, w, h, 1, 24, 0, len(data), 2835, 2835, 0, 0)
    path.write_bytes(hdr + dib + bytes(data))
    print(f"wrote {path.name} ({w}x{h})")


def main():
    tiles, tri = parse_svg()
    # Header: 150x57, logo at the right edge on the page background.
    write_bmp(HERE / "header.bmp", render(150, 57, (92, 5, 47), tiles, tri, gradient=False))
    # Welcome/finish sidebar: 164x314, large logo near the top, vertical gradient.
    write_bmp(HERE / "sidebar.bmp", render(164, 314, (22, 52, 120), tiles, tri, gradient=True))


if __name__ == "__main__":
    main()
