---
title: Tasks
description: The new tab, where a thread starts.
---

A task is what you write on the **new** tab before a thread starts: a prompt, a title, notes,
agents and a place to work. Starting it makes a thread and sends the task to every picked
agent as its first message. The prompt comes first; the title is optional: 0 title, the
prompt's 1st line names the thread (and its branch). Type, then `ctrl+enter`. `tab` / `shift+tab`
walk the fields; `ctrl+←` `ctrl+→` switch tabs.

## Prompt

- `@` lists agent names. `>` lists commands: files, notes, threads, artifacts and skills.
- A path (`src/`, `./a/b`, `~/x/`) lists that directory as you type. `tab` inserts the row.
- `ctrl+v` pastes a clipboard image as `[Image #n]`. Text over 256 characters becomes
  `[Pasted text #n +L lines]`. Both go into the message as attachments.
- Markdown lists continue on `enter`.

## Context

The **threads** tab lists recent Claude Code sessions. `enter` on one starts a task in its
directory with its last response as context. Threads work the same way: their lead's last
response goes in.

## Where the work runs

The **where** row (`←` `→`) picks the branch:

- the current branch (default): the thread works in place,
- `main` or another branch (newest commit first),
- a new branch in the main checkout,
- a new branch with its own worktree (`-b` sets this). codehead makes the worktree under
  `~/.local/share/codehead/worktrees/<repo>/<branch>`.

Outside a git repository, the thread works in the current directory.

## Drafts

An unsent task is saved after every key to `~/.cache/codehead/drafts`. The new tab shows
**view drafts (n)** on top. A started thread drops its draft; drafts older than 30 days go.

An idea for later (a usage limit hit, a thread you do not want yet): `ctrl+s` saves the task as
a kept draft and empties the form. Kept drafts show **★ saved** in the list and never expire.
