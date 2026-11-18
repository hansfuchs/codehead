---
title: Agents together
description: How agents message each other, bring each other in and report.
---

Every agent session gets codehead's MCP server (`codehead mcp`) with 3 tools:

| Tool | Input | Effect |
| --- | --- | --- |
| `send` | `to` (name or names), `message` | a message to other agents on the thread |
| `set_status` | `state` (`working`, `blocked`, `done`), `note?` | a status row in the thread |
| `list_agents` |  | every agent: harness, model, mode, on the thread or not |

An agent sees only what is sent to it. A message reaches the other agent as a prompt that
starts with `[from @name]`; you see it in the stream as `name → other`. Sending to an agent
that is not on the thread brings it in.

A typical split:

```text
add retries to the webhook sender, then ask @bob for a review
```

The lead does the work and calls `send` with `to: "bob"`. bob joins, reviews, and answers
the lead with `send` in turn. You can step in at any time: a message with `@bob` goes to bob
only.

## Replacing an agent

Type `@name` in the prompt and press `→` on the name: **⇄ replace** an agent of the thread. On
send, the old agent leaves and the new one gets your message together with the old agent's
last messages.

## Handoff

Each agent's system prompt carries a handoff rule: a step that needs a tool its harness lacks
goes to an agent on the thread that has it (`list_agents` shows harnesses). Without such an
agent, the agent reports the step as blocked with 1 line `needs harness: <harness> (<tool>)`.
