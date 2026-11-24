---
label: the loop
order: 1
font: 19
caption: The diff left, the chat right. EVA heads the wrong way: esc, 1 sentence, back on track. Migration first: 1 line comment, fixed, marked read.
ctx: EVA=64000
---

# seed 7c2e91 · Stop the 410 retry storm
agents EVA · branch fix/stop-the-410-retry-storm · age 1800 · span 300

## owner · use task
## EVA · use analysis

# live

## EVA · name wrong · when Go ahead · delay 1.0

### think
Fastest fix: keep retrying, just slower. A 24 h cap with a longer backoff stops the storm.

### say
Plan: keep retrying 410 too, with a longer backoff (base 60 s) and a 24 h cap. Editing `retry.rs`.

### edit src/webhook/retry.rs · 12s
```old
pub fn verdict(status: u16, attempt: u32, b: &Backoff) -> Verdict {
    // every non-2xx status retries
    Verdict::Retry(b.base_ms * (b.factor as u64).pow(attempt))
}
```
```new
pub fn verdict(status: u16, attempt: u32, b: &Backoff) -> Verdict {
    // every non-2xx status retries, slower, for 24 h at most
    let delay = 60_000 * (b.factor as u64).pow(attempt);
    if delay > 24 * 3_600_000 { Verdict::GiveUp } else { Verdict::Retry(delay) }
}
```

## EVA · use fix · when gone

## EVA · name line-fix · when 0009 · cost 0.21 · delay 0.8

### think
A free-text reason can't be queried. An enum type in the migration, the sender writes it.

### edit migrations/0009_endpoint_disabled.sql
```old
alter table webhook_endpoints add column disabled_reason text;
```
```new
create type endpoint_disabled_reason as enum ('gone', 'manual');
alter table webhook_endpoints add column disabled_reason endpoint_disabled_reason;
```

### bash Migrate a scratch database, run the tests · 4.2s
```sh
sqlx migrate run --database-url "$SCRATCH_URL" && cargo test 2>&1 | grep -c '^test result: ok'
```
```
Applied 9/migrate endpoint disabled (12.4ms)
3
```

### say
Done: `disabled_reason` is an enum now (`gone`, `manual`). Migration applies on a scratch database, all 3 test suites pass.

# keys

```keys
wait 'Go ahead\?'
sleep 1.5
type "Go ahead."
sleep 0.4
key ctrl+enter
# the wrong plan streams in: stop it mid-edit
wait 'longer backoff' 40
sleep 2.2
key esc
sleep 1.2
type "No: 410 means gone. Disable the endpoint, don't retry it. Tests first."
sleep 0.5
key ctrl+enter
wait 'Want it\?' 150
sleep 2
shot fixed
# wide: the changes left, the chat right. esc (0 agents at work) gives the keys to the changes
key esc
sleep 1
# the branch's commits: the panel's "all changes" row (the default view: uncommitted only)
click 'all changes'
sleep 1.2
# the keys to the review: a click on the migration's box head
click 'migrations/0009_endpoint_disabled\.sql'
sleep 0.8
shot changes
type "n"
sleep 0.6
key down 3 0.25
sleep 0.4
key space 2 0.35
sleep 0.5
type "c"
sleep 0.8
type "an enum, not free text: we will filter on it"
sleep 0.5
key ctrl+enter
sleep 0.8
key ctrl+enter
wait 'is an enum now' 60
sleep 1.5
type "v"
sleep 2.5
shot end
```
