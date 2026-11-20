---
label: the crew
order: 3
font: 24
caption: EVA asks HAL for a review. HAL joins on Codex, blocks on 1 line, EVA fixes it, HAL approves. The owner writes 1 line.
ctx: EVA=82000, HAL=24000
---

# seed 7c2e91 · Stop the 410 retry storm
agents EVA · branch fix/stop-the-410-retry-storm · age 1500 · span 400

## owner · use task
## EVA · use analysis
## owner · use followup
## EVA · use fix

# live

## EVA · use ship
## HAL · use review
## EVA · use review-fix
## HAL · use lgtm

# keys

```keys
wait 'Want it\?'
sleep 2
type "Ship it. HAL reviews first."
sleep 0.5
key ctrl+enter
wait 'Approved from my side' 150
sleep 3
```
