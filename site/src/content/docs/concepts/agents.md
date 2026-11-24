---
title: Agents
description: Named agents with a model, a mode, a style and a face.
---

An agent is a name, a [harness](/docs/requirements/#a-harness) and a profile, kept in 1 TOML
file: `~/.config/codehead/agents/<name>.toml`. The **agents** tab edits the same file; its
harness field takes `harness` or `harness/model` (`claude/opus`, `codex/gpt-5.5`, `opencode`).

```toml
harness = "claude"        # claude · codex · opencode · goose · cursor · pi · omp · devin · grok · kimi · amp · hermes · openclaw
model = "opus"            # an alias or a full model id; unset: Claude Code's default
effort = "high"           # low · medium · high · xhigh · max
mode = "autopilot"        # autopilot · edits · supervised · plan
color = "glacier"         # 1 of 23 pastels, see below (0 yellow: yellow is a blocked agent)
style = "mecha"           # an output style, see below
voice = "Short, dry, exact."
prompt = """
Run the tests before you report done.
"""
sprite = { id = "geob:22 speed=0.5", rows = ["⣀⣤⡴⣶⣵⣄⡀", "⣿⢀⠎⠘⡄⠉⣹", "⢿⡧⠤⠤⢵⣴⢻", " ⠙⢄⡴⠾⠛⠁"] }
skills = ["review"]       # optional: the skills it loads
command = "npx pi-acp"    # optional, ACP harnesses: another command
```

On an ACP harness, `model` goes to the agent as its model id (`session/set_model`), and the
agent's style, voice and instructions go in front of its 1st message.

## Modes

| Mode | Claude Code | ACP harnesses: the owner answers |
| --- | --- | --- |
| `autopilot` | `auto`: Claude Code's safety check flags the risky calls | only the guarded commands (below) |
| `edits` | `acceptEdits` | commands; reads and edits go through |
| `supervised` | `default` | everything but reads and searches |
| `plan` | `plan` | everything but reads and searches |

**Guard.** In autopilot, an ACP agent's command that is hard to take back waits for you:
recursive deletes outside the project, force pushes, hard resets, `sudo`, `curl | sh`,
publishing, dropped databases and the like.

Questions show up in the [thread view](/docs/concepts/thread-view/). codehead's own tools
(`send`, `set_status`, `list_agents`) never ask.

## Avatars

Each agent gets an animated 7×4 braille avatar, shown in the thread view, on the agents tab
and in the menu. It acts out the agent's status: it talks and blinks with its eyes while it
works, looks around while it monitors, flashes when it waits on you.

3 categories:

- **REGI** (`regi:<n>`, the default): a wireframe solid in 1-dot braille that floats (cube,
  slab, stele, capsule, gem, 4-point star, ellipse …), with eye dots in 1 of 12 patterns that
  talk and blink
- **GEO A** (`geo:<n>`): ray-marched solids, shaded, each with its own motion (tumble, snap,
  spin, pulse, orbit)
- **GEO B** (`geob:<n>`): the GEO A solids as outlines: only the silhouette and the folds

An id takes options: `regi:12 eyes=3 speed=0.5` (eye pattern, float speed), `geo:40 speed=2`.

On the agent form, `enter` on the avatar row opens the picker: `tab` switches the category,
`←→↑↓` picks, `s` previews a status, `f` sets the float speed, `e` the eye pattern (REGI),
`enter` takes it. When an agent appears in a thread, it spawns: it forms out of braille goo in
its color.

## Colors

An agent's color marks its avatar, its name and its messages. 23 pastels, picked to sit next to
the accent and the status hues of the [color scheme](/docs/reference/configuration/):

<p class="swatches">
<span style="--c:#b8a6ff">lavender</span> <span style="--c:#d6acf0">lilac</span>
<span style="--c:#98acf6">periwinkle</span> <span style="--c:#80beee">sky</span>
<span style="--c:#a0d0f0">glacier</span> <span style="--c:#b0dee6">ice</span>
<span style="--c:#7ed4ce">aqua</span> <span style="--c:#64bab0">teal</span>
<span style="--c:#a4e4c2">mint</span> <span style="--c:#76ca98">jade</span>
<span style="--c:#a2c494">sage</span> <span style="--c:#8ab870">fern</span>
<span style="--c:#b6ca7c">moss</span> <span style="--c:#cee492">lime</span>
<span style="--c:#e4ccaa">sand</span> <span style="--c:#f6b686">apricot</span>
<span style="--c:#faa690">peach</span> <span style="--c:#f48c86">coral</span>
<span style="--c:#ea789e">raspberry</span> <span style="--c:#f2a0bc">rose</span>
<span style="--c:#f8c2d2">blush</span> <span style="--c:#e696d6">orchid</span>
<span style="--c:#98a4be">slate</span>
</p>

0 yellow: yellow means an agent waits on you. On the agent form, `enter` on the color row
opens the color picker: every color as a swatch, the agent live in the color under the cursor
(`←→↑↓` pick, `enter` takes it, `esc` back). Old names still work: `red` → raspberry, `orange` → apricot, `green` →
sage, `blue` → glacier, `purple` → lavender, `gray` → slate. An unknown name: the accent.

## Output styles

An output style is a Markdown file (Claude Code format: frontmatter plus body) that sets how
an agent writes. codehead ships its styles in `output-styles/`; `styles` in the
[configuration](/docs/reference/configuration/) points at your own folder. A session gets its
agent's style, voice and instructions in its system prompt when it starts, so a changed style
file applies to the next session.

## Edits and live sessions

A session keeps the profile it started with. After an edit, codehead ends the agent's idle
sessions; the next message starts them again with the new profile and the conversation kept.
A changed harness starts fresh. Agents mid-turn keep going on the old profile until the turn
ends.

## Default agent

The default agent is preselected for new threads and leads threads of deleted agents: the
`agent` config key, else an agent named `amon`, else the first agent by name. Deleting an
agent archives its file: old threads still show its name.
