#!/usr/bin/env python3
"""Fake harness for the demo recordings: plays scripted turns, 0 model calls.

Run as `claude` (any args): Claude Code's stream-json protocol on stdin / stdout.
Run as `codex app-server`: the Codex app server protocol (JSON-RPC, item/* notifications).
Run under any other name (`opencode acp`, `goose acp`, ...): the Agent Client Protocol.

Script: $DEMO_SCRIPT, a JSON file {agent: {"model": ..., "turns": [turn, ...]}}. A turn is
{"match": regex (optional), "steps": [step, ...], "usage": {...} (optional)}. Each prompt to
the agent plays the 1st unplayed turn whose regex matches it. Played turns are kept in
$DEMO_STATE/<agent>.json, so a new session goes on where the last one stopped.

Steps:
  {"think": text}                                  a thought (reasoning summary)
  {"say": markdown, "cps": 260}                    text, streamed in chunks
  {"tool": name, "input": {...}, "output": text, "ok": true, "ms": 900, "ask": false}
  {"question": [questions], "ms": 400}             AskUserQuestion: waits on the owner
  {"send": {"to": [..], "message": text}}          codehead MCP send (delivered via ctl)
  {"status": {"state": .., "note": ..}}            codehead MCP set_status
  {"sleep": seconds}
  {"limits": {"five_hour": [0.42, resets_s], ...}} plan usage windows (Claude)
  {"fail": text}                                   the turn ends with this error
Only the Python standard library: it runs inside the recording's mount namespace.
"""

import json
import os
import queue
import random
import re
import subprocess
import sys
import threading
import time
import uuid

AGENT = os.environ.get("CODEHEAD_AGENT", "agent")
THREAD = os.environ.get("CODEHEAD_THREAD", "")
SCRIPT = json.load(open(os.environ["DEMO_SCRIPT"])) if os.environ.get("DEMO_SCRIPT") else {}
STATE = os.environ.get("DEMO_STATE", "/tmp")
ME = SCRIPT.get(AGENT, {})
LOG = open(os.path.join(STATE, f"{AGENT}.log"), "a", buffering=1)

out_lock = threading.Lock()


def emit(v):
    with out_lock:
        sys.stdout.write(json.dumps(v) + "\n")
        sys.stdout.flush()


def log(*a):
    LOG.write(" ".join(str(x) for x in a) + "\n")


def played_path():
    return os.path.join(STATE, f"{AGENT}.json")


def next_turn(prompt):
    try:
        played = json.load(open(played_path()))
    except Exception:
        played = []
    for i, t in enumerate(ME.get("turns", [])):
        if i in played:
            continue
        m = t.get("match")
        if m and not re.search(m, prompt, re.S | re.I):
            continue
        played.append(i)
        json.dump(played, open(played_path(), "w"))
        return t
    return {"steps": [{"say": "Done here. Nothing left on my side."}]}


def ctl(op):
    if not THREAD:
        return
    try:
        subprocess.run(["codehead", "ctl", THREAD, json.dumps(op)], capture_output=True, timeout=10)
    except Exception as e:
        log("ctl", e)


def chunks(text, cps):
    """Text in token-sized chunks with the pause per chunk."""
    i = 0
    while i < len(text):
        n = random.randint(2, 7)
        part = text[i : i + n]
        i += n
        yield part, len(part) / cps * random.uniform(0.6, 1.4)


class Cancelled(Exception):
    pass


