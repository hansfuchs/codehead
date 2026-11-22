---
title: Flows
description: The steps an agent of a thread takes for a kind of task, with gates and loops.
---

A **flow** is the order of steps for a kind of task: new feature, bug fix, refactor, ...
A flow belongs to 1 agent of a thread: give an agent a flow, and it gets 1 step at a time. Two
agents of a thread can run different flows (a builder on `new feature`, a reviewer on
`code review`). Each step says what to do and when it is done. A **gate** holds the flow until
you approve. A **loop** goes back until its condition holds.

codehead ships 10 flows: new feature, bug fix, bug investigation, refactor, concept,
performance, migration / upgrade, incident / hotfix, code review, chore.

## Steps

Flows are built from **steps**. A step is shared: edit it once, and each flow that uses it
changes. A flow can also hold a **one-off** step, with text for that flow only; `s` on the rail
saves a one-off as a shared step. Every step of a flow runs on the flow's agent.

```markdown
---
description: another agent reviews the diff
done: the reviewer's findings came back (or 0 findings)
needs: [skill:code-review]
---
Get a review of the full diff of this thread from another agent ...
```

- `gate: true`: the flow waits for your approval after the step.
- `done`: the exit criteria the agent checks.
- `needs`: `skill:<name>`, `mcp:<server>`, `harness:<name>`. The flow's agent lacks one: the
  thread shows a warning.
- the text: `#shortcuts` expand when the step starts.

A step that needs another agent says so in its text: `peer-review` has the agent ask a reviewer
with `send` and wait for the reply.

## Flows

```toml
name = "new feature"

[[step]]
id = "plan"
use = "plan"

[[step]]
use = "owner-gate"
label = "plan approval"
gate = { changes = "plan" }      # outcomes besides approve: outcome = step id

[[step]]
id = "review"
use = "peer-review"

[[step]]
use = "address-review"
loop = { to = "review", until = "0 findings", max = 3 }

[[step]]
text = "Update CHANGELOG.md"     # a one-off step
```

Per step: `use` (a shared step) or `text` (a one-off), `id`, `label`, `note` (text for this
flow after the step's text), `done`, `gate`, `loop`. A loop at its `max` waits for you: 1 more
round, or move on.

Roles are gone: a `role` / `roles` key of an older file gives a warning and has 0 effect; the
next save drops it.

## Files

```
built-in                     in the codehead binary
~/.config/codehead/steps/    <name>.md      user steps; an edited built-in
~/.config/codehead/flows/    <slug>.toml    user flows; an edited built-in
<repo>/.codehead/steps/      <name>.md      repo steps: share them with git
<repo>/.codehead/flows/      <slug>.toml    repo flows
```

The last layer wins per name. `r` in the flows tab resets an edited built-in.

## The flows tab

`F5` (alt+5). `←→` on the list: flows / steps. Every edit is saved at once.

- flows: `n` new, `D` duplicate, `R` rename, `d d` delete, `m` user ↔ repo, `ctrl+f` filter,
  `r` read the files again, `ctrl+d ctrl+d` an edited built-in back to its default,
  `enter` the rail.
- the rail: `a` add a step (type: a shared step, a new shared step, or a one-off), `enter`
  edit, `J` `K` move, `d d` remove, `g` gate, `o` gate outcome (`O` clears), `L` loop (`+`
  `-` its max), `s` save a one-off as a shared step.
- the step editor: `tab` next field, `ctrl+g` gate, `ctrl+f` fork a shared step into a
  one-off of this flow, `#` shortcuts, `ctrl+s` save, `esc` close (edits: `esc` again within 2 s
  drops them).
- steps: usage count per step, `n` new, `enter` edit, `R` rename (flows follow), `d d`
  delete (each flow keeps the text as a one-off).

## On a thread

- new task: each agent card shows its flow under the effort row. `f` on a card: the flows
  (`→` a flow's steps: start at a later step, `0` none). A flow picks the agent. The last flow of
  a repo is the lead's default for the next task. Each agent's step 1 goes to it with the task.
- the thread view: 1 flow row per agent with a flow (above the agents): the flow, the step,
  the agent, a wait, warnings. `ctrl+q` → an agent → `flow` (or a click on its row) opens the
  agent's flow box (`> flow`: of the agent a message goes to): the steps with their arcs,
  `enter` a step's text, `s` skip, `j` jump, `c` change the flow, `u` undo a queued change,
  `e` edit it, `x x` remove it. The menu (`ctrl+p`) has the same per agent.
- the stream: an agent's flow lines (step started, done, a wait) sit inside its messages, in
  a section with its own border.
- a wait: the row says `⏸ … answer below`. The wait is a question in the picker under the
  stream, after the agents' questions. It never takes the keys: write in the prompt,
  `ctrl+enter` sends a message (the wait stays); `tab` then `1`-`9` / `↑↓` `enter` answers, the
  prompt's text goes to the agent as your note.
  - a gate: approve, or an outcome (`changes · back to plan`; it needs your note).
  - a loop at its max: 1 more round, or move on.
  - `reply`: the prompt's text as a message, the wait stays.
- change the flow: `switch now` stops the agent's running turn at its next tool boundary and
  starts the new flow at the step you pick; `after current flow` queues it.
- an agent leaves: its flow ends. An agent replaced (`ctrl+q` → `replace with`): the new agent
  takes the flow over, the current step again with its 1st message.

An agent reports a step of its flow done with `flow_step` (codehead MCP); codehead hands it its
next step, or holds the flow at a gate. An agent on autopilot can pass a gate itself
(`pass_gate`); the thread marks it. An agent can propose another flow with `flow_suggest`.
