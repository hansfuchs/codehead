"""Recording scripts: 1 Markdown file per video in recordings/. Format: recordings/README.md.

parse_all() reads every file; a turn or message with `name X` can be reused in any file with
`use X`. Recording.script(): the fake harness turns, Recording.threads(): the seeded threads,
Recording.drive(d): the keys.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass, field
from pathlib import Path

import lib

DIR = Path(__file__).resolve().parent / "recordings"
FENCE = re.compile(r"^(```+|~~~+)(.*)$")
MODELS = {("claude", "opus"): "claude-opus-5-5", ("claude", "sonnet"): "claude-sonnet-5-5", ("claude", "haiku"): "claude-haiku-5-5"}


@dataclass
class Block:
    """1 heading (level, text) and the lines below it up to the next heading."""
    level: int
    head: str
    lines: list[str] = field(default_factory=list)


def blocks(text: str) -> list[Block]:
    out: list[Block] = [Block(0, "")]
    fence = None
    for line in text.split("\n"):
        m = FENCE.match(line)
        if m and (fence is None or m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) and not m.group(2).strip()):
            fence = None if fence else m.group(1)
        if fence is None and not m and (h := re.match(r"^(#{1,4}) (.*)$", line)):
            out.append(Block(len(h.group(1)), h.group(2).strip()))
        else:
            out[-1].lines.append(line)
    return out


def attrs(head: str) -> tuple[str, list[str]]:
    """`kind rest · a · b` → (`kind rest`, [a, b])."""
    parts = [p.strip() for p in head.split(" · ")]
    return parts[0], parts[1:]


def body(lines: list[str]) -> str:
    return "\n".join(lines).strip("\n").strip()


def fences(lines: list[str]) -> list[tuple[str, str]]:
    """Fenced blocks of LINES: (info string, content)."""
    out, cur, info, fence = [], [], "", None
    for line in lines:
        m = FENCE.match(line)
        if fence is None and m:
            fence, info, cur = m.group(1), m.group(2).strip(), []
        elif fence is not None and m and line.strip() == fence:
            out.append((info, "\n".join(cur)))
            fence = None
        elif fence is not None:
            cur.append(line)
    return out


def ms_of(opts: list[str], default: int) -> int:
    for o in opts:
        if m := re.fullmatch(r"([\d.]+)\s*(ms|s)", o):
            return int(float(m.group(1)) * (1 if m.group(2) == "ms" else 1000))
    return default


def step(b: Block, subs: list[Block]) -> dict:
    head, opts = attrs(b.head)
    kind, _, rest = head.partition(" ")
    rest = rest.strip().strip("`")
    fs = fences(b.lines)
    text = body(b.lines)
    if kind == "think":
        return lib.think(text, ms=ms_of(opts, 700))
    if kind == "say":
        return lib.say(text)
    if kind == "read":
        return lib.read(rest, fs[0][1], ms=ms_of(opts, 350))
    if kind == "grep":
        path = next((o[3:] for o in opts if o.startswith("in ")), None)
        return lib.grep(rest, fs[0][1] if fs else "", path, ms=ms_of(opts, 300))
    if kind == "bash":
        cmd = next(c for i, c in fs if i in ("sh", "bash", "shell"))
        out = next((c for i, c in fs if i not in ("sh", "bash", "shell")), "")
        return lib.bash(cmd, out, rest, ms=ms_of(opts, 1200), ok="fails" not in opts, ask="asks" in opts)
    if kind == "edit":
        f = dict(fs)
        return lib.edit(rest, f["old"], f["new"], ms=ms_of(opts, 250))
    if kind == "write":
        return lib.write(rest, fs[0][1] + "\n", ms=ms_of(opts, 250))
    if kind == "send":
        return lib.send([n.strip() for n in rest.split(",")], text)
    if kind == "status":
        return lib.status(rest, text)
    if kind == "fail":
        return {"fail": text}
    if kind == "sleep":
        return {"sleep": float(rest)}
    if kind == "question":
        qs = []
        for s in subs:
            qtext, qopts = attrs(s.head)
            options = []
            for line in s.lines:
                if m := re.match(r"^- (.+?)(?::\s+(.*))?$", line.strip()):
                    options.append((m.group(1), m.group(2) or ""))
            qs.append(lib.q(qtext, qopts[0] if qopts else "", options, multi="multi" in qopts))
        return lib.question(qs, ms=ms_of(opts, 400))
    raise ValueError(f"unknown step: ### {b.head}")


@dataclass
class Turn:
    """An agent turn (`## EVA`), an owner message (`## owner`), or an agent message
    (`## HAL → EVA`)."""
    who: str
    to: list[str]
    opts: dict
    steps: list[dict] = field(default_factory=list)
    text: str = ""


def turn_opts(opts: list[str]) -> dict:
    d = {}
    for o in opts:
        k, _, v = o.partition(" ")
        d[k] = v.strip()
    return d


def turns(bs: list[Block]) -> list[Turn]:
    out: list[Turn] = []
    i = 0
    while i < len(bs):
        b = bs[i]
        if b.level == 2:
            head, opts = attrs(b.head)
            who, _, to = head.partition("→")
            t = Turn(who.strip(), [n.strip() for n in to.split(",") if n.strip()], turn_opts(opts))
            t.text = body(b.lines)
            out.append(t)
        elif b.level == 3:
            subs = []
            while i + 1 < len(bs) and bs[i + 1].level == 4:
                i += 1
                subs.append(bs[i])
            out[-1].steps.append(step(b, subs))
        i += 1
    return out


@dataclass
class Recording:
    name: str
    meta: dict
    seeds: list[tuple[dict, list[Turn]]]
    live: list[Turn]
    keys: list[list[str]]
    defs: list[Turn] = field(default_factory=list)

    def caption(self) -> str:
        return self.meta.get("caption", "")

    def script(self) -> dict:
        out: dict = {}
        ctx = dict(p.split("=") for p in self.meta.get("ctx", "").split(",") if "=" in p)
        for t in self.live:
            me = out.setdefault(t.who, {"model": model_of(t.who), "ctx": int(ctx.get(t.who, 30000)), "turns": []})
            turn = {"steps": t.steps, "delay": float(t.opts.get("delay", 1.0))}
            if "when" in t.opts:
                turn["match"] = t.opts["when"]
            if "cost" in t.opts:
                turn["usage"] = {"cost": float(t.opts["cost"])}
            me["turns"].append(turn)
        return out

    def threads(self) -> list[dict]:
        out = []
        for head, ts in self.seeds:
            ev, task, pending = [], "", None
            for t in ts:
                if not t.steps and (t.who == "owner" or t.to):
                    if pending:
                        ev += [message(*pending)]
                    pending = (t.who, t.to, t.text)
                    task = task or (t.text if t.who == "owner" else "")
                    continue
                prompt, frm, to = (pending[2], pending[0], pending[1] or [t.who]) if pending else (None, "owner", None)
                ev += lib.events(t.who, t.steps, model=model_of(t.who), cost=float(t.opts.get("cost", 0.3)), prompt=prompt, frm=frm, to=to)
                pending = None
            if pending:
                ev.append(message(*pending))
            th = {"id": head["id"], "title": head["title"], "task": task or head["title"], "agents": head["agents"],
                  "age": head.get("age", 1800), "span": head.get("span", 600), "events": ev}
            for k in ("branch", "worktree", "state"):
                if k in head:
                    th[k] = head[k]
            out.append(th)
        return out

    def drive(self, d):
        for k in self.keys:
            op, args = k[0], k[1:]
            if op == "wait":
                d.wait(args[0], timeout=float(args[1]) if len(args) > 1 else 30)
            elif op == "sleep":
                d.sleep(float(args[0]))
            elif op == "type":
                d.type(args[0].replace("\\n", "\n"))
            elif op == "key":
                d.key(args[0], int(args[1]) if len(args) > 1 else 1, gap=float(args[2]) if len(args) > 2 else 0.18)
            elif op == "click":
                d.click_on(args[0], dx=int(args[1]) if len(args) > 1 else 0)
            elif op == "wheel":
                d.wheel(args[0] == "up", int(args[1]) if len(args) > 1 else 3)
            elif op == "shot":
                d.shot(args[0] if args else "shot")
            else:
                raise ValueError(f"unknown key op: {' '.join(k)}")


def message(frm, to, text):
    return (None if frm == "owner" else frm, "prompt", {"from": frm, "to": to or [], "text": text, "_dt": 1000})


def model_of(agent: str) -> str:
    import fixture

    a = fixture.AGENTS.get(agent, {})
    return MODELS.get((a.get("harness"), a.get("model")), a.get("model", ""))


def seed_head(head: str, lines: list[str]) -> dict:
    """`# seed 7c2e91 · title` + `agents EVA, HAL · branch x · age 1800` line."""
    first, opts = attrs(head)
    h = {"id": first.split()[1], "title": opts[0] if opts else first}
    line = next((l for l in lines if l.strip()), "")
    for part in line.split(" · "):
        k, _, v = part.strip().partition(" ")
        if k == "agents":
            h["agents"] = [n.strip() for n in v.split(",")]
        elif k in ("age", "span"):
            h[k] = int(float(v))
        elif k in ("branch", "worktree", "state"):
            h[k] = v.strip()
    return h


def parse(path: Path, named: dict | None = None) -> Recording:
    text = path.read_text()
    meta = {}
    if text.startswith("---\n"):
        fm, _, text = text[4:].partition("\n---\n")
        for line in fm.splitlines():
            k, _, v = line.partition(":")
            if k.strip():
                meta[k.strip()] = v.strip()
    bs = blocks(text)
    sections: list[tuple[Block, list[Block]]] = []
    for b in bs:
        if b.level == 1:
            sections.append((b, []))
        elif sections:
            sections[-1][1].append(b)
    seeds, live, keys, defs = [], [], [], []
    for h, sub in sections:
        kind = h.head.split()[0]
        if kind == "seed":
            seeds.append((seed_head(h.head, h.lines), resolve(turns(sub), named)))
        elif kind == "live":
            live += resolve(turns(sub), named)
        elif kind == "turns":
            defs += turns(sub)
        elif kind == "keys":
            for info, code in fences(h.lines + [l for b in sub for l in [f"{'#' * b.level} {b.head}"] + b.lines]):
                for line in code.splitlines():
                    if line.strip() and not line.strip().startswith("#"):
                        keys.append(shlex.split(line))
    return Recording(path.stem, meta, seeds, live, keys, defs)


def resolve(ts: list[Turn], named: dict | None) -> list[Turn]:
    """`use X`: the steps (or text) of the turn named X, own options win."""
    out = []
    for t in ts:
        if "use" in t.opts and named is not None:
            src = named[t.opts["use"]]
            t = Turn(t.who, t.to or src.to, {**src.opts, **{k: v for k, v in t.opts.items() if k != "use"}}, src.steps, t.text or src.text)
            t.opts.pop("name", None)
        out.append(t)
    return out


def parse_all() -> dict[str, Recording]:
    """Every recording; names resolve across files (2 passes: collect, then resolve)."""
    files = sorted(p for p in DIR.glob("*.md") if p.name != "README.md")
    named: dict[str, Turn] = {}
    for p in files:
        r = parse(p)
        for t in [t for _, ts in r.seeds for t in ts] + r.live + r.defs:
            if "name" in t.opts:
                named[t.opts["name"]] = t
    return {p.stem: parse(p, named) for p in files if not p.name.startswith("_")}
