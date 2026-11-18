---
title: Thread view
description: The agents, the stream, your prompt and the cost, in 1 terminal.
---

The thread view has 4 parts:

- **Threads** (`tab`): a card per open thread: title, a wave while agents work, `●` unread.
  `←` `→` pick, `enter` or a click switches, `esc` closes.
- **Agents** (top): a card per agent of the thread: sprite, name (`★` the lead), model, effort
  and state: working, idle, blocked, done, or **waits on you**.
- **Stream** (middle): every message, tool run and question of the thread. Agent text renders
  Markdown with syntax colors in code blocks. Links open on a click.
- **Prompt** (bottom left): `enter` adds a line, `ctrl+enter` sends (`⌘enter`, `alt+enter`,
  `ctrl+s` too). `↑` on the 1st row of an empty prompt walks your sent messages.
- **Cost** (bottom right, screens 100 columns and wider): tokens, cost and context per agent,
  the last hour as a sparkline, and your plan's usage windows.

Your messages sit on the right with their time; a message the host has not taken yet shows
`sending…` below it, and `send failed: …` when it fails.

## Select and copy

Drag over the stream to select: the text goes to the clipboard. A double click takes a word,
a triple click the row, `ctrl+c` copies the selection too. A click on inline code copies it; a
code block has a **copy** button on its top row. Links open on a click.

## Tool runs

The tool calls of an agent between 2 of its messages fold into 1 row:

```text
│ ▸ Bash: run the tests · Read: src/webhook.rs and 3 more · 5 calls · 12.0s
```

A click opens the row: each call with its input and the tail of its output. `ctrl+o` opens or
closes all of them.

## Questions

A permission request or an agent question (Claude Code's `AskUserQuestion`) appears below the
stream with its choices. On an empty prompt: `↑` `↓` move, `1`-`9` pick, `space` picks several
(multi-select), `enter` answers. A click on a choice works too. A question with several parts
asks them 1 by 1.

## Steering

- `esc`: interrupt the running turns (scrolled up: back to live first).
- `esc esc`: back to the start screen. The thread keeps running.
- `F2`: the menu. Its **agents** tab adds and removes agents of the thread; **threads**
  switches to another thread.
- `>` **branch**: a copy of the thread, see [threads](/docs/concepts/threads/#branching).

## Notes in the stream

- **Errors** come with what to do next. A usage limit lists the agents on other harnesses
  that can go on (`@agent` → ⇄ replace).
- **needs harness**: an agent that cannot do a step on its own harness says so; codehead
  lists the agents on the harness it needs.
