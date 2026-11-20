#!/usr/bin/env python3
"""The favicon: the website's monolith (site/public/monolith.json) at an angle, its top with
the coral eyes, cropped 1:1, as an SVG of dots on the paper color.

favicon.py [frame] [pattern] [step] > site/public/favicon.svg
  frame:   turn step of the monolith (0..20, default 15)
  pattern: eye pattern (default 3: cross 4 rows, the site's start pattern)
  step:    keep every Nth dot per axis (default 1: every dot)
"""

import json
import sys
from pathlib import Path

DATA = Path(__file__).resolve().parents[1] / "site/public/monolith.json"
BG, INK, CORAL = "#150700", (0xE3, 0xD2, 0xBE), "#FF7565"
# tone digit → ink share (global.css .s0-.s5)
TONE = {"0": 0.22, "1": 0.34, "2": 0.48, "3": 0.64, "4": 0.82, "5": 1.0}
# braille dot bit → (dx, dy) in the 2x4 cell
BITS = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)]


def ink(share: float) -> str:
    bg = (0x15, 0x07, 0x00)
    r, g, b = (round(bg[i] + (INK[i] - bg[i]) * share) for i in range(3))
    return f"#{r:02x}{g:02x}{b:02x}"


def main():
    frame = int(sys.argv[1]) if len(sys.argv) > 1 else 15
    pat = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    step = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    d = json.loads(DATA.read_text())
    cols = d["cols"]
    p = d["patterns"][pat]
    f = p["frames"][frame]
    rows = f["c"].split("\n")
    tones = f["t"].split("\n")
    dots = {}  # (x, y) → color
    for y, (row, trow) in enumerate(zip(rows, tones)):
        for x, ch in enumerate(row):
            c = ord(ch) - 0x2800
            if not 0 <= c < 256:
                continue
            col = ink(TONE.get(trow[x] if x < len(trow) else "5", 1.0))
            for bit, (dx, dy) in enumerate(BITS):
                if c >> bit & 1:
                    dots[(x * 2 + dx, y * 4 + dy)] = col
    # every eye dot lit: the dots each eye glyph adds turn coral
    for eye in f["e"]:
        for cell, ch in eye:
            x, y = cell % cols, cell // cols
            c = ord(ch) - 0x2800
            a = ord(rows[y][x]) - 0x2800 if x < len(rows[y]) else 0
            if not 0 <= c < 256 or not 0 <= a < 256:
                continue
            # the socket's own dots stay stone (monolith.ts: overlay = b & ~a)
            for bit, (dx, dy) in enumerate(BITS):
                if (c & ~a) >> bit & 1:
                    dots[(x * 2 + dx, y * 4 + dy)] = CORAL
    xs = [x for x, _ in dots]
    ys = [y for _, y in dots]
    x0, x1, y0 = min(xs), max(xs), min(ys)
    # 1:1: the stone's full width, from its top down by the same length
    size = x1 - x0 + 1
    pad = max(2, size // 14)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size + 2 * pad} {size + 2 * pad}">',
           f'<rect width="100%" height="100%" fill="{BG}"/>']
    # small stone dots (the screenshot's look), larger eye dots: the eyes read at 16 px
    for (x, y), col in sorted(dots.items(), key=lambda kv: kv[1] == CORAL):
        if y - y0 >= size or (x - x0) % step or (y - y0) % step:
            continue
        r = (0.62 if col == CORAL else 0.34) * step
        out.append(f'<circle cx="{x - x0 + pad + 0.5}" cy="{y - y0 + pad + 0.5}" r="{r:.2f}" fill="{col}"/>')
    out.append("</svg>")
    print("".join(out))


if __name__ == "__main__":
    main()
