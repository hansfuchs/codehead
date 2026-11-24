"""Terminal emulation (pyte) and frame rendering (Pillow) for the demo videos.

The screen state comes from pyte; every cell is drawn in JetBrains Mono. Box drawing, block
elements and braille are drawn as shapes (like Ghostty does), so sprites and frames line up
pixel for pixel. Glyphs JetBrains Mono lacks come from fallback fonts, scaled to the cell.
"""

from __future__ import annotations

import functools
import os
from dataclasses import dataclass

import pyte
from PIL import Image, ImageDraw, ImageFont

FONT_DIR = os.path.expanduser("~/.cache/codehead-demo/jbm/fonts/ttf")
FALLBACKS = [
    # Nerd Font icons (yazi, eza, starship …): private-use code points JetBrains Mono lacks
    "/usr/share/fonts/TTF/SymbolsNerdFontMono-Regular.ttf",
    "/usr/share/fonts/TTF/SymbolsNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf",
    "/usr/share/fonts/noto/NotoSansMath-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
]

# Default colors of the recording terminal: the paper and ink of codehead.sh.
BG = (20, 7, 0)
FG = (232, 228, 246)
# 16 ANSI colors: Ghostty's default palette.
ANSI = [
    (0x1D, 0x1F, 0x21), (0xCC, 0x66, 0x66), (0xB5, 0xBD, 0x68), (0xF0, 0xC6, 0x74),
    (0x81, 0xA2, 0xBE), (0xB2, 0x94, 0xBB), (0x8A, 0xBE, 0xB7), (0xC5, 0xC8, 0xC6),
    (0x66, 0x66, 0x66), (0xD5, 0x4E, 0x53), (0xB9, 0xCA, 0x4A), (0xE7, 0xC5, 0x47),
    (0x7A, 0xA6, 0xDA), (0xC3, 0x97, 0xD8), (0x70, 0xC0, 0xB1), (0xEA, 0xEA, 0xEA),
]
NAMES = ["black", "red", "green", "brown", "blue", "magenta", "cyan", "white"]

# indexed colors 0-15 (`38;5;n`): the same palette, not pyte's xterm defaults
import pyte.graphics as _g

for _i, (_r, _gg, _b) in enumerate(ANSI):
    _g.FG_BG_256[_i] = f"{_r:02x}{_gg:02x}{_b:02x}"


def color(c: str, default: tuple[int, int, int], bold: bool = False) -> tuple[int, int, int]:
    if c == "default":
        return default
    if c in NAMES:
        i = NAMES.index(c)
        return ANSI[i + 8 if bold else i]
    if c.startswith("bright") and c[6:] in NAMES:
        return ANSI[NAMES.index(c[6:]) + 8]
    if c in ("brightbrown", "yellow"):
        return ANSI[11]
    try:
        return (int(c[0:2], 16), int(c[2:4], 16), int(c[4:6], 16))
    except ValueError:
        return default


def osc4_reply(i: int) -> str:
    r, g, b = ANSI[i]
    return f"\x1b]4;{i};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x1b\\"


class Screen(pyte.Screen):
    """pyte screen that answers the queries codehead sends (palette, DA1, cursor, kitty)."""

    def __init__(self, cols: int, rows: int, reply):
        super().__init__(cols, rows)
        self.reply = reply

    def report_device_attributes(self, mode=0, **kw):
        if not kw.get("private"):
            self.reply("\x1b[?62;22c")

    def report_device_status(self, mode, **kw):
        if mode == 6:
            self.reply(f"\x1b[{self.cursor.y + 1};{self.cursor.x + 1}R")
        elif mode == 5:
            self.reply("\x1b[0n")

    def set_title(self, param):
        pass

    def write_process_input(self, data):
        self.reply(data)


class Stream(pyte.ByteStream):
    """ByteStream that also answers OSC 4 / OSC 10 / OSC 11 color queries and the kitty
    keyboard query (`CSI ? u`), which pyte ignores."""

    def __init__(self, screen: Screen):
        super().__init__(screen)
        self.tail = b""

    def feed(self, data: bytes) -> None:
        buf = self.tail + data
        self.tail = buf[-64:]
        i = 0
        while True:
            j = data.find(b"\x1b]4;", i)
            if j < 0:
                break
            k = data.find(b";", j + 4)
            if k > 0 and data[k + 1 : k + 2] == b"?":
                try:
                    self.listener.reply(osc4_reply(int(data[j + 4 : k])))
                except ValueError:
                    pass
            i = j + 4
        for code, rgb in ((b"10", FG), (b"11", BG)):
            if b"\x1b]" + code + b";?" in data:
                r, g, b = rgb
                self.listener.reply(f"\x1b]{code.decode()};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x1b\\")
        if b"\x1b[?u" in data:
            self.listener.reply("\x1b[?1u")
        # kitty push / pop (`CSI > 1 u`, `CSI < u`): pyte would print them
        for seq in (b"\x1b[>1u", b"\x1b[<u", b"\x1b[<1u"):
            data = data.replace(seq, b"")
        super().feed(data)


