---
description: another agent reviews the diff
done: the reviewer's findings came back (or 0 findings)
---
Get a review of the full diff of this thread (all changes since the branch point) from another agent: `list_agents` (codehead MCP) shows the agents; pick one whose harness or model differs from yours, a reviewer when there is one. `send` it the request: review correctness, edge cases, security, performance, tests, readability and the conventions of the codebase; 1 `comment` per finding with severity; a later round: check the resolved comments (`comments`, state resolved) and re-open each one whose fix does not hold (`comment_status` open, note why) and say why with `comment_reply`; then a short summary back to you. Wait for its reply. 0 other agent: say so in the thread and review the diff yourself the same way.
