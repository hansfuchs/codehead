---
title: Keys
description: Every key, per screen.
---

`codehead --help` prints the full list. ⌘ keys need a terminal with the kitty keyboard protocol.
`ctrl+p` and `>` are the defaults of the settings `[keys] menu` and `[keys] command` (a key
codehead binds to something else is refused). `tab` walks the fields on every screen; `←→` walk a
row of tabs.

Rules on every screen:

- 1 key, 1 meaning. A letter key never acts with `ctrl` / `alt` / `⌘` held (`alt+P` never pushes).
- `esc` goes 1 level back. It never saves. Edited text goes only on a 2nd `esc` within 2 s.
- Outward or destructive keys ask first: a 2nd press (`d d`, `S S`, `ctrl+d ctrl+d`, `enter enter`)
  or a yes / no box.
- `ctrl+f` filters a list (the only key to a filter; in the chat: the search bar), `r` reads it again, `ctrl+d` puts a thing back to its default.
- `q` never quits.

## Everywhere

| Key | Action |
| --- | --- |
| `ctrl+p` | menu, from every field: quick commands (`n`: new thread, `f`: search; in a thread per agent: its flow, remove, replace with), threads, merge, search, tokens (`tab` / `shift+tab`: tabs, `←→` too outside threads and merge, where they step the repos / states; `esc` / `ctrl+p`: close) |
| `ctrl+n` | notifications, from everywhere: questions, blocked agents, flow gates, errors, usage limits, PR conflicts / failed checks / comments, `@owner` mentions, context ≥ 90 %, threads done (`↑↓` move, `enter` jump to the event, `x` mark seen, `esc` close). The header shows the counts; a note goes once its event was on screen for 1 s |
| `tab` / `shift+tab` | next / previous field (a question: its choices ↔ the prompt; a view: the view ↔ the prompt; a row of tabs: into its content) |
| `F1`-`F7`, `alt+1`-`alt+7` | start screen: the tabs new, threads, projects, merge, agents, flows, settings (macOS: Option as Meta) |
| `ctrl+←` `ctrl+→`, `alt+←` `alt+→` | word jumps in a text field |
| `esc` | 1 level back: never saves, never drops edits unasked (edited text: `esc` again within 2 s) |
| `ctrl+z` / `ctrl+shift+z`, `ctrl+y` | undo / redo in any text field (`⌘z` / `⌘⇧z`) |
| `ctrl+c` | the selection: copy; else the field's text: cleared (`ctrl+z` brings it back); else 2× within 1 s: quit |
| `ctrl+m` | thread view: the prompt has the keys again, from every pane, box, filter, dropdown and the menu (kitty keyboard protocol; a legacy terminal sends `ctrl+m` as `enter`) |

## Start screen

| Key | Action |
| --- | --- |
| `esc esc` | quit (the task stays as a draft); menu → quit codehead too |
| `ctrl+s` | new tab: save the task as a draft for later, start over |
| `d d` | drafts dropdown: delete the draft |
| menu → reset | new tab: reset the where row, notes and context (asks first) |
| `@` / `>` | agents (cards by harness) / commands dropdown in the prompt |
| `f` | agents field: the flow of the card under the cursor (the flows, `→` its steps: the start step); the card shows it under its effort row. A flow picks the agent. The repo's last flow: the lead's default |
| `←` `→` | the **where** row: branch or worktree |
| `enter` | the **repo** row: the repo picker (a repo not on disk gets cloned at the start) |
| `←` `→` | threads tab: the previous / next repo (menu → filter by repo: the dropdown); on the **project** row: the previous / next project |
| `←` `→` · `space` | the **project** row of the new tab: a project · on / off (`auto`: the agents pick) |
| `n` · `enter` · `x` · `d d` · `o` | projects tab: new · edit the brief · done / active · delete · open in the IDE ([Projects](/docs/concepts/projects/)) |
| `ctrl+v` | paste a clipboard image |
| `ctrl+s` / `ctrl+r` | settings: save / reload |
| `esc` | settings: undo the field's edit (0 apply); agent form with edits: asks first |
| `enter` on **+ add** (menu: add path alias) / `d`, `ctrl+d` | settings: add / remove an alias |
| `ctrl+d`, click `↺` | settings: the field back to its default |
| `ctrl+f` | settings → mcp servers, notes, threads, agents, flows: filter |
| `n` · `r` · `d d` | settings → shortcuts: new · rename · delete; `enter` edits the text, `ctrl+s` saves |

## Thread view