@dataclass
class Style:
    font_px: int = 17
    pad: int = 6


class Painter:
    """Cells → pixels. Row images are cached by their content."""

    def __init__(self, style: Style):
        self.s = style
        reg = os.path.join(FONT_DIR, "JetBrainsMono-Regular.ttf")
        self.font = ImageFont.truetype(reg, style.font_px)
        self.bold = ImageFont.truetype(os.path.join(FONT_DIR, "JetBrainsMono-Bold.ttf"), style.font_px)
        self.italic = ImageFont.truetype(os.path.join(FONT_DIR, "JetBrainsMono-Italic.ttf"), style.font_px)
        self.cw = round(self.font.getlength("M"))
        self.ch = round(style.font_px * 1.32)
        self.base = round(style.font_px * 1.02 + (self.ch - style.font_px * 1.32) / 2)
        self.falls = [ImageFont.truetype(p, style.font_px) for p in FALLBACKS if os.path.exists(p)]
        self.cmap = self._cmap(reg)
        self.rows: dict = {}

    @staticmethod
    def _cmap(path):
        from fontTools.ttLib import TTFont

        return set(TTFont(path).getBestCmap())

    def grid(self, width: int, height: int) -> tuple[int, int]:
        return (width - 2 * self.s.pad) // self.cw, (height - 2 * self.s.pad) // self.ch

    @functools.lru_cache(maxsize=None)
    def fallback(self, ch: str):
        for f, p in zip(self.falls, [p for p in FALLBACKS if os.path.exists(p)]):
            if ord(ch) in _cmap_of(p):
                return f
        return self.falls[0] if self.falls else self.font

    def shape(self, d: ImageDraw.ImageDraw, ch: str, x: int, y: int, fg) -> bool:
        """Box drawing, blocks and braille as shapes. False: not a shape glyph."""
        o = ord(ch)
        w, h = self.cw, self.ch
        cx, cy = x + w // 2, y + h // 2
        t = max(1, round(self.s.font_px / 15))  # light line
        T = t * 2  # heavy line
        if 0x2800 <= o <= 0x28FF:
            bits = o - 0x2800
            # dot order: 1 2 3 7 (left column), 4 5 6 8 (right column)
            pos = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)]
            r = max(2, round(w / 4.2))
            for i, (c, rr) in enumerate(pos):
                if bits >> i & 1:
                    px = x + round(w * (0.27 + 0.46 * c)) - r // 2
                    py = y + round(h * (0.12 + 0.25 * rr)) - r // 2
                    d.rectangle([px, py, px + r - 1, py + r - 1], fill=fg)
            return True
        if 0x2580 <= o <= 0x259F:
            def rect(x0, y0, x1, y1):
                d.rectangle([x + round(w * x0), y + round(h * y0), x + round(w * x1) - 1, y + round(h * y1) - 1], fill=fg)
            if o == 0x2580: rect(0, 0, 1, 0.5)
            elif 0x2581 <= o <= 0x2588: rect(0, 1 - (o - 0x2580) / 8, 1, 1)
            elif 0x2589 <= o <= 0x258F: rect(0, 0, (0x2590 - o) / 8, 1)
            elif o == 0x2590: rect(0.5, 0, 1, 1)
            elif o in (0x2591, 0x2592, 0x2593):
                a = {0x2591: 0.25, 0x2592: 0.5, 0x2593: 0.75}[o]
                step = 2
                for yy in range(0, h, step):
                    for xx in range(0, w, step):
                        k = ((xx // step) + (yy // step)) % 4
                        if (k / 4) < a or (a == 0.5 and ((xx // step) + (yy // step)) % 2 == 0):
                            d.rectangle([x + xx, y + yy, x + xx + step - 1, y + yy + step - 1], fill=fg)
            elif o == 0x2594: rect(0, 0, 1, 1 / 8)
            elif o == 0x2595: rect(7 / 8, 0, 1, 1)
            else:
                quads = {0x2596: "3", 0x2597: "4", 0x2598: "1", 0x2599: "134", 0x259A: "14", 0x259B: "123",
                         0x259C: "124", 0x259D: "2", 0x259E: "23", 0x259F: "234"}[o]
                for q in quads:
                    qx, qy = {"1": (0, 0), "2": (0.5, 0), "3": (0, 0.5), "4": (0.5, 0.5)}[q]
                    rect(qx, qy, qx + 0.5, qy + 0.5)
            return True
        if not (0x2500 <= o <= 0x257F):
            return False

        def hl(x0, x1, th):
            if x1 < x0:
                return
            d.rectangle([x0, cy - th // 2, x1, cy - th // 2 + th - 1], fill=fg)

        def vl(y0, y1, th):
            if y1 < y0:
                return
            d.rectangle([cx - th // 2, y0, cx - th // 2 + th - 1, y1], fill=fg)

        L, R, U, D = x, x + w - 1, y, y + h - 1
        arms = BOX.get(ch)
        if arms:
            l, r, u, dn = arms
            if l: hl(L, cx, T if l == 2 else t)
            if r: hl(cx, R, T if r == 2 else t)
            if u: vl(U, cy, T if u == 2 else t)
            if dn: vl(cy, D, T if dn == 2 else t)
            return True
        if ch in "╭╮╰╯":
            rad = w // 2
            box = {"╭": (cx, cy, cx + 2 * rad, cy + 2 * rad, 180, 270, "r", "d"),
                   "╮": (cx - 2 * rad, cy, cx, cy + 2 * rad, 270, 360, "l", "d"),
                   "╰": (cx, cy - 2 * rad, cx + 2 * rad, cy, 90, 180, "r", "u"),
                   "╯": (cx - 2 * rad, cy - 2 * rad, cx, cy, 0, 90, "l", "u")}[ch]
            x0, y0, x1, y1, a0, a1, hh, vv = box
            off = t // 2
            d.arc([x0 - off, y0 - off, x1 - off + t - 1, y1 - off + t - 1], a0, a1, fill=fg, width=t)
            if hh == "r": hl(cx + rad, R, t)
            else: hl(L, cx - rad, t)
            if vv == "d": vl(cy + rad, D, t)
            else: vl(U, cy - rad, t)
            return True
        dash = {"┄": (3, "h", t), "┅": (3, "h", T), "┈": (4, "h", t), "┉": (4, "h", T), "╌": (2, "h", t), "╍": (2, "h", T),
                "┆": (3, "v", t), "┇": (3, "v", T), "┊": (4, "v", t), "┋": (4, "v", T), "╎": (2, "v", t), "╏": (2, "v", T)}.get(ch)
        if dash:
            n, axis, th = dash
            if axis == "h":
                seg = w / n
                for i in range(n):
                    d.rectangle([x + round(i * seg + seg * 0.15), cy - th // 2, x + round((i + 1) * seg - seg * 0.15) - 1, cy - th // 2 + th - 1], fill=fg)
            else:
                seg = h / n
                for i in range(n):
                    d.rectangle([cx - th // 2, y + round(i * seg + seg * 0.15), cx - th // 2 + th - 1, y + round((i + 1) * seg - seg * 0.15) - 1], fill=fg)
            return True
        if ch in "═║╔╗╚╝╠╣╦╩╬":
            g = max(2, t * 2)
            def h2(x0, x1):
                d.rectangle([x0, cy - g, x1, cy - g + t - 1], fill=fg); d.rectangle([x0, cy + g - t + 1, x1, cy + g], fill=fg)
            def v2(y0, y1):
                d.rectangle([cx - g, y0, cx - g + t - 1, y1], fill=fg); d.rectangle([cx + g - t + 1, y0, cx + g, y1], fill=fg)
            if ch == "═": h2(L, R)
            elif ch == "║": v2(U, D)
            else:
                if ch in "╗╝╣╦╩╬": h2(L, cx)
                if ch in "╔╚╠╦╩╬": h2(cx, R)
                if ch in "╚╝╠╣╩╬": v2(U, cy)
                if ch in "╔╗╠╣╦╬": v2(cy, D)
            return True
        return False

    def row(self, line: list) -> Image.Image:
        key = tuple(line)
        img = self.rows.get(key)
        if img is not None:
            return img
        w, h = self.cw, self.ch
        img = Image.new("RGB", (w * len(line), h), BG)
        d = ImageDraw.Draw(img)
        for i, (ch, fg, bg, bold, italics, underscore, reverse, strike) in enumerate(line):
            f = color(fg, FG, bold)
            b = color(bg, BG)
            if reverse:
                f, b = b, f
            x = i * w
            if b != BG:
                d.rectangle([x, 0, x + w - 1, h - 1], fill=b)
            if ch and ch != " ":
                if not self.shape(d, ch, x, 0, f):
                    if ord(ch[0]) in self.cmap:
                        font = self.bold if bold else self.italic if italics else self.font
                        d.text((x, self.base), ch, font=font, fill=f, anchor="ls")
                    else:
                        self.fallback_text(img, d, ch, x, f)
            if underscore:
                uy = self.base + max(2, h // 10)
                d.rectangle([x, uy, x + w - 1, uy + max(0, h // 22)], fill=f)
            if strike:
                d.rectangle([x, h // 2, x + w - 1, h // 2], fill=f)
        if len(self.rows) > 20000:
            self.rows.clear()
        self.rows[key] = img
        return img

    def fallback_text(self, img, d, ch, x, fg):
        font = self.fallback(ch)
        bbox = font.getbbox(ch, anchor="ls")
        gw = bbox[2] - bbox[0]
        if gw <= self.cw * 1.05:
            d.text((x + (self.cw - gw) / 2 - bbox[0], self.base), ch, font=font, fill=fg, anchor="ls")
            return
        # too wide for 1 cell: scale it down
        scale = self.cw / gw
        tmp = Image.new("L", (max(1, gw), self.ch * 2), 0)
        ImageDraw.Draw(tmp).text((-bbox[0], self.base + self.ch // 2), ch, font=font, fill=255, anchor="ls")
        small = tmp.resize((self.cw, max(1, round(tmp.height * scale))), Image.LANCZOS)
        y = self.base - round((self.base + self.ch // 2) * scale)
        img.paste(Image.new("RGB", small.size, fg), (x, y), small)

    def frame(self, screen: pyte.Screen, width: int, height: int, cursor: bool = True) -> Image.Image:
        img = Image.new("RGB", (width, height), BG)
        for y in range(screen.lines):
            src = screen.buffer[y]
            line = []
            for x in range(screen.columns):
                c = src[x]
                line.append((c.data, c.fg, c.bg, c.bold, c.italics, c.underscore, c.reverse, c.strikethrough))
            img.paste(self.row(line), (self.s.pad, self.s.pad + y * self.ch))
        if cursor and not screen.cursor.hidden:
            cx = self.s.pad + screen.cursor.x * self.cw
            cy = self.s.pad + screen.cursor.y * self.ch
            ImageDraw.Draw(img).rectangle([cx, cy, cx + max(1, self.cw // 6), cy + self.ch - 1], fill=FG)
        return img


@functools.lru_cache(maxsize=None)
def _cmap_of(path):
    from fontTools.ttLib import TTFont

    return set(TTFont(path).getBestCmap())


# Box drawing arms: (left, right, up, down), 1 light, 2 heavy.
BOX = {}
for chars, arms in [
    ("─", (1, 1, 0, 0)), ("━", (2, 2, 0, 0)), ("│", (0, 0, 1, 1)), ("┃", (0, 0, 2, 2)),
    ("┌", (0, 1, 0, 1)), ("┐", (1, 0, 0, 1)), ("└", (0, 1, 1, 0)), ("┘", (1, 0, 1, 0)),
    ("┏", (0, 2, 0, 2)), ("┓", (2, 0, 0, 2)), ("┗", (0, 2, 2, 0)), ("┛", (2, 0, 2, 0)),
    ("├", (0, 1, 1, 1)), ("┤", (1, 0, 1, 1)), ("┬", (1, 1, 0, 1)), ("┴", (1, 1, 1, 0)), ("┼", (1, 1, 1, 1)),
    ("┣", (0, 2, 2, 2)), ("┫", (2, 0, 2, 2)), ("┳", (2, 2, 0, 2)), ("┻", (2, 2, 2, 0)), ("╋", (2, 2, 2, 2)),
    ("╴", (1, 0, 0, 0)), ("╶", (0, 1, 0, 0)), ("╵", (0, 0, 1, 0)), ("╷", (0, 0, 0, 1)),
    ("╸", (2, 0, 0, 0)), ("╺", (0, 2, 0, 0)), ("╹", (0, 0, 2, 0)), ("╻", (0, 0, 0, 2)),
    ("┍", (0, 2, 0, 1)), ("┑", (2, 0, 0, 1)), ("┕", (0, 2, 1, 0)), ("┙", (2, 0, 1, 0)),
    ("┝", (0, 2, 1, 1)), ("┥", (2, 0, 1, 1)), ("┯", (2, 2, 0, 1)), ("┷", (2, 2, 1, 0)),
]:
    BOX[chars] = arms
