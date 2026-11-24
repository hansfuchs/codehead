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

## Worktrees

A thread on a new branch + worktree works like the main checkout, with 0 manual steps:

- **Base**: a new branch starts from `origin/<default>` (fetched first), else from HEAD.
- **Local files**: the paths in `.worktreeinclude` (repo root, 1 per line, `#` comments) are
  copied from the main checkout: an APFS clone on macOS, a reflink where the file system has
  one. 0 file: every gitignored `.env*` file of the root. A path the worktree has stays.
- **Dev server URLs** ([portless](https://github.com/vercel-labs/portless), installed with
  codehead when Node 24+ is there; setting `ports`): agents start dev servers with
  `portless run <command>`, and each worktree gets its own `https://<branch tail>.<app>.localhost`
  (`$PORTLESS_PREFIX` in the agents' and the setup script's env). The thread view's top row
  shows the live URL on the right (a click opens it); the threads tab shows it in the details.
- **Ports**: each worktree thread gets 10 ports, `$CODEHEAD_PORT` (40000, 40010, …) to `+9`, in
  the agents' env and system prompt: for databases and other non-HTTP services (with
  `ports = "block"`: for dev servers too).
- **Setup**: `.codehead/setup`, else `.agents/setup`, else `conductor.json` `scripts.setup`
  runs in the new worktree before the 1st turn (20 min max). The thread shows it as a `setup`
  tool run; messages wait for it. A failed setup: the agents get its output and a note.
- **Teardown**: `.codehead/teardown`, else `conductor.json` `scripts.archive`, runs before
  codehead removes the worktree.

Scripts get `CODEHEAD_ROOT` (main checkout), `CODEHEAD_WORKTREE`, `CODEHEAD_THREAD`,
`CODEHEAD_BRANCH`, `CODEHEAD_PORT` (and `CONDUCTOR_ROOT_PATH`, `CONDUCTOR_PORT`).

```sh
#!/bin/sh
# .codehead/setup
set -e
pnpm install --frozen-lockfile
echo "PORT=$CODEHEAD_PORT" >> .env.local
```

## Handoff

The `>` command **handoff** continues a thread in a new one, with a short document instead of
the full history: the `review` model writes it from the thread's log (goal, decisions with why,
state of the code, failed approaches, open todos, how to run and test, owner preferences), and
codehead adds exact facts (branch, directory, ports, uncommitted files, last commits). The new
thread keeps the worktree, branch, ports and agents, starts fresh sessions (any harness), and
opens with the document as its prompt draft: edit it, add the next goal, send. `thread.json`
`parent` names the old thread.

## Sub-threads

A side question should not fill the thread's history. `>sub <task>` hands it to a sub-thread:
0 history of the thread, the same checkout, the same branch and ports. The task takes
`@names` (else the agents your last message went to), `#shortcuts` and any text:

```text
>sub @HAL why does retry.rs cap at 30 min? 1 paragraph.
```

While a sub-thread runs, the thread waits: your messages to its agents are held, the
sub-threads row above the subagents row shows each run (a click: its agents, its time, its
last steps; `d` stops it). At its end, the sub-thread posts its result into the thread as 1
message, deletes itself and the held messages go out. A sub-thread gets 0 row of its own in the
threads list.
