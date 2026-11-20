---
title: Introduction
description: What codehead is and how a thread works.
---

codehead is a terminal app for agent work. You write the task, with notes and context, pick
the agents and the branch, and codehead starts a **thread**: the task and everything its agents
do about it. You follow the thread in the same terminal, answer the agents' questions, and
steer them with more messages.

codehead runs the agents itself, on the coding agent you already use: Claude Code, Codex,
OpenCode, Goose, Cursor, Pi, Oh My Pi, Devin, Grok Build, Kimi Code, Amp, Hermes Agent or
OpenClaw ([the list](/docs/requirements/#a-harness)). 1 thread can mix harnesses. Each agent is a
session of its harness that codehead starts, feeds and listens to.

## What you get

- **Task form**: title, prompt, notes from your notes folder, agents, branch or worktree.
- **Thread view**: the agents and the thread's cost on top, the live stream of messages, tool
  runs, thoughts and questions, your prompt.
- **Questions**: permission requests and agent questions in the stream, answered with a few keys.
- **Named agents**: a model, a mode, an output style and an avatar each, in 1 TOML file.
- **Agents together**: agents message each other and bring each other into the thread.
- **Threads tab**: threads, worktrees and recent Claude Code sessions to continue.

## How it fits together

| Part | What it does |
| --- | --- |
| `codehead` | the screens: task form, threads, agents, settings, thread view |
| thread host | `codehead host <id>`: 1 detached process per thread, runs its agents |
| agent session | 1 per agent: `claude -p` (stream-json), else the harness's ACP mode |
| codehead MCP server | the `send`, `set_status` and `list_agents` tools every agent gets |
| records | `thread.json` + `events.jsonl` per thread, 1 TOML file per agent |

The host keeps running when you close codehead. Its agents finish their turns, and the next
codehead you open shows everything that happened. See [threads](/docs/concepts/threads/).
