---
title: Configuration
description: ~/.config/codehead.toml, key by key.
---

codehead reads `~/.config/codehead.toml` (`$CODEHEAD_CONFIG` overrides the path). Per key, the
environment variable wins over the file, the file over the default. The **settings** tab edits
the same file: `ctrl+s` saves, `ctrl+r` reloads, `ctrl+d` (or a click on `↺`) sets the field back
to its default.

```toml
notes  = "~/notes"                    # $CODEHEAD_NOTES
styles = "~/styles"                   # $CODEHEAD_STYLES
agent  = "amon"                       # $CODEHEAD_AGENT
prefix = "task"                       # $CODEHEAD_PREFIX
models = "fable,opus,sonnet,haiku"    # $CODEHEAD_MODELS
messages = "bubbles"                  # $CODEHEAD_MESSAGES
thoughts = "hidden"                   # $CODEHEAD_THOUGHTS

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
| `styles` | `output-styles/` of the app dir | output styles folder |
| `agent` | `amon`, else the first agent | default agent |
| `prefix` | `task` | branch prefix for new branches |
| `models` | `fable,opus,sonnet,haiku` | Claude models in the agent form |
| `messages` | `bubbles` | Thread view messages: `bubbles` (boxes, your messages on the right) or `lines` (name badge, a bar in the agent's color) |
| `thoughts` | `hidden` | Agent thoughts in the thread view: `hidden`, `folded` (a click opens them) or `open` |
| `[keys] menu` | `F2` | the menu key: `F1`-`F12`, `ctrl+<letter>`, `alt+<letter>` |
| `[keys] command` | `>` | the char that opens the command dropdown in a prompt: 1 symbol |
| `[colors] scheme` | `coral` | color preset: `coral` (cool grays, coral accent) or `amber` (warm paper, amber accent) |
| `[colors] <name>` | the scheme's | `#RRGGBB` for 1 color: `accent`, `on_accent`, `panel`, `hover`, `text`, `muted`, `line`, `ok`, `danger`, `warn` |
| `[aliases]` | none | short names for directories, see [aliases](/docs/concepts/notes/#path-aliases) |

Agents have their own files: see [agents](/docs/concepts/agents/).
