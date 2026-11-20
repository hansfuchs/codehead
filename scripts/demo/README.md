# demo videos

Recordings of the real codehead TUI for the website: scripted keys, fake harnesses, 0 model calls.

```sh
just demo                  # every recording → site/public/demo/<name>.mp4
just demo stream ask       # some
```

The videos come from Markdown scripts in `recordings/` (format: `recordings/README.md`):
seeded threads, the turns the fake harnesses play, the keys of the owner. Change a script,
record again.

## How it works

1. `fixture.py` builds a demo world: a home folder with config, 5 agents, notes, the git
   repository of `payd` (a payments service) with a remote, branches and a worktree, seeded
   threads, and fake harnesses on `PATH`.
2. `record.py` runs codehead in a pseudo terminal inside a user + mount + pid namespace, with
   the world bind-mounted on `/home`: every path on screen reads `/home/dev/...`. It types
   like a person, sends kitty-protocol keys and mouse events, answers the terminal queries
   (palette, DA1, cursor), and writes an asciicast (`.cast`).
3. `script.py` parses the recordings; `demo.py` runs them (`--list`, `--explore <name>`).
4. `fake_agent.py` is every harness: `claude` speaks Claude Code's stream-json (partial
   messages, thinking, tool calls, `can_use_tool` asks, rate limits, results with cost), all
   other names speak ACP. It plays the `# live` turns of the recording; `send` and
   `set_status` go to the host with `codehead ctl`, as the MCP server does.
5. `render.py` replays the cast into a pyte screen and draws each frame in JetBrains Mono;
   box drawing, blocks and braille are drawn as shapes, like Ghostty does. Pointer overlay
   for clicks. Output: H.264 MP4 + poster PNG.

## Size

1920x1080: 1 quarter tile of a 3840x2160 monitor. JetBrains Mono 17 px (Ghostty's default
size at scale 1), cell 10x22 px, 190x48 cells.

## Needs

- `ffmpeg`, `unshare` (util-linux), Python 3 with `pyte`, `pillow`, `fonttools` in
  `~/.cache/codehead-demo/venv`
- JetBrains Mono in `~/.cache/codehead-demo/jbm/fonts/ttf/`
- a release build: `cargo build --release`

## Recordings

`demo.py --list` prints them with their captions.
