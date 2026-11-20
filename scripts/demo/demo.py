"""Record the demo videos from the Markdown scripts in recordings/.

demo.py                     every recording → site/public/demo/<name>.{mp4,png}
demo.py stream ask          some recordings
demo.py --list              names and captions
demo.py --explore stream    4x speed, 0 video: PNG + screen text at each `shot` key
demo.py --jobs 3            recordings in parallel (default: 3)

Per recording: a fresh demo world (fixture.py), the fake harness turns and seeded threads of
its script, codehead in a pseudo terminal of 1920x1080 px (1 quarter of a 4K monitor), the
cast rendered to H.264.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import fixture
import script
from record import Driver
from term import Painter, Style

W, H = 1920, 1080
OUT = HERE.parents[1] / "site/public/demo"
CACHE = Path(os.path.expanduser("~/.cache/codehead-demo"))
PY = str(CACHE / "venv/bin/python")


def run(name: str, explore: bool = False):
    rec = script.parse_all()[name]
    root = CACHE / f"w-{name}"
    fixture.build(root, rec.script(), rec.threads())
    # frontmatter `font: 26` (px): fewer, larger cells (default 17 px: about 190 columns)
    painter = Painter(Style(font_px=int(rec.meta.get("font", 17))))
    cols, rows = painter.grid(W, H)
    shots = CACHE / "shots" / name
    cast = (shots if explore else CACHE / "casts") / f"{name}.cast"
    cast.parent.mkdir(parents=True, exist_ok=True)
    d = Driver(fixture.argv(root, "codehead"), fixture.env(root), "/", cols, rows, str(cast), speed=4.0 if explore else 1.0)
    n = [0]

    def shot(label):
        if not explore:
            return
        n[0] += 1
        painter.frame(d.screen, W, H).save(shots / f"{n[0]:02d}-{label}.png")
        print(f"===== {n[0]:02d} {label}\n" + "\n".join(l.rstrip() for l in d.screen.display).rstrip() + "\n", flush=True)

    d.shot = shot
    try:
        rec.drive(d)
        d.sleep(0.5)
    finally:
        shot("end")
        d.close()
    if explore:
        print(f"shots: {shots}")
        return
    OUT.mkdir(parents=True, exist_ok=True)
    r = subprocess.run([PY, str(HERE / "render.py"), str(cast), str(OUT / f"{name}.mp4"), f"{W}x{H}", "--start", "0.4",
                        "--font", str(painter.s.font_px)],
                       capture_output=True, text=True)
    print(f"{name}: {r.stdout.strip() or r.stderr.strip()[-400:]}", flush=True)


def job(name: str) -> str:
    r = subprocess.run([PY, "-u", str(HERE / "demo.py"), "--one", name], capture_output=True, text=True)
    return (r.stdout + r.stderr).strip()[-2000:]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("names", nargs="*")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--explore")
    ap.add_argument("--one")
    a = ap.parse_args()
    if a.list:
        for name, rec in script.parse_all().items():
            print(f"{name:10} {rec.caption()}")
        return
    if a.explore:
        return run(a.explore, explore=True)
    if a.one:
        try:
            return run(a.one)
        except Exception as e:  # noqa: BLE001
            print(f"{a.one}: FAILED {type(e).__name__}: {str(e)[:1500]}")
            sys.exit(1)
    names = a.names or list(script.parse_all())
    with ThreadPoolExecutor(a.jobs) as ex:
        for line in ex.map(job, names):
            print(line, flush=True)


if __name__ == "__main__":
    main()
