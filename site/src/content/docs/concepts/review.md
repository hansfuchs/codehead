---
title: Review
description: The changes view, the comments, the reviewer, the ship menu. Done means read.
---

Agents write fast. The changes view (`F2`, `alt+2`) makes the reading fast too: the diff sorted
by gravity, comments on the exact line, a file marked read when you read it. From 160 columns it
sits left of the chat, so you read while the agents work.

## Sorted by gravity

The branch's changes against its base (merge-base with `origin/<default>`), 1 box per file. A
classifier agent (setting `classify`) sorts the files into levels: **critical** (security,
money, important business logic), **high**, **medium**, **low**. It groups the files that do 1
thing, with a title and a why per group. Inside a level, the review order: schema / API, logic,
config, UI, docs, tests, generated (folded to 1 line). Without the classifier: path rules.

Side by side from 170 columns, unified below (`w` switches).

## Done means read

`v` marks a file reviewed: its box folds. If an agent touches it again, the box opens and shows
`updated`. The summary row counts what you read: `1/5 reviewed`. Marks live in
`threads/<id>/review.json`, per hash of the file's diff.

## Comments on the line

Select lines (`space`, click, drag) and press `c`: a comment on those lines, as on GitHub. Each
comment has an id (`c3`), an author and a state (open, resolved).

- `ctrl+enter` sends the open comments as 1 message to the lead: `id [severity] file:line by author: text`.
- `space` on a comment picks it for the next prompt only: "work on c3".
- Agents resolve the comments they addressed, with a note (MCP tool `comment_status`). A
  reviewer re-opens a comment whose fix does not hold.
- The panel (`ctrl+b`) lists the comments, open by severity, then resolved.

## The reviewer

`V` sends the diff to a reviewer model (setting `review`, default `claude:sonnet`): read-only
tools in the worktree, findings back as structured output. Each finding shows under its line
(`⚠ severity · title`). `y` takes a finding into your comment batch, `x` dismisses it.

## Lines, not files

- `s` stages only the selected lines; `a` / `u` stage or unstage a file or a hunk.
- `p` stashes the selected lines out of the worktree. The agent that wrote them names the
  entry; `↺` has the agent put them back at their place.
- `m` opens the commit message: `ctrl+g` drafts it from the staged diff.
- `u` in the panel uncommits unpushed commits: the files stay, the messages go back into the
  commit box.

## Ship

`S` opens the ship menu: only the steps valid now, each with its state. Push, rebase on the
base, push and open the pull request, merge when the checks pass. A failed CI run sends its log
to the agents. The merge view (`F3`) then shows the pull request: checks, comments, labels,
merge.

Every key: [keys → changes view](/docs/reference/keys/#changes-view).
