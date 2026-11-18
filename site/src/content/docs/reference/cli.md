---
title: CLI
description: codehead's command line.
---

```text
codehead
codehead [-a agent[,agent…]] [-b branch] [-c] [-n] [title...]
codehead --view <thread>
codehead --cost <thread>
codehead ctl <thread> '<json>'
codehead --version
codehead --update
```

| Option | Effect |
| --- | --- |
| (0 options) | the thread view of the thread updated last; 0 threads: the start screen. `esc esc`: the start screen |
| `-a agent[,agent…]` | agents preselected for the new thread (default: the default agent) |
| `-b branch` | branch for a new worktree (default: `<prefix>/<title slug>`) |
| `-c` | copy the picked note paths to the clipboard, start nothing |
| `-n` | dry run: print what would start, start nothing |
| `title...` | prefills the title |
| `--view <thread>` | the thread view of a thread (id or a unique prefix) |
| `--cost <thread>` | the cost box of a thread, full screen |
| `ctl <thread> '<json>'` | 1 command to a thread's host, see [threads](/docs/concepts/threads/#scripting) |
| `-V`, `--version` | print the version |
| `--update` | install the latest release with the install script |
| `-h`, `--help` | full help, with every key |

`codehead host <thread>` and `codehead mcp` are codehead's own processes (the thread host, the
agents' MCP server). codehead starts them; you do not need to.

## Environment

| Variable | Effect |
| --- | --- |
| `CODEHEAD_CONFIG` | config file path (default `~/.config/codehead.toml`) |
| `CODEHEAD_NOTES`, `CODEHEAD_STYLES`, `CODEHEAD_AGENT`, `CODEHEAD_PREFIX`, `CODEHEAD_MODELS` | override the [config keys](/docs/reference/configuration/) |
| `CODEHEAD_HOME` | data folder (default `~/.local/share/codehead`) |
| `CODEHEAD_RUN` | socket folder (default `$XDG_RUNTIME_DIR/codehead`) |
| `CODEHEAD_ACP_<NAME>` | the ACP command of harness NAME (`CODEHEAD_ACP_PI="npx pi-acp"`) |
| `CODEHEAD_VSCODE` | VS Code binary for `ctrl+i` |
| `CODEHEAD_DRAFTS` | drafts folder (default `~/.cache/codehead/drafts`) |
| `CODEHEAD_NO_UPDATE_CHECK` | `1`: no daily release check |
| `CODEHEAD_KEYLOG` | file: log every key of the text inputs |
| `CODEHEAD_INSTALL_DIR` | install script target (default `~/.local/bin`) |
| `CLAUDE_CODE_ARTIFACT` | codehead sets `1` for its sessions unless set (Artifact tools on) |
