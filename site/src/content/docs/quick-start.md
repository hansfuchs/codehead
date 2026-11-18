---
title: Quick start
description: From 0 to a running thread in a few minutes.
---

1. Install codehead:

   ```sh
   curl -fsSL https://codehead.sh/install | sh
   ```

2. Start it in a git repository:

   ```sh
   cd ~/projects/my-app
   codehead
   ```

3. Make your first agent on the **agents** tab: a name, the harness `claude` (or
   `claude/opus`), a mode. `autopilot` works without asking; `supervised` asks before each
   tool call. The agent is a file now: `~/.config/codehead/agents/<name>.toml`.

4. Back on the **new** tab, type a title, then the task in the prompt. `@` lists agents, `>`
   lists commands, `tab` moves between fields.

5. Pick where the work runs in the **where** row (`←` `→`): the current branch, another
   branch, a new branch, or a new branch with its own worktree.

6. Start the thread. The thread view opens: the agents on top, the stream in the middle, your
   prompt below. Send more with `ctrl+enter`.

`esc esc` goes back to the start screen. The thread keeps running: open it again from the
**threads** tab, or with `codehead --view <thread>`.
