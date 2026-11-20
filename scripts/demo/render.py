"""Render a recording (.cast) to an MP4 video and a poster PNG.

render.py <cast> <out.mp4> <width>x<height> [--fps 30] [--poster seconds]

The cast replays into a pyte screen; frames are drawn at FPS with the pointer overlay
(markers `pointer` and `click`). Frames that did not change are not drawn again.
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))
from PIL import Image, ImageDraw

from term import Painter, Screen, Stream, Style

ARROW = [(0, 0), (0, 17), (4, 13), (7, 20), (10, 19), (7, 12), (12, 12)]


def pointer_img(scale: float) -> Image.Image:
    pts = [(x * scale + 2, y * scale + 2) for x, y in ARROW]
    w = int(14 * scale + 6)
    h = int(22 * scale + 6)
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.polygon(pts, fill=(227, 210, 190, 255), outline=(204, 92, 16, 255), width=max(1, round(scale * 1.2)))
    return img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cast")
    ap.add_argument("out")
    ap.add_argument("size")
    ap.add_argument("--fps", type=int, default=30)
    ap.add_argument("--poster", type=float, default=-1.0, help="seconds into the video, -1: 2 s before the end")
    ap.add_argument("--start", type=float, default=0.0, help="skip the first SECONDS")
    ap.add_argument("--hold", type=float, default=1.5, help="hold the last frame SECONDS")
    ap.add_argument("--font", type=int, default=17)
    a = ap.parse_args()
    W, H = map(int, a.size.split("x"))

    lines = open(a.cast).read().splitlines()
    head = json.loads(lines[0])
    events = [json.loads(l) for l in lines[1:] if l.strip()]
    painter = Painter(Style(a.font))
    screen = Screen(head["width"], head["height"], lambda s: None)
    stream = Stream(screen)
    end = (events[-1][0] if events else 0) + a.hold
    total = max(1, int((end - a.start) * a.fps))
    arrow = pointer_img(painter.cw / 9)
    poster_at = a.poster if a.poster >= 0 else max(0.0, end - a.start - 2.0)

    ff = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(a.fps), "-i", "-",
         "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-tune", "animation", "-pix_fmt", "yuv420p",
         "-movflags", "+faststart", a.out],
        stdin=subprocess.PIPE,
    )
    i = 0
    pointer = None
    click_t = -9.0
    last_key = None
    raw = None
    poster_done = False
    for f in range(total):
        t = a.start + f / a.fps
        changed = False
        while i < len(events) and events[i][0] <= t:
            ts, kind, data = events[i]
            i += 1
            if kind == "o":
                stream.feed(data.encode())
                changed = True
            elif kind == "m":
                m = json.loads(data)
                if "pointer" in m:
                    pointer = m["pointer"]
                    changed = True
                if "click" in m:
                    pointer = m["click"]
                    click_t = ts
                    changed = True
        ripple = t - click_t < 0.35
        if changed or ripple or raw is None:
            img = painter.frame(screen, W, H)
            if pointer is not None:
                px = painter.s.pad + pointer[0] * painter.cw + painter.cw // 2
                py = painter.s.pad + pointer[1] * painter.ch + painter.ch // 2
                if ripple:
                    k = (t - click_t) / 0.35
                    r = painter.cw * (0.6 + 1.6 * k)
                    d = ImageDraw.Draw(img, "RGBA")
                    d.ellipse([px - r, py - r, px + r, py + r], outline=(236, 143, 26, int(220 * (1 - k))), width=max(2, painter.cw // 5))
                img.paste(arrow, (int(px) - 2, int(py) - 2), arrow)
            raw = img.tobytes()
            if not poster_done and t - a.start >= poster_at:
                img.save(os.path.splitext(a.out)[0] + ".png")
                poster_done = True
        ff.stdin.write(raw)
    if not poster_done:
        img.save(os.path.splitext(a.out)[0] + ".png")
    ff.stdin.close()
    ff.wait()
    print(f"{a.out}: {total / a.fps:.1f}s, {total} frames")


if __name__ == "__main__":
    main()
