---
title: Keys
description: Every key, per screen.
---

`codehead --help` prints the full list. ⌘ keys need a terminal with the kitty keyboard protocol.
`F2` and `>` are the defaults of the settings `[keys] menu` and `[keys] command`.

## Everywhere

| Key | Action |
| --- | --- |
| `F2` | menu: quick commands (`n`: new thread), threads, tokens, agents |
| `tab` / `shift+tab` | next / previous field (a question: its choices ↔ the prompt) |
| `ctrl+←` `ctrl+→` | previous / next tab of the start screen (word jumps: `alt+←` `alt+→`) |
| `esc` | 1 level back: never applies, never drops edits unasked |
| `ctrl+z` / `ctrl+shift+z` | undo / redo in any text field (`⌘z` / `⌘⇧z`) |

## Start screen

| Key | Action |
| --- | --- |
| `esc esc` | quit from the new tab |
| `ctrl+s` | new tab: save the task as a draft for later, start over |
| `ctrl+r` | new tab: reset the where row, notes and context (asks first) |
| `ctrl+c` | quit |
| `@` / `>` | agents (cards by harness) / commands dropdown in the prompt |
| `←` `→` | the **where** row: branch or worktree |
| `ctrl+v` | paste a clipboard image |
| `ctrl+s` / `ctrl+r` | settings: save / reload |
| `esc` | settings: undo the field's edit (0 apply); agent form with edits: asks first |
| `ctrl+n` / `ctrl+d` | settings: add / remove an alias |
| `ctrl+d`, click `↺` | settings: the field back to its default |

## Thread view

| Key | Action |
| --- | --- |
| `ctrl+enter` | send (`⌘enter`, `alt+enter`); `ctrl+s` never sends |
| `enter` | new line; a question open: answer |
| `↑` `↓` | a question open: its choices; else sent messages |
| `1`-`9`, `space` | a question: pick (several: toggle); letters: the **Other** answer |
| `←` `→` | a question with several parts: previous / next part |
| `esc` | back to live, else interrupt the running turns (after 0.5 s: a 2nd `esc` in time means back) |
| `esc esc` | back to the start screen |
| `ctrl+c` | clear the prompt; twice on an empty prompt: quit |
| `ctrl+o` | open / close all tool runs |
| `ctrl+t` | open / close all thoughts (hidden: show them) |
| `ctrl+i` | VS Code on the thread's worktree (`⌘i`) |
| `pgup` `pgdn`, `shift+↑↓`, wheel | scroll the stream |
