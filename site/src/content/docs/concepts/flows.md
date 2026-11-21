---
title: Flows
description: The steps the agents of a thread take for a kind of task, with gates, loops and roles.
---

A **flow** is the order of steps for a kind of task: new feature, bug fix, refactor, ...
Set one on a thread, and its agents get 1 step at a time. Each step says what to do and when
it is done. A **gate** holds the flow until you approve. A **loop** goes back until its
condition holds. A **role** (`lead`, `reviewer`, ...) names the agent of a step.

codehead ships 10 flows: new feature, bug fix, bug investigation, refactor, concept,
performance, migration / upgrade, incident / hotfix, code review, chore.

## Steps

Flows are built from **steps**. A step is shared: edit it once, and each flow that uses it
changes. A flow can also hold a **one-off** step, with text for that flow only; `s` on the rail
saves a one-off as a shared step.

```markdown
---
description: another agent reviews the diff
role: reviewer
done: findings sent to the lead, or 0 findings reported
needs: [skill:code-review]
---
Review the full diff of this thread ...
```

- `role`: the role whose agent does the step (default: `lead`, the thread's lead).
- `gate: true`: the flow waits for your approval after the step.
- `done`: the exit criteria the agent checks.
- `needs`: `skill:<name>`, `mcp:<server>`, `harness:<name>`. The agent of the role lacks one:
  the thread shows a warning.
- the text: `#shortcuts` expand when the step starts.

## Flows

```toml
name = "new feature"
roles = ["lead", "reviewer"]

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
flow after the step's text), `role`, `done`, `gate`, `loop`. A loop at its `max` waits for
you: 1 more round, or move on.

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

- flows: `n` new, `D` duplicate, `R` rename, `d d` delete, `m` user ↔ repo, `/` filter,
  `enter` the rail.
- the rail: `a` add a step (type: a shared step, a new shared step, or a one-off), `enter`
  edit, `J` `K` move, `d d` remove, `g` gate, `o` gate outcome (`O` clears), `L` loop (`+`
  `-` its max), `@` role, `s` save a one-off as a shared step.
- the step editor: `tab` next field, `ctrl+g` gate, `ctrl+f` fork a shared step into a
  one-off of this flow, `#` shortcuts, `esc` save.
- steps: usage count per step, `n` new, `enter` edit, `R` rename (flows follow), `d d`
  delete (each flow keeps the text as a one-off).

## On a thread

- new task: the `flow` row (`←→` flows, a letter jumps, `0` none). The last flow of a repo
  is the next task's default. Step 1 goes to its agent with the task.
- the thread view: the flow row above the agents shows the flow, the step, its agent, a
  wait, warnings. `ctrl+f` (or a click, or `> flow`) opens its box: the steps with their
  arcs, `enter` a step's text, `a` answer a wait, `s` skip, `j` jump, `@` map a role,
  `c` change the flow, `e` edit it, `x x` remove it.
- a gate: the row says `⏸ gate ...`. `ctrl+f`, pick approve or an outcome; text in the prompt
  goes to the agent as your note.
- a role with 0 agent: the flow waits; pick the agent to bring in. Agent files take default
  roles: `roles = ["reviewer"]`.
- change the flow: `switch now` stops the running turn at its next tool boundary and starts
  the new flow at the step you pick; `after current flow` queues it.

Agents report a step done with `flow_step` (codehead MCP); codehead hands them the next
step, hands it to another agent, or holds the flow at a gate. An agent on autopilot can pass
a gate itself (`pass_gate`); the thread marks it. An agent can propose another flow with
`flow_suggest`.
