"""Demo world: a home folder with codehead's config, 5 agents, notes, a git repository of a
payments service (`payd`), fake harnesses on PATH and seeded threads.

The recording bind-mounts ROOT over /home (user + mount namespace), so every path on screen
reads /home/dev/..., never the real home folder.
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import time
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
H = "/home/dev"  # the home folder as the recording sees it

AGENTS = {
    "EVA": dict(harness="claude", model="opus", effort="high", mode="autopilot", color="lavender", style="mecha",
                voice="Short, exact, numbers first.", sprite="regi:47",
                prompt="Run the tests before you report done. Small commits, 1 concern each."),
    "HAL": dict(harness="codex", model="gpt-5.5", effort="high", mode="autopilot", color="glacier",
                 voice="Reviewer. Name the risk, the file and the line.", sprite="geob:20",
                 prompt="You review diffs. Block on correctness, nit on style."),
    "DMON": dict(harness="claude", model="sonnet", effort="medium", mode="supervised", color="orchid",
                 voice="Database person. Careful with migrations.", sprite="regi:48 speed=0.5",
                 prompt="Plan schema changes. Never run a migration on production unasked."),
    "ENOCH": dict(harness="opencode", model="kimi-k2", mode="autopilot", color="sage", sprite="geob:27",
                voice="Fast, pragmatic.", prompt="Finish what others started. Keep their style."),
}


def sprite(sid: str) -> list[str]:
    """The saved still rows of avatar SID: none needed, codehead draws the avatar from its id."""
    return []


def toml_str(s: str) -> str:
    if "\n" in s:
        return '"""\n' + s.rstrip("\n") + '\n"""'
    return json.dumps(s, ensure_ascii=False)


def write(path: Path, text: str, mode: int | None = None):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    if mode:
        path.chmod(mode)


# ------------------------------------------------------------------ repository
FILES = {
    "Cargo.toml": """[package]
name = "payd"
version = "0.14.2"
edition = "2024"

[dependencies]
axum = "0.8"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "time", "uuid"] }
time = "0.3"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
uuid = { version = "1", features = ["v7"] }

[dev-dependencies]
wiremock = "0.6"
""",
    "README.md": """# payd

Payment intents, refunds and merchant webhooks for the Acme checkout.

- `src/api/`: the HTTP API (axum)
- `src/ledger/`: double-entry ledger, 1 transaction per state change
- `src/webhook/`: signed webhook deliveries with retries
- `migrations/`: sqlx migrations, applied by `just migrate`
""",
    "src/main.rs": """mod api;
mod config;
mod idempotency;
mod ledger;
mod webhook;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::load()?;
    let db = sqlx::PgPool::connect(&cfg.database_url).await?;
    let worker = webhook::Worker::new(db.clone(), cfg.webhook.clone());
    tokio::spawn(worker.run());
    api::serve(db, cfg).await
}
""",
    "src/config.rs": "pub struct Config { pub database_url: String, pub webhook: crate::webhook::Settings }\n",
    "src/idempotency.rs": "//! Idempotency keys: 24 h, per merchant.\n",
    "src/api/mod.rs": "pub mod intents;\npub mod refunds;\n",
    "src/api/intents.rs": "//! POST /v1/payment_intents\n",
    "src/api/refunds.rs": "//! POST /v1/refunds\n",
    "src/ledger/mod.rs": "pub mod entry;\npub mod balance;\n",
    "src/ledger/entry.rs": "//! 1 debit + 1 credit per entry, same currency.\n",
    "src/ledger/balance.rs": "//! Balances per account, from entries.\n",
    "src/webhook/mod.rs": """mod retry;
mod sender;
mod sign;

pub use sender::Worker;

#[derive(Clone)]
pub struct Settings {
    pub timeout_ms: u64,
    pub max_attempts: u32,
}
""",
    "src/webhook/sender.rs": """use super::retry::{Backoff, Verdict};

pub struct Worker { /* db, http, settings */ }

impl Worker {
    pub async fn deliver(&self, d: &Delivery) -> Result<(), Error> {
        let res = self.http.post(&d.url).json(&d.body).send().await;
        match res {
            Ok(r) if r.status().is_success() => self.mark_sent(d).await,
            Ok(r) => self.schedule_retry(d, r.status().as_u16()).await,
            Err(e) if e.is_timeout() => self.schedule_retry(d, 0).await,
            Err(e) => Err(e.into()),
        }
    }
}
""",
    "src/webhook/retry.rs": """/// Exponential backoff: 1 s, 4 s, 16 s, ... per attempt.
pub struct Backoff { pub base_ms: u64, pub factor: u32 }

pub enum Verdict { Retry(u64), GiveUp }

