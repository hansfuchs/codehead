# codehead

<p>
  <a href="https://github.com/hansfuchs/codehead/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/hansfuchs/codehead?color=ff7565"></a>
  <img alt="Linux and macOS" src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-150700">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-e3d2be"></a>
</p>

**Keep your head in the code.**

Write the task with your notes, pick named agents and a branch, then follow every message,
tool run and question of the thread, and steer it. codehead runs the agents itself, on the
harness you already use: Claude Code, Codex, OpenCode, Goose, Cursor, Pi, Oh My Pi, Devin,
Grok Build, Kimi Code, Amp, Hermes Agent or OpenClaw.

[Website](https://codehead.sh) · [Docs](https://codehead.sh/docs/) ·
[Quick start](https://codehead.sh/docs/quick-start/)

## Install

```sh
curl -fsSL https://codehead.sh/install | sh
```

Needs git and 1 harness on `PATH`, logged in
([the list](https://codehead.sh/docs/requirements/)). Update with `codehead --update`.
Every commit on `main` that passes CI is a release.

## How it works

- `codehead`: the screens (task form, threads, agents, settings, thread view).
- `codehead host <thread>`: 1 detached process per thread. It runs 1 harness session per
  agent (Claude Code: `claude -p`, stream-json; the others: Agent Client Protocol) and keeps
  going when the terminal closes.
- `codehead mcp`: the MCP server every agent gets: `send`, `set_status`, `list_agents`.
- Records: `~/.local/share/codehead/threads/<id>/{thread.json,events.jsonl}`, agents in
  `~/.config/codehead/agents/<name>.toml`.

See [threads](https://codehead.sh/docs/concepts/threads/) for the details.

## Development

```sh
just check     # clippy (warnings fail) + tests, like CI
just install   # release build, linked into ~/.local/bin
just site      # website dev server (site/, Astro)
just lab       # ASCII lab: moai mascot, 3D avatars, dither loaders (examples/lab)
```

- Rust toolchain: pinned in `rust-toolchain.toml`.
- Releases: `.github/workflows/release.yml` with `scripts/`. Version = `major.minor` of
  `Cargo.toml` + the commit count on `main`.
- Website: `site/`, deployed to Vercel (root directory `site`). The install script is
  `site/public/install.sh`, served at `https://codehead.sh/install`.

## License

[Apache-2.0](LICENSE).
