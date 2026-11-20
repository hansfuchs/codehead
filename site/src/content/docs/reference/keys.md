---
title: Keys
description: Every key, per screen.
---

`codehead --help` prints the full list. ⌘ keys need a terminal with the kitty keyboard protocol.
`F10` and `>` are the defaults of the settings `[keys] menu` and `[keys] command`. In the thread
view, `tab` opens the menu too (on forms, `tab` walks the fields).

## Everywhere

| Key | Action |
| --- | --- |
| `F10` (thread view: `tab`) | menu: quick commands (`n`: new thread), threads, tokens, agents (`←→`: tabs, `esc` / `tab`: close) |
| `tab` / `shift+tab` | next / previous field (a question: its choices ↔ the prompt) |
| `F1`-`F4`, `alt+1`-`alt+4` | start screen: the tabs (macOS: Option as Meta) |
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
| `r` / `R` | the **where** row: next / previous repo: the one codehead started in, then the home repo's other repos (its `AGENTS.md`); a repo not on disk gets cloned at the start |
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
| `!` (1st char of the prompt) | shell mode (`! ` with a space): `enter` runs the command (`$SHELL -c`, max. 120 s) in the shell dir; the thread shows its output and dir, your next message takes them to the agents |
| shell dir | the thread's checkout with the most line changes (uncommitted + commits since the thread began); your pick stays per thread. `cd DIR` alone (0 `&&` `;` `\|`) sets it, 0 command run |
| `↑` `↓` (shell mode) | the thread's shell commands, newest first, like a terminal; `↓` below the newest: back to your draft |
| `↓` (shell mode, on your draft) | edit the shell dir (`in …` below the prompt): a dropdown of its dirs as you type, `tab` completes, `enter` sets it, `esc` / `↑` back to the prompt |
| `ctrl+r` (shell mode) | search the thread's commands and your shell history (fzf, else a built-in fuzzy filter): the best match at the bottom, right above the prompt; `↑` / `ctrl+r` / `ctrl+p` worse matches, `↓` / `ctrl+n` better ones, `enter` / `tab` take it, `esc` closes |
| `↑` `↓` | a question open: its choices; else sent messages (an empty prompt too: shell commands only in shell mode) |
| a path (`src/`, `~/x/`, `/usr/`) | a dropdown of that dir's entries; a name with a space gets a `"` in front (a file: one after it too), the dropdown goes on inside the quote |
| `1`-`9`, `space` | a question: pick (several: toggle); letters: the **Other** answer |
| `←` `→` | a question with several parts: previous / next part |
| `esc` | back to live, else interrupt the running turns (after 0.5 s: a 2nd `esc` in time means back) |
| `esc esc` | back to the start screen |
| `ctrl+c` | clear the prompt; twice on an empty prompt: quit |
| `ctrl+o` | open / close all tool runs |
| `ctrl+t` | open / close all thoughts (hidden: show them) |
| `ctrl+i` | the IDE (setting `editor`) on the thread's worktree (`⌘i`): the window that has it open, else a new one |
| `pgup` `pgdn`, `shift+↑↓`, wheel | scroll the stream |
| `tab` / `shift+tab` | thread header under the view bar: the open threads (`←→` pick, `enter` open) |
| `alt+←` `alt+→` (`⌥←` `⌥→`) | previous / next view of the thread: chat, changes, merge, files (the view bar on the top row); word jumps in the prompt: `ctrl+←→` |
| `F1`-`F4`, `alt+1`-`alt+4` | chat / changes / merge / files view |
| `alt+\` | changes view: the chat on its right on / off (from 160 columns; on by default) |
| `ctrl+y` | yazi (full screen) on the worktree; a picked file goes into the prompt as its path |

## Changes view

The classifier agent (setting `classify`; empty: the default agent, with its harness and model;
Claude Code or Codex) sorts the files into levels: critical (security, money, important
business logic), high, medium, low. The same call groups the files that do 1 thing, in reading
order, with a title + why per group and a why per file. It runs in the background (`classifying…`),
1 call per set of paths (`threads/<id>/rank.json`); without it (or while it runs): path rules.

The branch's changes against its base (merge-base with `origin/<default>`; on the default
branch: uncommitted only), 1 box per file, by level, then in review order: schema / API, logic,
config, UI, docs, tests, generated (folded: 1 line). Side by side from 170 columns. A reviewed
box folds; if its diff changes later, it shows `updated`. The summary row and the filter (fuzzy,
by file name and diff content, best hit first) stay on top; a box's head (and its why) stays
under them until the box scrolls past.

The source control panel on the left (`ctrl+b`, from 90 columns): the commit message on top,
then staged, changes (by level, `U` `A` `M` `D` `R` per file), comments (by file) and the stash.
Labels out of view stick to the top / bottom edge; a click on one scrolls to it. Narrow (0 chat on
the left): the chat sits above the prompt (`t` or a click on its title row folds it). A message
from the changes view takes the selected lines and the open comments along: "stage only these",
"work on the comments".

The stash: lines or files out of the worktree, each with the patch that puts them back (not a git
stash). The agent that wrote the lines names the entry (MCP tool `stash_name`). `↺` restore: the
agent applies the patch at the lines' place (`git apply`, else `git apply -C1`) and asks you on any conflict (`stash_restored` ends
the entry). `✕` remove one, `✕ clear` all (both ask first).

| Key | Action |
| --- | --- |
| `j` / `k`, `ctrl+↑` / `ctrl+↓` | 1 file up / down (the box head; its border lights up) |
| `{` / `}` | previous / next box |
| `n` / `N` | next / previous hunk |
| `↑` `↓`, `pgup` `pgdn`, `g` `G` | move the cursor (`↑` `↓`: 1 line) |
| `space` | select the line on / off, the cursor goes 1 down (lines apart are fine); `esc` clears |
| click, drag | add the lines to the selection; `shift+click`: the lines from the last click |
| right click, right drag | take the lines out of the selection; 2 right clicks: clear it |
| `s` | stage only the selected lines (a part of each file: `git apply --cached`) |
| `p` | stash the selected lines (out of the worktree, into the panel's stash) |
| `r` | reviewed on / off (the cursor goes to the next box) |
| `←` / `→`, `enter` | fold / open, toggle the box |
| `f`, click | the filter: type, `enter` jumps to the 1st hit, `esc` clears |
| `e`, `o` | edit in the IDE (setting `editor`): at the 1st selected line, else at the line under the cursor (a removed line: where it was); the window that has the worktree open, else a new one (VS Code family, Zed, JetBrains); `flow` (flow-control.dev): a running instance with the worktree as a project gets the file (`--connect`), else a new GUI window (`--gui`); `terminal`: full screen in codehead's terminal, back on quit |
| `w` | side by side / unified |
| `ctrl+r` | read git again (it also does every 4 s) |
| `R` | classify again (setting `classify`) |
| `c` | comment on the selected lines (else the line under the cursor): a modal shows the lines per file (`alt+↑` `↓`, the wheel: scroll them), `⌘/ctrl+⏎` adds it to the batch, `esc` cancels |
| `enter`, click on a comment | edit it in the modal (0 text: deleted) |
| `x` | delete the comment (or dismiss the finding) under the cursor |
| `ctrl+enter` (`⌘enter`, `alt+enter`) | send all comments as 1 message to the lead (`file:line`: text each) |
| `a` / `u` | stage / unstage: on a box head the file, on a diff row its hunk (box head: `●` staged, `◐` a part) |
| `m` | the commit message (panel): `ctrl+g` drafts it from the staged diff (setting `rank` model), `⌘/ctrl+⏎` commits, `esc` leaves it |
| `ctrl+b` | the source control panel on / off |
| `alt` (alone: press, release) | the keys to the panel / the review (terminals with the kitty keyboard protocol) |
| `t` | the chat above the prompt: fold / open |
| `P` | push (`git push -u origin HEAD`) |
| `U` | fetch + rebase on the base (`origin/<default>`) |
| `A` / `K` / `X` | a rebase stopped on conflicts: the agent resolves / continue / abort |
| `V` | the reviewer (setting `review`) reads the diff: findings show under their line (`⚠ severity · title`) |
| `y` | on a finding: into the comment batch (`ctrl+enter` sends it) |
| `O` | ship: push + open the pull request (`gh pr create --fill`); the summary row shows `PR #n` and its checks `✓ ✗ ●` (read every 30 s) |
| `F` | CI failed: the latest failed run's log (last 150 lines) goes to the agents of your last message |
| `M` | merge the PR when its checks pass (`gh pr merge --squash --auto`) |
| `i` | the prompt gets the keys; `esc` gives them back |

In the panel:

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` `↓` | move the cursor |
| `←` / `→` | on a label: fold / open; `←` on a row: to its label; `→` on a file / comment: jump to it |
| `space`, `shift+click` | pick files |
| `s` | stage the picked files (staged section: unstage) |
| `p` | stash the picked files (whole) |
| `enter` | on a comment: edit it; on a file: jump to it, the keys to the review |
| `x` | delete the comment / remove the stash entry (asks first) |
| `r` | restore the stash entry (its agent) |
| `X` | clear the stash (asks first) |
| `m` | the commit message |

## Files view

The files the thread's agents wrote or edited (their Write / Edit calls) and the home repo's
notes of the thread (`thread: <id>` on line 1), newest first; a preview right (Markdown
rendered, code in syntax colors).

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` `↓` | pick a file |
| `space`, `pgup` `pgdn`, wheel | scroll the preview |
| `e`, `o` | edit the file in the IDE (its 1st changed line) |
| `y` | yazi on the file (full screen; codehead's own yazi config, themed from the palette) |
| `c` | copy the file's path |
| `r` | read the list again |
| `S` | share: commit the thread's home repo files (only those paths, message `notes: <title>`), push when the branch has an upstream |
| `i` | the prompt gets the keys; `esc` gives them back |