pub fn verdict(status: u16, attempt: u32, b: &Backoff) -> Verdict {
    // every non-2xx status retries
    Verdict::Retry(b.base_ms * (b.factor as u64).pow(attempt))
}
""",
    "src/webhook/sign.rs": "//! HMAC-SHA256 signature header: `Payd-Signature: t=..,v1=..`\n",
    "migrations/0006_webhook_endpoints.sql": "create table webhook_endpoints (id uuid primary key, merchant_id uuid not null, url text not null, disabled_at timestamptz);\n",
    "migrations/0007_webhook_attempts.sql": "create table webhook_attempts (id uuid primary key, delivery_id uuid not null, status int, created_at timestamptz not null default now());\n",
    "tests/webhook_retry.rs": "// retry policy tests\n",
    "tests/ledger_balance.rs": "// balance tests\n",
    "justfile": "test:\n    cargo test\n\nmigrate:\n    sqlx migrate run\n",
    ".gitignore": "/target\n",
}

NOTES = {
    "incidents/2026-11-03-webhook-storm.md": """# 2026-11-03: webhook retry storm

- 14:02 merchant `m_7Qd2` removed its endpoint, answers 410 Gone
- payd retried every delivery for 9 h: 41k requests, queue lag 22 min for all merchants
- cause: `retry::verdict` retries every non-2xx status
- follow-up: give up on 410, disable the endpoint, cap the total wait
""",
    "design/webhook-v2.md": "# webhook v2\n\n- per-endpoint circuit breaker\n- partitioned `webhook_attempts`\n",
    "runbooks/migrations.md": "# migrations\n\n- staging first, then sandbox, production last\n- backfills outside 08:00-20:00 UTC\n",
    "team/oncall.md": "# on-call\n\n- primary: Mira, secondary: Jonas\n",
}


def git(cwd: Path, *args: str, env=None):
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, env=env)


def build_repo(root: Path):
    repo = root / "dev/projects/payd"
    if repo.exists():
        shutil.rmtree(repo)
    repo.mkdir(parents=True)
    env = dict(os.environ, GIT_AUTHOR_NAME="Dev", GIT_AUTHOR_EMAIL="dev@acme.test", GIT_COMMITTER_NAME="Dev",
               GIT_COMMITTER_EMAIL="dev@acme.test", HOME=str(root / "dev"))
    git(repo, "init", "-q", "-b", "main", env=env)
    msgs = ["feat: payment intents", "feat: double-entry ledger", "feat: refunds", "feat: signed webhooks",
            "feat: webhook retries with backoff", "chore: sqlx 0.8"]
    items = list(FILES.items())
    per = len(items) // len(msgs) + 1
    for i, m in enumerate(msgs):
        for p, t in items[i * per : (i + 1) * per]:
            write(repo / p, t)
        git(repo, "add", "-A", env=env)
        git(repo, "commit", "-q", "-m", m, env=env)
    return repo


SETUP = r"""
set -e
export GIT_AUTHOR_DATE="2026-11-18T09:12:00+01:00" GIT_COMMITTER_DATE="2026-11-18T09:12:00+01:00"
cd /home/dev/projects/payd
git init -q --bare /home/dev/.remotes/payd.git
git remote add origin /home/dev/.remotes/payd.git
git push -q -u origin main
git branch fix/refund-rounding
git push -q origin fix/refund-rounding
git switch -q -c fix/stop-the-410-retry-storm
printf '%s' "$RETRY_V1" > src/webhook/retry.rs
printf '// 5 tests: 410, 5xx + 429, timeouts, other 4xx, the 30 min cap\n' > tests/webhook_retry.rs
# more files of different gravity for the changes view: a migration, the sender, the docs
mkdir -p migrations docs
printf 'alter table webhook_endpoints add column disabled_at timestamptz;\nalter table webhook_endpoints add column disabled_reason text;\ncreate index webhook_endpoints_live on webhook_endpoints (id) where disabled_at is null;\n' > migrations/0009_endpoint_disabled.sql
printf '\n/// Disabled endpoints get 0 deliveries (migration 0009).\npub fn live(e: &Endpoint) -> bool { e.disabled_at.is_none() }\n' >> src/webhook/sender.rs
printf '# Webhooks\n\n410 Gone disables the endpoint. 429, 5xx and timeouts back off, 30 min in total.\n' > docs/webhooks.md
git add -A
git commit -qm 'fix(webhook): give up on 410, cap the total wait at 30 min'
export GIT_AUTHOR_DATE="2026-11-18T09:31:00+01:00" GIT_COMMITTER_DATE="2026-11-18T09:31:00+01:00"
printf '%s' "$RETRY_V2" > src/webhook/retry.rs
git commit -qam 'fix(webhook): no overflow on huge attempt counts'
git switch -q main
mkdir -p /home/dev/.local/share/codehead/worktrees/payd/feat
git worktree add -q -b feat/payout-schedules /home/dev/.local/share/codehead/worktrees/payd/feat/payout-schedules main
cd /home/dev/.local/share/codehead/worktrees/payd/feat/payout-schedules
mkdir -p src/payout && printf 'pub enum Schedule { Daily, Weekly(Weekday), Monthly(u8) }\n' > src/payout/schedule.rs
git add -A && git commit -qm 'feat(payout): weekly and monthly schedules'
printf '// cutoff: 00:00 in the merchant time zone\n' >> src/payout/schedule.rs
# the main checkout on the fix branch: the changes view shows its diff against origin/main
cd /home/dev/projects/payd
git switch -q fix/stop-the-410-retry-storm
"""


# retry.rs on the fix branch: after the 1st fix, after the overflow fix
RETRY_V1 = """/// Exponential backoff: 1 s, 4 s, 16 s, ... per attempt.
pub struct Backoff { pub base_ms: u64, pub factor: u32 }

