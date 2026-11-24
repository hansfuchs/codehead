---
title: Projects
description: Group threads by goal, give their agents a brief, let agents name the threads.
---

A **project** is a goal in a repo that takes more than 1 thread: `website refinement`,
`bug fixes`, 1 per feature. A thread belongs to 0, 1 or more projects of its repo. The thread
lists filter by project; the agents of a thread read the briefs of its projects.

## Briefs

1 project = 1 Markdown file, its **brief**: the goal, the decisions, the open points.

```markdown
---
title: Website refinement
---

Goal: the landing page says in 5 s what codehead does.

## Decisions

## Open
```

`done: true` in the front matter marks a project done: out of the pickers, its threads keep it.

Where the briefs live: the home repo, in the repo's dir there. That is the `Notes:` dir of the
repo's line in the home repo's `AGENTS.md`, else `<home repo>/<repo name>/`; the briefs go in
its `projects/`. 0 home repo: `~/.local/share/codehead/projects/<repo name>/`.

## The projects tab

`F3` (`alt+3`). The repo row (`←` `→`), the projects of that repo with their thread counts, the
brief of the project under the cursor.

| Key | Action |
| --- | --- |
| `n` | new project: a title field, a brief field (`tab` switches); `ctrl+s` makes the file |
| `enter` / `e` | edit title and brief here (`ctrl+s` save, `esc` close) |
| `x` | done / active |
| `d d` | delete the brief |
| `o` | open the brief in the IDE (`editor` setting) |
| `r` | read the briefs again |

Only you make projects. An agent never does.

## Assign

- **new** tab: the **project** row below the repo row (`←` `→` a project, `space` on / off).
  `auto` (the default): the agents pick.
- edit form of a thread (`e` on its card): the same picks; `auto` hands them back to the agents.
- the agents: once the work is clear, `set_projects` puts the thread in the fitting projects.
  Your picks stay: an agent changes only `auto` threads.

## Filter

The thread lists (threads tab, `ctrl+p` → threads) have a **project** row below the **repo**
row: `all`, each project (most threads first), `0 projects`. The card names its projects on its
bottom border.

The filter takes words too:

| Word | Threads |
| --- | --- |
| `project:web` | in a project whose slug starts with `web` |
| `kind:fix` | on a branch `fix/…` |
| `area:menu` | with a title `menu: …` |

## Names

The agents name the thread after its work with `set_title`: after the 1st turn, again when the
work changes direction.

- title: `area: outcome`, e.g. `menu: live quick commands`. A vague title (`misc. improvements
  #8`) gets refused.
- branch (a thread in its own worktree): `<kind>/<area>-<slug>`, e.g. `feat/menu-live-commands`.
  Kinds: `feat` `fix` `refactor` `perf` `docs` `test` `chore` `style` `ci` `build`. codehead
  renames the branch, also a pushed one: on GitHub with the rename API (open pull requests
  follow), elsewhere the new name gets pushed and the old one stays on the remote.
- a title you wrote (new tab, edit form) stays: the agents still name the branch.
