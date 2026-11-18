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
| `alt+1`-`alt+4` | start screen: new, threads, agents, settings tab (macOS: Option as Meta) |
| `ctrl+←` `ctrl+→`, `alt+←` `alt+→` | word jumps in a text field |
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
| `tab` | thread header: the open threads (`←→` pick, `enter` open) |
| `shift+tab` | next view of the thread: chat → changes |
| `alt+1` / `alt+2` | chat / changes view |

## Changes view

The `rank` model (default `claude:haiku`) groups the files that do 1 thing, in reading order, with a title + why per group and a why per file; without it (or while it runs): path rules.

The branch's changes against its base (merge-base with `origin/<default>`; on the default
branch: uncommitted only), 1 box per file, in review order: schema / API, logic, config, UI,
docs, tests, generated (folded). Side by side from 170 columns. A reviewed box folds; if its
diff changes later, it shows `updated`.

| Key | Action |
| --- | --- |
| `j` / `k` | next / previous box |
| `n` / `p` | next / previous hunk |
| `↑` `↓`, `pgup` `pgdn`, `g` `G` | move the cursor |
| `space` | reviewed on / off (the cursor goes to the next box) |
| `enter` | fold / unfold the box |
| `o` | VS Code at the line under the cursor (the window that has the worktree open) |
| `s` | side by side / unified |
| `r` | read git again (it also does every 4 s) |
| `R` | rank again (setting `rank`) |
| `c` | comment on the line under the cursor: the prompt takes it, `⌘/ctrl+⏎` adds it to the batch, `esc` cancels (the draft comes back) |
| `x` | delete the comment under the cursor |
| `C` | send all comments as 1 message to the lead (`file:line`: text each) |
| `a` / `u` | stage / unstage the file under the cursor (box head: `●` staged, `◐` a part) |
| `m` | commit message in the prompt: `ctrl+g` drafts it from the staged diff (setting `rank` model), `⌘/ctrl+⏎` commits, `esc` cancels |
| `P` | push (`git push -u origin HEAD`) |
| `U` | fetch + rebase on the base (`origin/<default>`) |
| `A` / `K` / `X` | a rebase stopped on conflicts: the agent resolves / continue / abort |
| `i` | the prompt gets the keys; `esc` gives them back |
