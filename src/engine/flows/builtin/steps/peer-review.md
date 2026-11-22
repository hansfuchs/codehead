---
description: another agent reviews the diff
role: reviewer
done: findings sent to the lead, or 0 findings reported
---
Review the full diff of this thread (all changes since the branch point): correctness, edge cases, security, performance, tests, readability, conventions of the codebase. Write 1 `comment` per finding (codehead MCP) with severity. A later round: check the resolved comments (`comments`, state resolved); re-open each one whose fix does not hold (`comment_status` open, note why). Then send the lead a short summary of the findings.
