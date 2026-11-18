---
title: Configuration
description: ~/.config/codehead.toml, key by key.
---

codehead reads `~/.config/codehead.toml` (`$CODEHEAD_CONFIG` overrides the path). Per key, the
environment variable wins over the file, the file over the default. The **settings** tab edits
the same file: `ctrl+s` saves, `ctrl+r` reloads, `ctrl+d` (or a click on `↺`) sets the field back
to its default. `esc` undoes the edit of the field (0 apply); `tab` / `shift+tab` walk the fields.

```toml
notes  = "~/notes"                    # $CODEHEAD_NOTES
styles = "~/styles"                   # $CODEHEAD_STYLES
home   = "~/home"                     # $CODEHEAD_HOME_REPO
agent  = "amon"                       # $CODEHEAD_AGENT
prefix = "task"                       # $CODEHEAD_PREFIX
models = "fable,opus,sonnet,haiku"    # $CODEHEAD_MODELS
messages = "bubbles"                  # $CODEHEAD_MESSAGES
thoughts = "hidden"                   # $CODEHEAD_THOUGHTS
edits = "diff"                       # $CODEHEAD_EDITS
rank = "claude:haiku"                # $CODEHEAD_RANK
review = "claude:sonnet"             # $CODEHEAD_REVIEW
editor = "auto"                      # $CODEHEAD_EDITOR

[keys]
menu    = "F2"                        # $CODEHEAD_MENU_KEY
command = ">"                         # $CODEHEAD_COMMAND_KEY

[colors]
scheme = "coral"                      # $CODEHEAD_SCHEME
accent = "#FF7565"                    # overrides 1 color of the scheme

[aliases]
"project:platform" = "~/projects/exampleorg/project/notes/platform/"
```

| Key | Default | Meaning |
| --- | --- | --- |
| `notes` | `notes/` of the app dir | notes root |
| `home` | unset | Home repo: a git repo with `AGENTS.md` (notes, other repos). Every agent gets its path in the system prompt and `$CODEHEAD_HOME_REPO`, so each harness reads the same rules |
| `styles` | `output-styles/` of the app dir | output styles folder |
| `agent` | `amon`, else the first agent | default agent |
| `prefix` | `task` | branch prefix for new branches |
| `models` | `fable,opus,sonnet,haiku` | Claude models in the agent form |
| `messages` | `bubbles` | Thread view messages: `bubbles` (boxes, your messages on the right) or `lines` (name badge, a bar in the agent's color) |
| `thoughts` | `hidden` | Agent thoughts in the thread view: `hidden`, `folded` (a click opens them) or `open` |
| `edits` | `diff` | What an agent gets told, before its next turn, of edits in its worktree it did not make: `off`, `files` (files and line ranges) or `diff` (also your diff, max. 150 lines) |
| `rank` | `claude:haiku` | Changes view order: `claude:<model>` groups the changed files (1 call per set of paths, about 1 cent with haiku; cached in `threads/<id>/rank.json`), `off`: path rules only |
| `review` | `claude:sonnet` | Changes view `V`: `claude:<model>` reviews the branch diff with read-only tools (Read, Grep, Glob) and returns findings (file, line, severity, title, body), kept in `threads/<id>/findings.json`; `off`: 0 reviews |
| `editor` | `auto` | The IDE of `ctrl+i` and the changes view `o`: `auto` (VS Code, VSCodium, Cursor, Zed, JetBrains: the 1st found), a command (`code`, `cursor`, `zed`, `idea` …), a path, or `terminal` ($VISUAL / $EDITOR full screen in codehead's terminal, at the line). Trust codehead's worktrees dir once in the IDE: every worktree under it is trusted |
| `[keys] menu` | `F2` | the menu key: `F1`-`F12`, `ctrl+<letter>`, `alt+<letter>` |
| `[keys] command` | `>` | the char that opens the command dropdown in a prompt: 1 symbol |
| `[colors] scheme` | `coral` | color preset: `coral` (cool grays, coral accent) or `amber` (warm paper, amber accent) |
| `[colors] <name>` | the scheme's | `#RRGGBB` for 1 color: `accent`, `on_accent`, `panel`, `hover`, `text`, `muted`, `line`, `ok`, `danger`, `warn` |
| `[aliases]` | none | short names for directories, see [aliases](/docs/concepts/notes/#path-aliases) |

Agents have their own files: see [agents](/docs/concepts/agents/).