# ---------------------------------------------------------------- Claude Code stream-json
class Claude:
    def __init__(self):
        self.session = str(uuid.uuid4())
        self.model = ME.get("model", "claude-opus-5-5")
        self.inbox = queue.Queue()
        self.responses = {}
        self.resp_cv = threading.Condition()
        self.cancel = threading.Event()
        self.cost = 0.0
        self.ctx = ME.get("ctx", 18000)
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in sys.stdin:
            try:
                v = json.loads(line)
            except ValueError:
                continue
            t = v.get("type")
            if t == "user":
                c = v["message"]["content"]
                text = c if isinstance(c, str) else " ".join(b.get("text", "") for b in c if isinstance(b, dict))
                self.inbox.put(text)
            elif t == "control_response":
                r = v["response"]
                with self.resp_cv:
                    self.responses[r["request_id"]] = r.get("response", {})
                    self.resp_cv.notify_all()
            elif t == "control_request" and v["request"].get("subtype") == "interrupt":
                self.cancel.set()
                with self.resp_cv:
                    self.resp_cv.notify_all()
        os._exit(0)

    def wait_response(self, rid):
        with self.resp_cv:
            while rid not in self.responses:
                if self.cancel.is_set():
                    raise Cancelled()
                self.resp_cv.wait(0.1)
            return self.responses.pop(rid)

    def sleep(self, s):
        if self.cancel.wait(s):
            raise Cancelled()

    def assistant(self, content, grow=0):
        self.ctx += grow
        emit({"type": "assistant", "parent_tool_use_id": None, "session_id": self.session,
              "message": {"id": "msg_" + uuid.uuid4().hex[:20], "role": "assistant", "model": self.model, "content": content,
                          "usage": {"input_tokens": 4, "cache_read_input_tokens": self.ctx - 900, "cache_creation_input_tokens": 896, "output_tokens": 120}}})

    def tool(self, name, inp, output, ok=True, ms=800, ask=False):
        tid = "toolu_" + uuid.uuid4().hex[:22]
        self.assistant([{"type": "tool_use", "id": tid, "name": name, "input": inp}], grow=random.randint(300, 900))
        if ask:
            rid = str(uuid.uuid4())
            emit({"type": "control_request", "request_id": rid,
                  "request": {"subtype": "can_use_tool", "tool_name": name, "input": inp, "tool_use_id": tid}})
            r = self.wait_response(rid)
            if r.get("behavior") != "allow":
                emit({"type": "user", "parent_tool_use_id": None, "session_id": self.session,
                      "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "is_error": True,
                                                                "content": r.get("message", "The user doesn't want to proceed with this tool use.")}]}})
                return r
            inp = r.get("updatedInput", inp)
            self.sleep(0.15)
        self.sleep(ms / 1000)
        if callable(output):
            output = output(inp)
        emit({"type": "user", "parent_tool_use_id": None, "session_id": self.session,
              "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "is_error": not ok, "content": output}]}})
        self.ctx += len(output) // 4
        return inp

    def turn(self, prompt):
        self.cancel.clear()
        t0 = time.time()
        emit({"type": "system", "subtype": "init", "session_id": self.session, "model": self.model, "cwd": os.getcwd(), "tools": []})
        t = next_turn(prompt)
        usage = t.get("usage", {})
        error = None
        try:
            self.sleep(t.get("delay", 0.9))
            for s in t["steps"]:
                self.step(s)
        except Cancelled:
            error = "error_during_execution"
        except Failed as f:
            error = str(f)
        self.cost += usage.get("cost", round(random.uniform(0.18, 0.6), 2))
        res = {"type": "result", "subtype": "error_during_execution" if error == "error_during_execution" else "success",
               "is_error": error is not None, "result": error or "", "session_id": self.session,
               "duration_ms": int((time.time() - t0) * 1000), "total_cost_usd": round(self.cost, 4),
               "usage": {"input_tokens": usage.get("input", 1200), "output_tokens": usage.get("output", 2400),
                         "cache_read_input_tokens": usage.get("cache_read", self.ctx), "cache_creation_input_tokens": usage.get("cache_write", 3200)},
               "modelUsage": {self.model: {"contextWindow": ME.get("window", 200000)}}}
        emit(res)

    def step(self, s):
        if "think" in s:
            self.sleep(s.get("ms", 700) / 1000)
            self.assistant([{"type": "thinking", "thinking": s["think"], "signature": "x"}])
        elif "say" in s:
            idx = 0
            emit({"type": "stream_event", "parent_tool_use_id": None, "session_id": self.session,
                  "event": {"type": "content_block_start", "index": idx, "content_block": {"type": "text", "text": ""}}})
            for part, pause in chunks(s["say"], s.get("cps", 260)):
                emit({"type": "stream_event", "parent_tool_use_id": None, "session_id": self.session,
                      "event": {"type": "content_block_delta", "index": idx, "delta": {"type": "text_delta", "text": part}}})
                self.sleep(pause)
            self.assistant([{"type": "text", "text": s["say"]}], grow=len(s["say"]) // 3)
        elif "tool" in s:
            self.tool(s["tool"], s.get("input", {}), s.get("output", ""), s.get("ok", True), s.get("ms", 800), s.get("ask", False))
        elif "question" in s:
            self.sleep(s.get("ms", 400) / 1000)

            def answered(inp):
                ans = inp.get("answers", {})
                return "User has answered your questions: " + ", ".join(f'"{q}"="{a}"' for q, a in ans.items()) + ". You can now continue with the user's answers in mind."
            self.tool("AskUserQuestion", {"questions": s["question"]}, answered, ms=100, ask=True)
        elif "send" in s:
            inp = {"to": s["send"]["to"], "message": s["send"]["message"]}
            self.tool("mcp__codehead__send", inp, lambda i: (ctl({"op": "send", "from": AGENT, "to": i["to"], "text": i["message"]}), f"sent to {', '.join(i['to'])}")[1], ms=300)
        elif "status" in s:
            inp = dict(s["status"])
            self.tool("mcp__codehead__set_status", inp, lambda i: (ctl({"op": "status", "agent": AGENT, "state": i["state"], "note": i.get("note", "")}), "ok")[1], ms=200)
        elif "sleep" in s:
            self.sleep(s["sleep"])
        elif "limits" in s:
            now = int(time.time())
            emit({"type": "rate_limit_event", "rate_limit_info": {"unifiedWindows": {
                k: {"utilization": v[0], "resetsAt": now + v[1]} for k, v in s["limits"].items()}}})
        elif "fail" in s:
            raise Failed(s["fail"])

    def run(self):
        while True:
            self.turn(self.inbox.get())


class Failed(Exception):
    pass


# ---------------------------------------------------------------- ACP
class Acp:
    def __init__(self):
        self.sid = None
        self.next = 1000
        self.pending = {}
        self.cv = threading.Condition()
        self.prompts = queue.Queue()
        self.cancel = threading.Event()
        self.model = ME.get("model", "gpt-5.5")
        self.ctx = ME.get("ctx", 22000)

    def send(self, v):
        v["jsonrpc"] = "2.0"
        emit(v)

    def update(self, u):
        self.send({"method": "session/update", "params": {"sessionId": self.sid, "update": u}})

    def request(self, method, params):
        self.next += 1
        rid = self.next
        self.send({"id": rid, "method": method, "params": params})
        with self.cv:
            while rid not in self.pending:
                if self.cancel.is_set():
                    raise Cancelled()
                self.cv.wait(0.1)
            return self.pending.pop(rid)

    def read(self):
        for line in sys.stdin:
            try:
                v = json.loads(line)
            except ValueError:
                continue
            m = v.get("method")
            if m is None:
                with self.cv:
                    self.pending[v.get("id")] = v.get("result") or {}
                    self.cv.notify_all()
            elif m == "initialize":
                self.send({"id": v["id"], "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}, "authMethods": []}})
            elif m in ("session/new", "session/load"):
                self.sid = v["params"].get("sessionId") or "sess_" + uuid.uuid4().hex[:16]
                self.send({"id": v["id"], "result": {"sessionId": self.sid, "models": {"currentModelId": self.model, "availableModels": [{"modelId": self.model, "name": self.model}]}}})
            elif m == "session/set_model":
                self.send({"id": v["id"], "result": {}})
            elif m == "session/prompt":
                text = " ".join(b.get("text", "") for b in v["params"].get("prompt", []))
                self.prompts.put((v["id"], text))
            elif m == "session/cancel":
                self.cancel.set()
                with self.cv:
                    self.cv.notify_all()
            elif "id" in v:
                self.send({"id": v["id"], "error": {"code": -32601, "message": "not supported"}})
        os._exit(0)

    def sleep(self, s):
        if self.cancel.wait(s):
            raise Cancelled()

    def tool(self, name, inp, output, ok=True, ms=800, ask=False):
        tid = "call_" + uuid.uuid4().hex[:16]
        kind = {"Bash": "execute", "Read": "read", "Edit": "edit", "Write": "edit", "Grep": "search", "Glob": "search", "WebFetch": "fetch"}.get(name, "other")
        title = inp.get("description") or name
        if kind == "other":
            title = name
        self.update({"sessionUpdate": "tool_call", "toolCallId": tid, "title": title, "kind": kind, "status": "pending", "rawInput": inp})
        if ask:
            r = self.request("session/request_permission", {"sessionId": self.sid, "toolCall": {"toolCallId": tid, "title": title, "kind": kind, "rawInput": inp},
                                                             "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                                                                         {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
                                                                         {"optionId": "reject", "name": "Reject", "kind": "reject_once"}]})
            if r.get("outcome", {}).get("optionId") == "reject":
                self.update({"sessionUpdate": "tool_call_update", "toolCallId": tid, "status": "failed", "content": [{"type": "content", "content": {"type": "text", "text": "rejected by the user"}}]})
                return
        self.sleep(ms / 1000)
        if callable(output):
            output = output(inp)
        self.update({"sessionUpdate": "tool_call_update", "toolCallId": tid, "status": "completed" if ok else "failed",
                     "content": [{"type": "content", "content": {"type": "text", "text": output}}]})
        self.ctx += len(output) // 4

    def step(self, s):
        if "think" in s:
            self.sleep(s.get("ms", 600) / 1000)
            for part, pause in chunks(s["think"], 400):
                self.update({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": part}})
                self.sleep(pause)
        elif "say" in s:
            for part, pause in chunks(s["say"], s.get("cps", 260)):
                self.update({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": part}})
                self.sleep(pause)
            self.ctx += len(s["say"]) // 3
        elif "tool" in s:
            self.tool(s["tool"], s.get("input", {}), s.get("output", ""), s.get("ok", True), s.get("ms", 800), s.get("ask", False))
        elif "send" in s:
            inp = {"to": s["send"]["to"], "message": s["send"]["message"]}
            self.tool("mcp__codehead__send", inp, lambda i: (ctl({"op": "send", "from": AGENT, "to": i["to"], "text": i["message"]}), f"sent to {', '.join(i['to'])}")[1], ms=300)
        elif "status" in s:
            inp = dict(s["status"])
            self.tool("mcp__codehead__set_status", inp, lambda i: (ctl({"op": "status", "agent": AGENT, "state": i["state"], "note": i.get("note", "")}), "ok")[1], ms=200)
        elif "sleep" in s:
            self.sleep(s["sleep"])
        elif "fail" in s:
            raise Failed(s["fail"])

    def run(self):
        threading.Thread(target=self.read, daemon=True).start()
        while True:
            rid, text = self.prompts.get()
            self.cancel.clear()
            t = next_turn(text)
            stop = "end_turn"
            try:
                self.sleep(t.get("delay", 0.8))
                for s in t["steps"]:
                    self.step(s)
            except Cancelled:
                stop = "cancelled"
            except Failed as f:
                self.send({"id": rid, "error": {"code": -32000, "message": str(f)}})
                continue
            u = t.get("usage", {})
            self.send({"id": rid, "result": {"stopReason": stop, "usage": {
                "inputTokens": u.get("input", 2100), "outputTokens": u.get("output", 1900),
                "cachedReadTokens": u.get("cache_read", self.ctx), "cachedWriteTokens": 0, "totalTokens": self.ctx}}})


# ---------------------------------------------------------------- Codex app server
class Codex(Acp):
    """`codex app-server`: JSON-RPC on stdio (src/engine/codex.rs). initialize → thread/start →
    turn/start; a turn streams item/* notifications and ends with turn/completed."""

    def __init__(self):
        super().__init__()
        self.tid = None
        self.turn_id = None

    def send(self, v):
        emit(v)

    def read(self):
        for line in sys.stdin:
            try:
                v = json.loads(line)
            except ValueError:
                continue
            m = v.get("method")
            if m is None:
                with self.cv:
                    self.pending[v.get("id")] = v.get("result") or {}
                    self.cv.notify_all()
            elif m == "initialize":
                self.send({"id": v["id"], "result": {"userAgent": "codex-demo"}})
            elif m in ("thread/start", "thread/resume", "thread/fork"):
                self.tid = v["params"].get("threadId") or "thr_" + uuid.uuid4().hex[:16]
                self.send({"id": v["id"], "result": {"model": self.model, "thread": {"id": self.tid}}})
            elif m == "turn/start":
                text = " ".join(b.get("text", "") for b in v["params"].get("input", []))
                self.turn_id = "turn_" + uuid.uuid4().hex[:12]
                self.send({"id": v["id"], "result": {"turn": {"id": self.turn_id}}})
                self.prompts.put((self.turn_id, text))
            elif m == "turn/interrupt":
                self.cancel.set()
                self.send({"id": v["id"], "result": {}})
                with self.cv:
                    self.cv.notify_all()
            elif "id" in v:
                self.send({"id": v["id"], "result": {}})
        os._exit(0)

    def note(self, method, params):
        self.send({"method": method, "params": params})

    def item(self, name, inp):
        """A scripted tool → an app server item (started)."""
        iid = "item_" + uuid.uuid4().hex[:12]
        if name in ("Edit", "Write"):
            path = inp.get("file_path", "")
            return {"type": "fileChange", "id": iid, "status": "inProgress", "changes": [{"path": path, "kind": {"type": "update"}, "diff": ""}]}
        if name.startswith("mcp__"):
            _, server, tool = name.split("__", 2)
            return {"type": "mcpToolCall", "id": iid, "status": "inProgress", "server": server, "tool": tool, "arguments": inp}
        cmd = inp.get("command") or {"Read": "cat", "Grep": "rg", "Glob": "ls"}.get(name, name) + " " + str(inp.get("file_path") or inp.get("pattern") or inp.get("path") or "")
        return {"type": "commandExecution", "id": iid, "status": "inProgress", "command": cmd.strip(), "cwd": os.getcwd()}

    def tool(self, name, inp, output, ok=True, ms=800, ask=False):
        it = self.item(name, inp)
        self.note("item/started", {"item": it, "threadId": self.tid, "turnId": self.turn_id})
        if ask and it["type"] == "commandExecution":
            r = self.request("item/commandExecution/requestApproval", {"itemId": it["id"], "threadId": self.tid, "turnId": self.turn_id, "command": it["command"], "cwd": it["cwd"]})
            if r.get("decision") not in ("accept", "acceptForSession"):
                self.note("item/completed", {"item": dict(it, status="declined")})
                return
        self.sleep(ms / 1000)
        if callable(output):
            output = output(inp)
        done = dict(it, status="completed" if ok else "failed")
        if it["type"] == "commandExecution":
            done.update(exitCode=0 if ok else 1, aggregatedOutput=output)
        elif it["type"] == "fileChange":
            done["changes"] = [dict(done["changes"][0], diff=output)]
        else:
            done["result"] = {"content": [{"type": "text", "text": output}]}
        self.note("item/completed", {"item": done})
        self.ctx += len(output) // 4

    def step(self, s):
        if "think" in s:
            self.sleep(s.get("ms", 600) / 1000)
            self.note("item/completed", {"item": {"type": "reasoning", "id": "r_" + uuid.uuid4().hex[:8], "summary": [s["think"]], "content": []}})
        elif "say" in s:
            iid = "msg_" + uuid.uuid4().hex[:8]
            for part, pause in chunks(s["say"], s.get("cps", 260)):
                self.note("item/agentMessage/delta", {"itemId": iid, "delta": part})
                self.sleep(pause)
            self.note("item/completed", {"item": {"type": "agentMessage", "id": iid, "text": s["say"]}})
            self.ctx += len(s["say"]) // 3
        else:
            super().step(s)

    def run(self):
        threading.Thread(target=self.read, daemon=True).start()
        while True:
            tid, text = self.prompts.get()
            self.cancel.clear()
            self.note("turn/started", {"turn": {"id": tid}})
            t = next_turn(text)
            status, error = "completed", None
            t0 = time.time()
            try:
                self.sleep(t.get("delay", 0.8))
                for s in t["steps"]:
                    self.step(s)
            except Cancelled:
                status = "interrupted"
            except Failed as f:
                status, error = "failed", {"message": str(f)}
            u = t.get("usage", {})
            last = {"inputTokens": u.get("input", 2100) + self.ctx, "cachedInputTokens": self.ctx, "outputTokens": u.get("output", 1900), "totalTokens": self.ctx}
            self.note("thread/tokenUsage/updated", {"tokenUsage": {"total": last, "last": last, "modelContextWindow": ME.get("window", 272000)}})
            self.note("turn/completed", {"turn": {"id": tid, "status": status, "error": error, "durationMs": int((time.time() - t0) * 1000)}})


if __name__ == "__main__":
    try:
        if os.path.basename(sys.argv[0]) == "claude":
            Claude().run()
        elif os.path.basename(sys.argv[0]) == "codex" and "app-server" in sys.argv[1:]:
            Codex().run()
        else:
            Acp().run()
    except BrokenPipeError:
        pass
