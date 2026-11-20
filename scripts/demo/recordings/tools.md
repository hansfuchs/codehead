---
label: your tools
order: 5
font: 24
caption: ! runs a shell command in the thread, ctrl+r searches the history with fzf, ctrl+y opens yazi, alt+4 the files the agents wrote.
ctx: EVA=64000
---

# seed 7c2e91 · Stop the 410 retry storm
agents EVA · branch fix/stop-the-410-retry-storm · age 1800 · span 300

## owner · use task
## EVA · use analysis
## owner · use followup
## EVA · use fix

# keys

```keys
wait 'Want it\?'
sleep 1.5
# ! : shell mode, the output lands in the thread
type "! git log --oneline -3"
sleep 0.6
key enter
sleep 2.5
# ctrl+r: fzf over the thread's commands and the shell history
type "! "
key ctrl+r
sleep 1
type "log"
sleep 1.2
key esc
sleep 0.4
key ctrl+c
sleep 0.6
# ctrl+y: yazi on the worktree, q back
key ctrl+y
sleep 3
type "q"
sleep 1.2
# alt+4: the files the agents wrote, Markdown rendered, code in colors
key alt+4
sleep 2.5
key down 1
sleep 2
key alt+1
sleep 1.5
```