pub enum Verdict { Retry(u64), GiveUp, Disable }

/// Total wait of 1 delivery, from its 1st attempt.
pub const MAX_WAIT_MS: u64 = 30 * 60 * 1000;

/// What to do after a failed attempt. `waited_ms`: time since the 1st attempt.
pub fn verdict(status: u16, attempt: u32, waited_ms: u64, b: &Backoff) -> Verdict {
    match status {
        410 => Verdict::Disable,
        0 | 429 | 500..=599 => {
            let delay = b.base_ms * (b.factor as u64).pow(attempt);
            if waited_ms + delay > MAX_WAIT_MS { Verdict::GiveUp } else { Verdict::Retry(delay) }
        }
        _ => Verdict::GiveUp,
    }
}
"""
RETRY_V2 = RETRY_V1.replace("""            let delay = b.base_ms * (b.factor as u64).pow(attempt);
            if waited_ms + delay > MAX_WAIT_MS { Verdict::GiveUp } else { Verdict::Retry(delay) }""", """            let Some(delay) = (b.factor as u64).checked_pow(attempt).map(|f| f.saturating_mul(b.base_ms)) else {
                return Verdict::GiveUp;
            };
            if waited_ms.saturating_add(delay) > MAX_WAIT_MS { Verdict::GiveUp } else { Verdict::Retry(delay) }""")


def setup_git(root: Path):
    """Remote, branches and a worktree, made inside the namespace: paths read /home/dev."""
    env = dict(os.environ, HOME=H, RETRY_V1=RETRY_V1, RETRY_V2=RETRY_V2,
               GIT_AUTHOR_NAME="Dev", GIT_AUTHOR_EMAIL="dev@acme.test", GIT_COMMITTER_NAME="Dev", GIT_COMMITTER_EMAIL="dev@acme.test")
    subprocess.run(["unshare", "-rm", "/bin/sh", "-c", f"mount --bind {root} /home && {SETUP}"], check=True, env=env,
                   capture_output=True)
    out = subprocess.run(["unshare", "-rm", "/bin/sh", "-c", f"mount --bind {root} /home && cd {H}/projects/payd && git log --format=%h -2 fix/stop-the-410-retry-storm"],
                         capture_output=True, text=True, env=env).stdout.split()
    return {"{{overflow}}": out[0], "{{fix}}": out[1]}


# ------------------------------------------------------------------ home folder
def build(root: Path, script: dict, threads: list | None = None, config: dict | None = None):
    """A fresh demo world under ROOT (mounted on /home). SCRIPT: the fake harness turns."""
    if root.exists():
        subprocess.run(["chmod", "-R", "u+w", str(root)], check=False)
        shutil.rmtree(root)
    home = root / "dev"
    (home / ".run").mkdir(parents=True)
    write(home / ".gitconfig", "[user]\n\tname = Dev\n\temail = dev@acme.test\n[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n")
    build_repo(root)
    hashes = setup_git(root)
    for old, new in hashes.items():
        script = json.loads(json.dumps(script).replace(old, new))
        threads = json.loads(json.dumps(threads).replace(old, new)) if threads else threads
    for p, t in NOTES.items():
        write(home / "notes" / p, t)
    shutil.copytree(REPO / "output-styles", home / ".config/codehead/styles")

    cfg = {"notes": f"{H}/notes", "styles": f"{H}/.config/codehead/styles", "agent": "EVA", "prefix": "fix",
           "models": "fable,opus,sonnet,haiku", "messages": "bubbles", "thoughts": "folded",
           # 0 model calls behind the scenes: the changes view sorts by path rules
           "classify": "off", "rank": "off", "review": "off"}
    cfg.update(config or {})
    lines = [f"{k} = {json.dumps(v)}" for k, v in cfg.items() if not isinstance(v, dict)]
    # the default scheme: lavender on warm paper, as codehead.sh
    lines += ["", "[colors]", 'scheme = "lavender"', "", "[aliases]", f'"payd:notes" = "{H}/notes/"']
    write(home / ".config/codehead.toml", "\n".join(lines) + "\n")

    for name, a in AGENTS.items():
        rows = sprite(a["sprite"])
        body = [f"harness = {toml_str(a['harness'])}"]
        for k in ("model", "effort", "mode", "color", "style", "voice", "prompt"):
            if a.get(k):
                body.append(f"{k} = {toml_str(a[k])}")
        body.append("sprite = { id = %s, rows = [%s] }" % (json.dumps(a["sprite"]), ", ".join(json.dumps(r, ensure_ascii=False) for r in rows)))
        write(home / f".config/codehead/agents/{name}.toml", "\n".join(body) + "\n")

    # fake harnesses + codehead itself
    bin_ = home / "bin"
    bin_.mkdir()
    shutil.copy(REPO / "target/release/codehead", bin_ / "codehead")
    fake = (HERE / "fake_agent.py").read_text()
    for b in ["claude", "codex", "codex-acp", "opencode", "amp", "amp-acp", "goose", "pi", "pi-acp"]:
        write(bin_ / b, fake, 0o755)

    demo = home / ".demo"
    (demo / "state").mkdir(parents=True)
    write(demo / "script.json", json.dumps(script, ensure_ascii=False, indent=1))

    # a recent Claude Code session (threads tab: sessions to continue)
    sid = str(uuid.uuid4())
    proj = home / ".claude/projects/-home-dev-projects-payd"
    tr = [
        {"type": "user", "cwd": f"{H}/projects/payd", "gitBranch": "main", "message": {"role": "user", "content": "why does the refund test flake on CI?"}},
        {"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "The flake comes from `Utc::now()` in `refunds.rs:88`: 2 calls 1 ms apart round to different cents windows. Fix: pass 1 clock into the refund builder."}]}},
        {"type": "ai-title", "aiTitle": "Refund test flakes on CI"},
    ]
    write(proj / f"{sid}.jsonl", "\n".join(json.dumps(x) for x in tr) + "\n")

    now = int(time.time())
    write(home / ".local/share/codehead/limits.json", json.dumps({"claude": {
        "five_hour": {"pct": 34.0, "resets": now + 2 * 3600 + 840}, "seven_day": {"pct": 58.0, "resets": now + 3 * 86400}}}))
    for t in threads or []:
        seed_thread(home, t)
    return home


def seed_thread(home: Path, t: dict):
    """A thread record with its events. T: id, title, agents, events [(agent|None, kind, {...})],
    age (s), state, branch, worktree."""
    now = int(time.time() * 1000)
    created = now - int(t.get("age", 3600) * 1000)
    d = home / ".local/share/codehead/threads" / t["id"]
    rec = {"id": t["id"], "title": t["title"], "task": t.get("task", t["title"]), "dir": f"{H}/projects/payd",
           "state": t.get("state", "open"), "created": created, "updated": created + int(t.get("span", 600) * 1000),
           "agents": t["agents"], "sessions": {}}
    for k in ("branch", "worktree"):
        if t.get(k):
            rec[k] = t[k]
    write(d / "thread.json", json.dumps(rec, indent=1))
    evs = []
    ts = created
    for agent, kind, body in t.get("events", []):
        ts += body.pop("_dt", 4000) if isinstance(body, dict) else 4000
        e = {"ts": ts}
        if agent:
            e["agent"] = agent
        e["kind"] = kind
        e.update(body)
        evs.append(json.dumps(e, ensure_ascii=False))
    write(d / "events.jsonl", "\n".join(evs) + ("\n" if evs else ""))


def env(root: Path) -> dict:
    return {
        "HOME": H, "USER": "dev", "LOGNAME": "dev", "SHELL": "/bin/sh",
        "PATH": f"{H}/bin:/usr/local/bin:/usr/bin:/bin",
        "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "en_US.UTF-8",
        "XDG_RUNTIME_DIR": f"{H}/.run", "CODEHEAD_NO_UPDATE_CHECK": "1",
        "DEMO_SCRIPT": f"{H}/.demo/script.json", "DEMO_STATE": f"{H}/.demo/state",
        "EDITOR": "vi", "TZ": "Europe/Berlin",
    }


def argv(root: Path, cmd: str, cwd: str = f"{H}/projects/payd") -> list[str]:
    """Run CMD in the demo world: a user + mount + pid namespace with ROOT on /home."""
    inner = f"mount --bind {root} /home && cd {cwd} && exec {cmd}"
    return ["unshare", "-rm", "--pid", "--fork", "--kill-child", "/bin/sh", "-c", inner]
