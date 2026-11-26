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
repo   = "~/projects/app"             # $CODEHEAD_REPO
agent  = "amon"                       # $CODEHEAD_AGENT
prefix = "task"                       # $CODEHEAD_PREFIX
models = "fable,opus,sonnet,haiku"    # $CODEHEAD_MODELS
messages = "bubbles"                  # $CODEHEAD_MESSAGES
thoughts = "hidden"                   # $CODEHEAD_THOUGHTS
edits = "diff"                       # $CODEHEAD_EDITS
classify = ""                         # $CODEHEAD_CLASSIFY (empty: the default agent)
rank = "claude:haiku"                # $CODEHEAD_RANK
review = "claude:sonnet"             # $CODEHEAD_REVIEW
editor = "auto"                      # $CODEHEAD_EDITOR

[keys]
menu    = "ctrl+p"                      # $CODEHEAD_MENU_KEY
command = ">"                         # $CODEHEAD_COMMAND_KEY

[colors]
scheme = "lavender"                   # $CODEHEAD_SCHEME
accent = "#B8A6FF"                    # overrides 1 color of the scheme

[aliases]
"project:platform" = "~/projects/exampleorg/project/notes/platform/"

[prices."gpt-6.1-sol"]                # $ per 1M tokens of a model
input  = 1.25
cached = 0.125                        # cache reads (default: input)
output = 10.0
# cache_write = 1.25                  # cache writes (default: input)
```

| Key | Default | Meaning |
| --- | --- | --- |
| `notes` | `notes/` of the app dir | notes root |
| `home` | unset | Home repo: a git repo with `AGENTS.md` (notes, other repos). Every agent gets its path in the system prompt and `$CODEHEAD_HOME_REPO`, so each harness reads the same rules |
| `repo` | unset | The repo the new tab's where rows start on, as if picked in the repo picker (a new branch + worktree first). Unset, or a dir in the repo codehead starts in: that repo, its current branch first. `-b` ignores it |
| `styles` | `output-styles/` of the app dir | output styles folder |
| `agent` | `amon`, else the first agent | default agent |
| `prefix` | `task` | branch prefix for new branches |
| `models` | `fable,opus,sonnet,haiku` | Claude models in the agent form |
| `messages` | `bubbles` | Thread view messages: `bubbles` (boxes, your messages on the right) or `lines` (name badge, a bar in the agent's color) |
| `thoughts` | `hidden` | Agent thoughts in the thread view: `hidden`, `folded` (a click opens them) or `open` |
| `edits` | `diff` | What an agent gets told, before its next turn, of edits in its worktree it did not make: `off`, `files` (files and line ranges) or `diff` (also your diff, max. 150 lines) |
| `classify` | the default agent | Changes view tabs and order: an agent (its harness and model; Claude Code or Codex) sorts the changed files into critical / high / medium / low and groups them (1 call per set of paths, in the background; cached in `threads/<id>/rank.json`), `off`: path rules only |
| `rank` | `claude:haiku` | Changes view: the model of that call when 0 agent is found (`classify` empty, 0 default agent); `off` with `classify` empty: path rules only |
| `review` | `claude:sonnet` | Changes view `V`: `claude:<model>` reviews the branch diff with read-only tools (Read, Grep, Glob) and returns findings (file, line, severity, title, body), kept in `threads/<id>/findings.json`; `off`: 0 reviews |
| `ports` | `auto` | Dev servers: `auto` (portless when it is on PATH), `portless`, or `block` (only the `$CODEHEAD_PORT` block). With portless, agents run `portless run <command>` and each worktree gets `https://<branch tail>.<app>.localhost` |
| `previews` | `auto` | Files view: images and a pdf's page 1 drawn as images. `auto`: the protocol from the terminal's `TERM` / `TERM_PROGRAM` (Ghostty, kitty: kitty; Rio, iTerm2, WezTerm: iterm2; foot, mlterm, contour: sixel; 0 in tmux, zellij, screen). `kitty`, `iterm2`, `sixel`: that protocol. `off`: text previews only. pdf: `pdftoppm` (poppler), else `sips` (macOS). Applies at once |
| `editor` | `auto` | The IDE of `e` (changes, files view) and menu → IDE on the worktree: `auto` (VS Code, VSCodium, Cursor, Zed, JetBrains: the 1st found), a command (`code`, `cursor`, `zed`, `idea` …), a path, or `terminal` ($VISUAL / $EDITOR full screen in codehead's terminal, at the line). Trust codehead's worktrees dir once in the IDE: every worktree under it is trusted |
| `[keys] menu` | `ctrl+p` | the menu key: `F1`-`F12`, `ctrl+<letter>`, `alt+<letter>` |
| `[keys] command` | `>` | the char that opens the command dropdown in a prompt: 1 symbol |
| `[colors] scheme` | `lavender` | color preset: `lavender` (violet grays, pastel lavender accent), `coral` (cool grays, coral accent) or `amber` (warm paper, amber accent) |
| `[colors] <name>` | the scheme's | `#RRGGBB` for 1 slot (base16 / catppuccin model): `base`, `surface0`-`surface2`, `overlay0`, `subtext0`, `text`, `accent`, `on_accent`, `red`, `peach`, `yellow`, `green`, `teal`, `sky`, `mauve`. Old names still work: `panel` → `surface0`, `hover` → `surface1`, `muted` → `subtext0`, `line` → `overlay0`, `ok` → `green`, `danger` → `red`, `warn` → `yellow` |
| `[aliases]` | none | short names for directories, see [aliases](/docs/concepts/notes/#path-aliases) |
| `[prices."<model>"]` | none | $ per 1M tokens of a model: `input`, `output` (required), `cached`, `cache_write` (default: `input`). Prices the turns of harnesses that report 0 $: Codex always, ACP agents unless their `usage_update` carries a USD cost. The model name as the harness reports it; `provider/model` also finds `model`. 0 entry: the turn costs 0 $. codehead ships 0 prices. Read when an agent's session starts; the settings tab leaves the table as it is |

Agents have their own files: see [agents](/docs/concepts/agents/).
