"""Step builders for the fake harness scripts, and seeded events from the same steps."""

from __future__ import annotations

import uuid

P = "/home/dev/projects/payd"


def numbered(text: str, start: int = 1) -> str:
    return "\n".join(f"{i:>6}\t{l}" for i, l in enumerate(text.rstrip("\n").split("\n"), start))


def think(text, ms=700):
    return {"think": text, "ms": ms}


def say(text, cps=240):
    return {"say": text.strip("\n"), "cps": cps}


def read(path, text, ms=350, start=1):
    return {"tool": "Read", "input": {"file_path": f"{P}/{path}"}, "output": numbered(text, start), "ms": ms}


def grep(pattern, out, path=None, ms=300):
    inp = {"pattern": pattern, "output_mode": "content", "-n": True}
    if path:
        inp["path"] = f"{P}/{path}"
    return {"tool": "Grep", "input": inp, "output": out.strip("\n"), "ms": ms}


def bash(cmd, out, desc, ms=1200, ok=True, ask=False):
    return {"tool": "Bash", "input": {"command": cmd, "description": desc}, "output": out.strip("\n"), "ms": ms, "ok": ok, "ask": ask}


def edit(path, old, new, ms=250):
    return {"tool": "Edit", "input": {"file_path": f"{P}/{path}", "old_string": old.strip("\n"), "new_string": new.strip("\n")},
            "output": f"The file {P}/{path} has been updated successfully.", "ms": ms}


def write(path, content, ms=250):
    return {"tool": "Write", "input": {"file_path": f"{P}/{path}", "content": content.lstrip("\n")},
            "output": f"File created successfully at: {P}/{path}", "ms": ms}


def send(to, message):
    return {"send": {"to": to if isinstance(to, list) else [to], "message": message.strip()}}


def status(state, note=""):
    return {"status": {"state": state, "note": note}}


def question(qs, ms=400):
    return {"question": qs, "ms": ms}


def q(question, header, options, multi=False):
    return {"question": question, "header": header, "multiSelect": multi,
            "options": [{"label": o[0], "description": o[1]} for o in options]}


# ------------------------------------------------------------------ seeded events
def events(agent: str, steps: list, model="claude-opus-5-5", cost=0.84, prompt=None, frm="owner", to=None, gap=2500):
    """Events of 1 finished turn: PROMPT (optional), then STEPS as the host logs them."""
    ev = []
    if prompt:
        ev.append((None if frm == "owner" else frm, "prompt", {"from": frm, "to": to or [agent], "text": prompt, "_dt": 1000}))
    ev.append((agent, "session", {"session": str(uuid.uuid4()), "model": model, "_dt": 400}))
    ev.append((agent, "turn_start", {"_dt": 10}))
    total = 0
    for s in steps:
        if "think" in s:
            ev.append((agent, "thought", {"text": s["think"], "_dt": gap}))
        elif "say" in s:
            ev.append((agent, "text", {"text": s["say"], "_dt": gap + len(s["say"]) * 4}))
        elif "tool" in s or "send" in s or "status" in s:
            if "send" in s:
                name, inp, out = "mcp__codehead__send", {"to": s["send"]["to"], "message": s["send"]["message"]}, f"sent to {', '.join(s['send']['to'])}"
            elif "status" in s:
                name, inp, out = "mcp__codehead__set_status", s["status"], "ok"
            else:
                name, inp, out = s["tool"], s.get("input", {}), s.get("output", "")
            tid = "toolu_" + uuid.uuid4().hex[:22]
            ev.append((agent, "tool", {"id": tid, "name": name, "input": inp, "_dt": gap}))
            ms = s.get("ms", 800)
            total += ms
            ev.append((agent, "tool_result", {"id": tid, "ok": s.get("ok", True), "output": out, "_dt": ms}))
    ev.append((agent, "turn_end", {"cost": cost, "input": 1800, "output": 4200, "cache_read": 61000, "cache_write": 5200,
                                   "ctx": 64000, "window": 200000, "ms": 52000, "_dt": 500}))
    return ev
