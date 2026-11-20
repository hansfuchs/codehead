"""Run codehead in a pseudo terminal, drive it with scripted keys, record the output.

The recording is an asciicast v2 file (`.cast`): a header line, then `[t, "o", data]` per output
chunk and `[t, "m", json]` markers (pointer moves and clicks, for the pointer overlay).
A live pyte screen follows the output, so a script can wait for text on screen.
"""

from __future__ import annotations

import base64
import fcntl
import json
import os
import pty
import random
import re
import select
import signal
import struct
import termios
import time

import pyte

from term import Screen, Stream

# Keys as a kitty-protocol terminal (disambiguate mode) sends them.
KEYS = {
    "enter": "\r", "tab": "\t", "shift+tab": "\x1b[Z", "esc": "\x1b[27u", "backspace": "\x7f", "space": " ",
    "up": "\x1b[A", "down": "\x1b[B", "right": "\x1b[C", "left": "\x1b[D",
    "ctrl+left": "\x1b[1;5D", "ctrl+right": "\x1b[1;5C", "alt+left": "\x1b[1;3D", "alt+right": "\x1b[1;3C",
    "shift+up": "\x1b[1;2A", "shift+down": "\x1b[1;2B",
    "pgup": "\x1b[5~", "pgdn": "\x1b[6~", "home": "\x1b[H", "end": "\x1b[F",
    "ctrl+enter": "\x1b[13;5u", "shift+enter": "\x1b[13;2u", "alt+enter": "\x1b[13;3u",
    "f2": "\x1bOQ",
}
for ch in "abcdefghijklmnopqrstuvwxyz":
    KEYS[f"ctrl+{ch}"] = f"\x1b[{ord(ch)};5u"
KEYS["ctrl+c"] = "\x1b[99;5u"
# alt+<char> (alt+1-3: the thread views)
for ch in "0123456789abcdefghijklmnopqrstuvwxyz\\":
    KEYS[f"alt+{ch}"] = f"\x1b[{ord(ch)};3u"


class Driver:
    def __init__(self, argv: list[str], env: dict, cwd: str, cols: int, rows: int, cast: str, speed: float = 1.0):
        self.cols, self.rows = cols, rows
        self.speed = speed
        self.out = open(cast, "w")
        self.out.write(json.dumps({"version": 2, "width": cols, "height": rows, "timestamp": int(time.time()),
                                   "env": {"TERM": env.get("TERM", "xterm-256color")}}) + "\n")
        self.screen = Screen(cols, rows, self._reply)
        self.stream = Stream(self.screen)
        self.t0 = time.monotonic()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(cwd)
            os.execvpe(argv[0], argv, env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.pointer = None

    # ---------- io ----------
    def _reply(self, data: str):
        os.write(self.fd, data.encode())

    def t(self) -> float:
        return time.monotonic() - self.t0

    def pump(self, secs: float = 0.0):
        """Read output for SECS seconds (0: what is there now)."""
        end = time.monotonic() + secs
        while True:
            left = max(0.0, end - time.monotonic())
            r, _, _ = select.select([self.fd], [], [], left)
            if r:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                self.out.write(json.dumps([round(self.t(), 4), "o", data.decode("utf-8", "replace")]) + "\n")
                self.stream.feed(data)
            elif time.monotonic() >= end:
                return True

    def marker(self, **kw):
        self.out.write(json.dumps([round(self.t(), 4), "m", json.dumps(kw)]) + "\n")

    def send(self, data: str):
        os.write(self.fd, data.encode())
        self.pump(0.0)

    # ---------- screen ----------
    def text(self) -> str:
        return "\n".join(self.screen.display)

    def find(self, pattern: str):
        """(col, row) of the 1st match of regex PATTERN on screen, else None."""
        for y, line in enumerate(self.screen.display):
            m = re.search(pattern, line)
            if m:
                return m.start(), y
        return None

    def wait(self, pattern: str, timeout: float = 30.0, gone: bool = False):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            hit = self.find(pattern)
            if (hit is None) == gone:
                return hit
            self.pump(0.05)
        raise TimeoutError(f"wait {'gone ' if gone else ''}{pattern!r}\n{self.text()}")

    # ---------- input ----------
    def sleep(self, secs: float):
        self.pump(secs / self.speed)

    def key(self, name: str, n: int = 1, gap: float = 0.18):
        for _ in range(n):
            self.marker(key=name)
            self.send(KEYS[name])
            self.sleep(gap)

    def type(self, text: str, cps: float = 22.0):
        """Type like a person: CPS characters per second, with jitter and pauses after words."""
        for ch in text:
            if ch == "\n":
                self.send(KEYS["enter"])
            else:
                self.send(ch)
            d = random.uniform(0.5, 1.5) / cps
            if ch in " ,.:" and random.random() < 0.25:
                d += random.uniform(0.08, 0.3)
            self.sleep(d)

    def paste(self, text: str):
        self.send("\x1b[200~" + text + "\x1b[201~")

    def move(self, col: int, row: int, secs: float = 0.45):
        """Pointer glides to cell (COL, ROW): motion events + markers."""
        start = self.pointer or (col - 12, row + 6)
        steps = max(4, int(secs * 30))
        for i in range(1, steps + 1):
            k = i / steps
            k = 1 - (1 - k) ** 3
            x = start[0] + (col - start[0]) * k
            y = start[1] + (row - start[1]) * k
            self.marker(pointer=[round(x, 2), round(y, 2)])
            self.send(f"\x1b[<35;{int(x) + 1};{int(y) + 1}M")
            self.sleep(secs / steps)
        self.pointer = (col, row)

    def click(self, col: int, row: int):
        self.move(col, row)
        self.sleep(0.12)
        self.marker(click=[col, row])
        self.send(f"\x1b[<0;{col + 1};{row + 1}M")
        self.sleep(0.06)
        self.send(f"\x1b[<0;{col + 1};{row + 1}m")
        self.sleep(0.2)

    def click_on(self, pattern: str, dx: int = 0):
        col, row = self.wait(pattern)
        self.click(col + dx, row)

    def wheel(self, up: bool, n: int = 3):
        col, row = self.pointer or (self.cols // 2, self.rows // 2)
        for _ in range(n):
            self.send(f"\x1b[<{64 if up else 65};{col + 1};{row + 1}M")
            self.sleep(0.05)

    def close(self):
        try:
            os.kill(self.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        end = time.monotonic() + 2
        while time.monotonic() < end:
            try:
                if os.waitpid(self.pid, os.WNOHANG)[0]:
                    break
            except ChildProcessError:
                break
            time.sleep(0.05)
        else:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except (ProcessLookupError, ChildProcessError):
                pass
        os.close(self.fd)
        self.out.close()
