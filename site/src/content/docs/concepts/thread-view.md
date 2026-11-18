---
title: Thread view
description: The agents, the stream, your prompt and the cost, in 1 terminal.
---

The thread view has these parts, top to bottom:

- **Threads** (`tab`): a card per open thread: title, a wave while agents work, `●` unread.
  `←` `→` pick, `enter` or a click switches, `esc` closes.
- **Stream**: every message, tool run and question of the thread. Agent text renders
  Markdown (tables too) with syntax colors in code blocks. Links open on a click. Scrolled
  up, its last row is a `┄` fold line.
- **Title row**: the thread title, the scroll position (`↑12 · esc: live`).
- **Header**: 1 frame with a card per agent (sprite, name, harness and model, effort, and
  **working…**, **monitoring…** or **waits on you**) and, on screens 70 columns and wider,
  the tokens box: tokens, cost and context per agent, the last hour as a sparkline, and your
  plan's usage windows.
- **Prompt**: a rounded bubble below the header. `enter` adds a line, `ctrl+enter` sends
  (`⌘enter`, `alt+enter`, `ctrl+s` too). `↑` on the 1st row of an empty prompt walks your
  sent messages. The row below names who gets the message (`→ ada`: the lead, or the
  `@named` agents).

The `messages` setting picks how messages look:

- `bubbles` (default): an agent's turn in a rounded box, its name and time in the top border,
  a wave there while it works. Your messages sit in a bubble on the right, their time in its
  top border, attached files below it.
- `lines`: a name badge, then the agent's text behind a bar in its color. Your messages on the
  right with their time.

A message the host has not taken yet shows `sending…` below it, and `send failed: …` when it
fails.

## Select and copy

Drag over the stream to select: the text goes to the clipboard. A double click takes a word,
a triple click the row, `ctrl+c` copies the selection too. A click on inline code copies it; a
code block has a **copy** button on its top row. Links open on a click.

## Tool runs

The tool calls of an agent between 2 of its messages fold into 1 row:

```text
│ ▸ Run the tests, read webhook.rs, edit main.rs and 2 more
│   5 calls · 12.0s
```

(`lines`: 1 row, `▸ Bash: run the tests · Read: src/webhook.rs and 3 more · 5 calls · 12.0s`.)

A click opens the row: each call with its code and the tail of its output, as code blocks with
syntax colors and a copy button (a Bash command, the text of a Write, an Edit as a diff).
`ctrl+o` opens or closes all of them.

## Thoughts

Some harnesses report what the agent thinks between steps (Claude Code's thinking summaries,
the `agent_thought_chunk`s of ACP harnesses): 1-2 sentences like "cause found, fixing it now".
The `thoughts` setting picks how they show:

- `hidden` (default): 0 rows.
- `folded`: a run counts its thoughts (`5 calls · 2 thoughts · 12.0s`); opened, they sit
  between its calls. Thoughts right before a message are 1 folded line on top of it:

  ```text
  │ ▸ ∴ The timing flake is pre-existing, so I'm retrying the push once.
  ```

  A click opens it.
- `open`: every thought, in full.

`ctrl+t` opens or closes all thoughts; with `hidden`, it shows them for this view.

## Questions

A permission request or an agent question (Claude Code's `AskUserQuestion`) appears below the
stream with its choices. On an empty prompt: `↑` `↓` move, `1`-`9` pick, `space` picks several
(multi-select), `enter` answers. A click on a choice works too. A question with several parts
asks them 1 by 1.

## Steering

- `esc`: interrupt the running turns (scrolled up: back to live first).
- `esc esc`: back to the start screen. The thread keeps running.
- `F2`: the menu. `n` in its **quick** tab: a new thread. Its **agents** tab shows the agents of the
  thread as cards by harness, adds and removes them; **threads**
  switches to another thread.
- `>` **branch**: a copy of the thread, see [threads](/docs/concepts/threads/#branching).

## Notes in the stream

- **Errors** come with what to do next. A usage limit lists the agents on other harnesses
  that can go on (`@agent` → `→` replace).
- **needs harness**: an agent that cannot do a step on its own harness says so; codehead
  lists the agents on the harness it needs.
