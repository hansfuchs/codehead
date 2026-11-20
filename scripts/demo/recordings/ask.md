---
label: questions
order: 6
font: 24
caption: DMON asks before a database query, then asks the owner 2 questions in the stream: 1 pick, then several.
ctx: DMON=31000
---

# seed d41b07 · Partition webhook_attempts
agents DMON · branch main · age 900 · span 60

## owner · use partition-task
## DMON · use partition-scan

# live

## DMON · use partition-plan

# keys

```keys
wait '1 read-only query'
sleep 2
type "Go on. Ask me what you need."
sleep 0.5
key ctrl+enter
# the permission ask: allow
wait 'allow' 30
sleep 2.2
key enter
# question 1: 1 pick
wait 'Which partition key' 40
sleep 2.5
key down
sleep 0.5
key up
sleep 0.6
key enter
# question 2: several
wait 'Where may the backfill' 10
sleep 2
key space
sleep 0.5
key down
sleep 0.3
key space
sleep 0.5
key down
sleep 0.3
key space
sleep 0.8
key enter
wait "I'll write them, not run them" 60
sleep 3
```
