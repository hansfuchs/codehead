---
title: Threads
description: A task and everything its agents do about it, kept in 2 files.
---

A thread is a task and everything done about it: the messages, the tool runs, the questions,
the answers. It outlives the terminal you started it in.

## The host

Each thread with work going on has a **host**: `codehead host <id>`, a process in its own
session, so it keeps running when codehead closes. codehead starts it with the first message
and talks to it over a Unix socket (`$XDG_RUNTIME_DIR/codehead/<id>.sock`).

The host runs 1 harness session per working agent. A session starts on the first message
to its agent and resumes the agent's last session on the thread, so the conversation carries
over. 15 minutes after the last turn (0 open questions), the host ends its sessions and exits.
The next message starts it again.

The host runs each harness in its own process group, so a stop or an interrupt reaches the
harness's children too. `SIGTERM`, `SIGINT` and `SIGHUP` stop the host cleanly: it closes cut
and crashed turns in the log. An ACP harness with 0 session after 120 s ends that session with
an error. File locks guard `thread.json`, `limits.json` and the host socket, so 2 codeheads
never write over each other.

## Records

```
~/.local/share/codehead/threads/<id>/
  thread.json    title, task, dir, branch, agents (lead first), session per agent, state
  events.jsonl   1 JSON event per line, append only
```

Events: `prompt`, `text`, `thought`, `tool`, `tool_result`, `ask`, `answer`, `turn_start`, `turn_end`
(cost, tokens, context), `session`, `status`, `join`, `leave`, `error`. The thread view and
the cost box read only this log; `$CODEHEAD_HOME` moves the whole folder.

## Messages

An owner message without `@name` goes to the agents of your last message (still on the
thread); the 1st one goes to the **lead**, the 1st agent of the thread. The hint row under the
prompt shows who gets it. `@name` sends it to that agent instead; an agent not on the thread
joins it.

## Scripting

`codehead ctl <thread> '<json>'` sends 1 command to a thread's host (started if down) and
prints its reply:

```sh
codehead ctl 6a1f3c '{"op":"prompt","text":"run the tests again"}'
codehead ctl 6a1f3c '{"op":"ping"}'
codehead ctl 6a1f3c '{"op":"stop"}'
```

| op | fields | effect |
| --- | --- | --- |
| `prompt` | `text`, `to?` | an owner message |
| `send` | `from`, `to`, `text` | a message from 1 agent to others (what the MCP `send` tool does) |
| `status` | `agent`, `state`, `note?` | a status row (`working`, `blocked`, `done`) |
| `answer` | `id`, `allow`, `answers?`, `label` | answer an open question |
| `interrupt` |  | stop the running turns |
| `join` / `leave` | `agent`, `text?` | add / remove an agent |
| `reload` | `agent`, `fresh?` | end an agent's idle session (its profile changed) |
| `ping` |  | live and busy agents, open questions |
| `stop` |  | end every session, exit |
| `subscribe` |  | keep the connection open: `live` / `delta` (text as it streams), `wake` (new events in the log) |

## Branching

The `>` command **branch** copies a thread: its events, agents and directory. Each agent's
next session forks its Claude Code session (`--fork-session`), so the 2 threads go their own
ways from there. Files are shared: same directory or worktree.

## Your edits

You edit the agents' files in your IDE between their turns. Before the next turn of an agent,
the host tells it what changed in the worktree since its last turn and was not its own work:

```
[codehead: changes in this worktree since your last turn, not made by you. Read a file again before you edit it.]
owner:
- src/deploy.rs:40-52 (+5 −2)
- src/retry.rs (new)
@lilith:
- src/api.rs:10-30 (+18 −3)
owner diff:
…
[end of changes]
```

The thread view shows the same as a note under the agent's name. Whitespace-only edits (a
formatter on save) are left out. The `edits` setting picks `off`, `files` or `diff` (default).

How: at each turn's start and end, the host writes a git tree of the worktree (tracked and
untracked files, ignored ones left out) through a temp index, so your index, HEAD and branch
stay as they are. Your edits are the changes while 0 agents work; an edit you make while an
agent works counts as that agent's. A worktree with more than 2000 untracked files gets 0
note. State: `threads/<id>/edits.json`.
