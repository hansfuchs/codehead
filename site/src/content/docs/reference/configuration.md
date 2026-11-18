---
title: Configuration
description: ~/.config/codehead.toml, key by key.
---

codehead reads `~/.config/codehead.toml` (`$CODEHEAD_CONFIG` overrides the path). Per key, the
environment variable wins over the file, the file over the default. The **settings** tab edits
the same file: `ctrl+s` saves, `ctrl+r` reloads.

```toml
notes  = "~/notes"                    # $CODEHEAD_NOTES
styles = "~/styles"                   # $CODEHEAD_STYLES
agent  = "amon"                       # $CODEHEAD_AGENT
prefix = "task"                       # $CODEHEAD_PREFIX
models = "fable,opus,sonnet,haiku"    # $CODEHEAD_MODELS

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
| `[aliases]` | none | short names for directories, see [aliases](/docs/concepts/notes/#path-aliases) |

Agents have their own files: see [agents](/docs/concepts/agents/).
