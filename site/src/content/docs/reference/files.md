---
title: Files
description: Every file codehead reads and writes.
---

| Path | Content |
| --- | --- |
| `~/.config/codehead.toml` | configuration |
| `~/.config/codehead/agents/<name>.toml` | 1 agent |
| `~/.local/share/codehead/threads/<id>/thread.json` | a thread: title, task, dir, agents, sessions |
| `~/.local/share/codehead/threads/<id>/events.jsonl` | everything that happened on the thread |
| `~/.local/share/codehead/limits.json` | plan usage windows, as the last session saw them |
| `~/.local/share/codehead/worktrees/<repo>/<branch>` | worktrees codehead made |
| `~/.local/share/codehead/logs/` | host logs (`host-<id>.log`), `view.log` |
| `~/.local/share/codehead/paste/` | pasted images and long texts |
| `$XDG_RUNTIME_DIR/codehead/<id>.sock` | a running host's socket |
| `~/.cache/codehead/drafts` | unsent tasks |
| `~/.cache/codehead/seen.json` | unread state of the thread cards |
| `~/.cache/codehead/release` | daily update check cache |
| `~/.claude/projects/…/<session>.jsonl` | Claude Code's own transcripts of the agents' sessions |

`$CODEHEAD_HOME` replaces `~/.local/share/codehead`, `$XDG_CONFIG_HOME` and
`$XDG_DATA_HOME` move their parts.