| Key | Action |
| --- | --- |
| `ctrl+enter` | send (`⌘enter`, `alt+enter`); `ctrl+s` never sends |
| `enter` | new line; a question open: answer |
| `!` (1st char of the prompt) | shell mode (`! ` with a space): `enter` runs the command (`$SHELL -c`, max. 120 s) in the shell dir; the thread shows its output and dir, your next message takes them to the agents |
| shell dir | the thread's checkout with the most line changes (uncommitted + commits since the thread began); your pick stays per thread. `cd DIR` alone (0 `&&` `;` `\|`) sets it, 0 command run |
| `↑` `↓` (shell mode) | the thread's shell commands, newest first, like a terminal; `↓` below the newest: back to your draft |
| `↓` (shell mode, on your draft) | edit the shell dir (`in …` below the prompt): a dropdown of its dirs as you type, `tab` completes, `enter` sets it, `esc` / `↑` back to the prompt |
| `ctrl+r` (shell mode) | search the thread's commands and your shell history (fzf, else a built-in fuzzy filter): the best match at the bottom, right above the prompt; `↑` / `ctrl+r` worse matches, `↓` better ones, `enter` / `tab` take it, `esc` closes |
| `↑` `↓` | a question open: its choices; else sent messages (an empty prompt too: shell commands only in shell mode) |
| a path (`src/`, `~/x/`, `/usr/`) | a dropdown of that dir's entries; a name with a space gets a `"` in front (a file: one after it too), the dropdown goes on inside the quote |
| `#` | a dropdown of your [shortcuts](/docs/concepts/shortcuts/), a 2nd box with the text: `↑` `↓` preview it in place, `tab` / `enter` take the text; a `#name` left in the prompt expands on send |
| `>sub <task>` | a [sub-thread](/docs/concepts/threads/#sub-threads) does the task (0 history, same checkout); the thread waits for its result. Sub-threads row: a click opens the box, `d` stops one |
| `1`-`9`, `space` | a question: pick (several: toggle); letters: the **Other** answer |
| a new question | takes the keys only on an empty prompt after 1 s of 0 keys; else it waits for `tab` (0 typed key answers it by surprise). `tab` with an `@` / `#` / path dropdown open: completes there |
| `←` `→` | a question with several parts: previous / next part |
| `esc` | back to live, else interrupt the running turns (after 0.5 s: a 2nd `esc` in time means back) |
| `esc esc` | back to the start screen |
| `ctrl+c` | the prompt's selection: copy; else clear the prompt (`ctrl+z`: back); twice on an empty prompt: quit |
| `ctrl+o` | open / close all tool runs |
| `ctrl+t` | open / close all thoughts (hidden: show them) |
| `ctrl+i` | the IDE (setting `editor`) on the thread's worktree (`⌘i`): the window that has it open, else a new one |
| `pgup` `pgdn`, `shift+↑↓`, wheel | scroll the stream |
| `F1`-`F4`, `ctrl+1`-`ctrl+4`, `alt+1`-`alt+4` | chat / changes / merge / files view (the view bar on the top row) |
| `alt+\` | changes view: the chat on its right on / off (from 160 columns; on by default) |
| `ctrl+f` | the search bar above the prompt: the chat's messages, notes, shell, tool calls; `tab` / `enter` / `↑` the next older match, `shift+tab` / `↓` the next newer one (both wrap), `esc` closes. Another view: its filter |
| `ctrl+q` | the agents box above the prompt has the keys: `←` `→` an agent, its menu above it: `flow` (that agent's flow box), `remove` (`enter` twice), `replace with` (an agent off the thread takes over, its flow too); `1`-`3` pick a row, `esc` / `ctrl+m` back to the prompt |
| agents box → flow, `> flow` | an agent's flow box: the steps (`s` skip, `j` jump, `c` change, `e` edit, `x x` remove); 0 flow: the flows to pick. `> flow`: the agent a message goes to |
| a flow wait | a gate (approve or an outcome), a loop at its max (1 more round, move on): a question in the picker under the stream, after the agents' questions. It never takes the keys: write in the prompt, `ctrl+enter` sends it as a message (the wait stays); `tab` then `1`-`9` / `↑↓` `enter` answers, the prompt's text goes along as your note. An outcome back to a step needs that note; `reply` sends the prompt's text, the wait stays |
| menu → yazi, files view `y` | yazi (full screen) on the worktree; a picked file goes into the prompt as its path |
| `d d` | subagents / monitors box: stop the subagent / end the watch; another key closes the box and acts as usual |

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
then staged, changes (the uncommitted files, by level, `U` `A` `M` `D` `R` per file), comments
(open by severity, resolved), the stash and the commits.
Labels out of view stick to the top / bottom edge; a click on one scrolls to it. Narrow (0 chat on
the left): the chat sits above the prompt (`t` or a click on its title row folds it). A message
from the changes view takes the selected lines and the picked comments (`space` on a comment) along,
once: "stage only these", "work on c3". 0 comment goes along unpicked; an agent reads all of them
with the MCP tool `comments` when you ask it to.

Comments, as on GitHub: each has an id (`c3`), an author (`you`, `@REGI`) and a state, open or
resolved. Agents resolve the comments they addressed (MCP tool `comment_status`, with a note) and
a reviewer re-opens one whose fix does not hold. A comment on a file with 0 uncommitted changes
(committed or reverted) resolves itself (`✓ @codehead`). A resolved comment leaves the file
view; the panel's RESOLVED lists it with who resolved it (`✓ @REGI`). The panel lists the
comments of the checkout in view; the open comments of another checkout (an agent's linked
worktree) are 1 row, `↗ 3 open in <worktree>`: `enter` goes there.

The stash: lines or files out of the worktree, each with the patch that puts them back (not a git
stash). The agent that wrote the lines names the entry (MCP tool `stash_name`). `↺` restore: the
agent applies the patch at the lines' place (`git apply`, else `git apply -C1`) and asks you on any conflict (`stash_restored` ends
the entry). `✕` remove one, `✕ clear` all (both ask first).

The commits section at the bottom of the panel: "all changes" (a branch: its changes against
`origin/<default>`, commits + uncommitted, in the review with comments and reviewed marks; 0 git
step), then the branch's commits, unpushed first (`●`, not on
the branch of the same name: `@{u}` when it has that name, else `origin/<branch>`), then pushed (`○`), newest first, the 50 newest at
most. `enter` / click on a commit: its files in the review (`◆`), read only: moving, folding, the
filter, `w`; 0 IDE (the worktree file may differ), comment, reviewed mark or git step; `esc` brings the changes back.
The shown commit lists its files under it in the panel (`A` `M` `D` `R`; the review's file in
accent): `enter` / click / `→` on one puts the review on its box (`enter`: the keys go there); the
files hide again when the commit is no longer shown (`esc` in the review, another commit, `←` on
the commit). `u`
uncommits the picked unpushed commits (else the commit under the cursor): they leave the history,
the later commits get replayed on top, the files stay as they are (their changes show uncommitted,
the staged changes stay staged), the commits' messages go into the empty commit message box. A
later commit's change that conflicts with the change taken out leaves the history too (the toast
names each file). Pushed commits, merge commits in the range and a stopped rebase: 0 uncommit.
The old HEAD stays at `refs/codehead/uncommit/<thread>` (with a reflog: older uncommits too); `git reset --mixed` on it undoes it (the staged changes go unstaged).

| Key | Action |
| --- | --- |
| `j` / `k`, `ctrl+↓` / `ctrl+↑` | 1 file down / up (the box head; its border lights up) |
| `n` / `N` | next / previous hunk |
| `↑` `↓`, `pgup` `pgdn`, `g` `G` | move the cursor (`↑` `↓`: 1 line) |
| `space` | select the line on / off, the cursor goes 1 down (lines apart are fine); `esc` clears |
| click, drag | add the lines to the selection; `shift+click`: the lines from the last click |
| right click, right drag | take the lines out of the selection; 2 right clicks: clear it |
| `s` | stage only the selected lines (a part of each file: `git apply --cached`) |
| `p` | stash the selected lines (out of the worktree, into the panel's stash) |
| `v` | reviewed on / off (the cursor goes to the next box) |
| `←` / `→`, `enter` | fold / open, toggle the box |
| `ctrl+f`, click | the filter: type, `enter` jumps to the 1st hit, `esc` clears |
| `e`, `o` | edit in the IDE (setting `editor`): at the 1st selected line, else at the line under the cursor (a removed line: where it was); the window that has the worktree open, else a new one (VS Code family, Zed, JetBrains); `flow` (flow-control.dev): a running instance with the worktree as a project gets the file (`--connect`), else a new GUI window (`--gui`); `terminal`: full screen in codehead's terminal, back on quit |
| `w` | side by side / unified |
| `r` (`ctrl+r`) | read git again (it also does every 4 s) |
| `R` | classify again (setting `classify`) |
| `c` | comment on the selected lines (else the line under the cursor): a modal shows the lines per file (`alt+↑` `↓`, the wheel: scroll them), `⌘/ctrl+⏎` adds it to the batch, `esc` closes (typed text: `esc` again within 2 s) |
| `enter`, click on a comment | edit it in the modal (0 text: deleted) |
| `x` | delete the comment (or dismiss the finding) under the cursor |
| `space` on a comment | pick it: the next prompt takes it along (once) |
| `d` | the comment under the cursor: resolved (it leaves the file view) |
| `C` | every comment: 1 row (`+n lines`) / its whole text |
| `ctrl+enter` (`⌘enter`, `alt+enter`) | send the open comments not sent yet as 1 message to the lead (`id [severity] file:line by author`: text each); they stay open until resolved |
| `a` / `u` | stage / unstage: on a box head the file, on a diff row its hunk (box head: `●` staged, `◐` a part) |
| `m` | the commit message (panel): `ctrl+g` drafts it from the staged diff (setting `rank` model), `⌘/ctrl+⏎` commits (the keys go back to the review: `P` / `O` next), `esc` leaves it |
| `ctrl+b` | the source control panel: hidden → shown with the keys; shown, keys in the review → the keys to the panel; the panel with the keys → hidden (any terminal) |
| `ctrl+w`, `shift+←→` | the next checkout / the repo tabs (several checkouts) |
| `alt` (alone: press, release) | the keys to the panel / the review (terminals with the kitty keyboard protocol) |
| `t` | the chat above the prompt: fold / open |
| `S` | the ship menu (review and panel): the steps valid now, each with its state; `↑↓` `j` `k`, `enter` runs, an outward step asks first (`enter enter`), `esc` closes. Steps: push (`git push -u origin HEAD`, `n ahead`) · push up to the newest picked commit (its older unpushed commits go along; a question lists them first) · fetch + rebase on the base (`origin/<default>`, `n behind`) · push + open the pull request (`gh pr create --fill`) · merge the PR when its checks pass (`gh pr merge --squash --auto`, `✓ ✗ ●`) · CI failed: the latest failed run's log (last 150 lines) to the agents of your last message. A rebase stopped on conflicts: only its steps: the agents resolve · continue · abort |
| `V` | the reviewer (setting `review`) reads the diff: findings show under their line (`⚠ severity · title`) |
| `y` | on a finding: into the comment batch (`ctrl+enter` sends it) |
| `i`, `tab` | the prompt gets the keys; `esc` / `tab` gives them back |

In the panel:

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` `↓` | move the cursor |
| `←` / `→` | on a label: fold / open; `←` on a row: to its label; `→` on a file / comment: jump to it; on a commit: show it, the keys stay; `←` on the shown commit: hide it; on a commit's file: show it, `←`: to its commit |
| `[` / `]` | the previous / next section |
| `space`, `shift+click` | pick files, unpushed commits, comments (comments: for the next prompt) |
| `d` | on a comment: resolved / open again |
| `u` | uncommit the picked commits (else the commit under the cursor; asks first) |
| `s` | stage the picked files (staged section: unstage) |
| `p` | stash the picked files (whole) |
| `enter` | on a comment: edit it; on a file: jump to it, the keys to the review; on a commit: its files in the review (read only, `esc`: back) |
| `x` | delete the comment / remove the stash entry (the stash entry asks first) |
| `enter` (stash entry) | restore the stash entry (its agent) |
| `X` | clear the stash (asks first) |
| `m` | the commit message |

## Files view

The files the thread's agents wrote or edited (their Write / Edit calls), the home repo's
notes of the thread (`thread: <id>` on line 1) and, last, the attachments of your messages
(`[Image #1] image-1.png`), newest first; a preview right (Markdown rendered, code in syntax
colors). Images (png, jpeg, gif, webp, heic) and a pdf's page 1 are drawn as images in
Ghostty, kitty, Rio, iTerm2, WezTerm and sixel terminals (setting `previews`; 0 in tmux, zellij, screen). A
click on an attachment's marker or `📎` row in the chat opens this view on it.

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` `↓` | pick a file |
| `ctrl+f` | filter (fuzzy, names and content) |
| `space`, `pgup` `pgdn`, wheel, `g` | scroll the preview (`g`: its top) |
| `e`, `o` | edit the file in the IDE (its 1st changed line) |
| `y` | yazi on the file (0 files: on the worktree; full screen; codehead's own yazi config, themed from the palette) |
| `c` | copy the file's path |
| `r` | read the list again |
| `S S` | share: commit the thread's home repo files (only those paths, message `notes: <title>`), push when the branch has an upstream |
| `i`, `tab` | the prompt gets the keys; `esc` / `tab` gives them back |

## Merge view

The thread's pull requests (`alt+3`), or a pull request of the start screen's merge tab (`F4`).
An overview of stops (title, body, status, labels, update, merge, checks, comments) and the
PR's diff (the changes view, read only).

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` `↓`, `g` `G`, `pgup` `pgdn` | the previous / next stop |
| `←` `→` | along a row (PR tabs, views, checks) |
| `enter` | act on the stop: edit the title / body, the status menu, add a label, a comment |
| `enter enter` | remove a label · the agents update the branch · merge (when ready: auto-merge) · auto-merge off · status → close |
| `ctrl+enter` | open the stop on GitHub |
| `[` `]` · `<` `>` | the previous / next PR · overview / diff (typed as text in a filter or a comment) |
| `r` | read the PR again (the diff: `ctrl+r`) |
| `esc` | an edit: close (edited text: `esc` again within 2 s); the start screen: back to the list |
| `i`, `tab` | the prompt gets the keys |
