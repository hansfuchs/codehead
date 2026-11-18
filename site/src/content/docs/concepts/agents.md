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
color = "blue"
style = "mecha"           # an output style, see below
voice = "Short, dry, exact."
prompt = """
Run the tests before you report done.
"""
sprite = { id = "regi-008", rows = ["╭─────╮", "│⢀⢀⢐⢀⢀│", "│  ⢐  │", "╰─────╯"] }
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

## Sprites

Each agent gets a 7×4 glyph portrait, shown in the thread view and on the agents tab:

<pre class="sprite">╭─────╮
│⢀⢀⢐⢀⢀│
│  ⢐  │
╰─────╯</pre>

The agent form picks a sprite set, then a sprite, or you draw your own in `$EDITOR`.

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
