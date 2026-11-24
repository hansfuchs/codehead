---
title: Requirements
description: What codehead needs on your machine.
---

## A harness

codehead's agents run on a **harness**: a coding agent's own CLI. Install at least 1, log it
in once, and keep it on your `PATH`. Claude Code runs as `claude -p` (stream-json); every
other harness runs over the [Agent Client Protocol](https://agentclientprotocol.com).

| Harness | Name | codehead runs | Install |
| --- | --- | --- | --- |
| Claude Code | `claude` | `claude -p` (stream-json) | `curl -fsSL https://claude.ai/install.sh \| bash` |
| Codex | `codex` | `codex-acp` | `npm i -g @openai/codex @zed-industries/codex-acp` |
| OpenCode | `opencode` | `opencode acp` | `npm i -g opencode-ai` |
| Goose | `goose` | `goose acp` | [goose-docs.ai](https://goose-docs.ai/docs/getting-started/installation/) |
| Cursor | `cursor` | `cursor-agent acp` | `curl https://cursor.com/install -fsS \| bash` |
| Pi | `pi` | `pi-acp` | `npm i -g @earendil-works/pi-coding-agent pi-acp` |
| Oh My Pi | `omp` | `omp acp` | [omp.sh](https://omp.sh) |
| Devin | `devin` | `devin acp` | [docs.devin.ai/cli](https://docs.devin.ai/cli) |
| Grok Build | `grok` | `grok agent stdio` | [docs.x.ai](https://docs.x.ai/build/overview) |
| Kimi Code | `kimi` | `kimi acp` | [kimi.ai](https://kimi.ai/download) |
| Amp | `amp` | `amp-acp` | `npm i -g @sourcegraph/amp amp-acp` |
| Hermes Agent | `hermes` | `hermes-acp` | [hermes-agent.nousresearch.com](https://hermes-agent.nousresearch.com) |
| OpenClaw | `openclaw` | `openclaw acp` | [docs.openclaw.ai](https://docs.openclaw.ai/start/getting-started) |

Another command for a harness: `$CODEHEAD_ACP_<NAME>` (`CODEHEAD_ACP_PI="npx pi-acp"`), or
`command` in the agent's file. The agent detail on the **agents** tab shows whether its harness
is installed and the command codehead runs.

## System

- **OS**: Linux or macOS, x86_64 or arm64.
- **git**: for branches and worktrees.
- **Terminal**: true color. The kitty keyboard protocol (kitty, Ghostty, WezTerm, foot, …)
  enables ⌘ shortcuts, `ctrl+enter` and undo / redo keys.

## Optional

- **Clipboard** (Linux): `wl-clipboard` on Wayland, `xclip` or `xsel` on X11.
- **Editor**: `$VISUAL` or `$EDITOR` (default `nvim`, else `vi`), to edit notes.
- **IDE**: VS Code, VSCodium, Cursor, Zed or a JetBrains IDE (setting `editor`), for `e` (changes,
  files view) and menu → IDE on the worktree.
- **portless** (Node 24+): dev server URLs per worktree; the install script installs it (`npm -g`)
  and runs `portless trust` once (its local CA in the OS trust store; may ask for your password).
- **yazi**: the file browser (menu → yazi, files view `ctrl+.`); the install script installs it (brew,
  else `cargo binstall`).
- **claude** (Claude Code), logged in: the changes view order (`rank`), the reviewer (`review`),
  commit message drafts and `> handoff`, also when the agents run on other harnesses.
- **gh**, logged in: pull requests and CI in the changes view (`O`, `F`, `M`); the install
  script checks each release's build provenance with it too.
