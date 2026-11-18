---
title: Keys
description: Every key, per screen.
---

`codehead --help` prints the full list. ⌘ keys need a terminal with the kitty keyboard protocol.
`F2` and `>` are the defaults of the settings `[keys] menu` and `[keys] command`.

## Everywhere

| Key | Action |
| --- | --- |
| `F2` | menu: quick commands, threads, tokens, agents |
| `tab` / `shift+tab` | next / previous tab or field |
| `esc` | close the current control, 1 level back |
| `ctrl+z` / `ctrl+shift+z` | undo / redo in any text field (`⌘z` / `⌘⇧z`) |

## Start screen

| Key | Action |
| --- | --- |
| `esc esc` | quit from the new tab |
| `ctrl+s` | new tab: save the task as a draft for later, start over |
| `ctrl+c` | quit |
| `@` / `>` | agents / commands dropdown in the prompt |
| `←` `→` | the **where** row: branch or worktree |
| `ctrl+v` | paste a clipboard image |
| `ctrl+s` / `ctrl+r` | settings: save / reload |
| `ctrl+n` / `ctrl+d` | settings: add / remove an alias |
| `ctrl+d`, click `↺` | settings: the field back to its default |

## Thread view

| Key | Action |
| --- | --- |
| `ctrl+enter` | send (`⌘enter`, `alt+enter`, `ctrl+s`) |
| `enter` | new line; on an empty prompt with a question open: answer |
| `↑` `↓` | question choices (empty prompt), else sent messages |
| `1`-`9`, `space` | pick a choice, pick several |
| `esc` | back to live, else interrupt the running turns |
| `esc esc` | back to the start screen |
| `ctrl+c` | clear the prompt; twice on an empty prompt: quit |
| `ctrl+o` | open / close all tool runs |
| `ctrl+t` | open / close all thoughts (hidden: show them) |
| `ctrl+i` | VS Code on the thread's worktree (`⌘i`) |
| `pgup` `pgdn`, `shift+↑↓`, wheel | scroll the stream |
