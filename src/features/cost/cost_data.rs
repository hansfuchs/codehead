use super::{
    AgentRow, AgentUse, Duration, HashMap, Instant, Names, RATE_THREADS, Snap, State, SystemTime, UNIX_EPOCH, agents,
};

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Context window of MODEL in tokens: a guess for events without the harness' window.
pub(super) fn window_of(model: &str) -> u64 {
    if model.contains("[1m]") { 1_000_000 } else { 200_000 }
}

/// Per agent of thread ID, from its turn ends; live agents from its host, their running
/// turns' tokens so far (the $ of a running turn: at the agent's last $ per token).
pub(crate) fn uses(id: &str) -> Vec<AgentUse> {
    let state = if crate::engine::client::alive(id) {
        crate::engine::client::state_full(id)
    } else {
        State::default()
    };

    let live = state.live;
    let mut usages: Vec<AgentUse> = Vec::new();
    let mut models: HashMap<String, String> = HashMap::new();

    for event in crate::engine::store::events(id) {
        account_event(&live, &mut usages, &mut models, event);
    }

    // 0 turn end with a $ / a window in this thread yet: the agent's last ones of its other threads
    for usage in usages.iter_mut().filter(|candidate| candidate.usd_per_token == 0.0) {
        usage.usd_per_token = past_turn(&usage.agent, id).rate;
    }

    for (name, turn) in state.usage {
        let Some(running_agent) = usages.iter_mut().find(|candidate_agent| candidate_agent.agent == name) else {
            continue;
        };

        // a 1st turn: the window of the agent's last turn elsewhere (Claude reports it at the turn's end only)
        let past = past_turn(&name, id).window;

        let window = if past > 0 {
            past
        } else {
            window_of(models.get(&name).map_or("", String::as_str))
        };

        add_running(running_agent, &turn, window, now());
    }

    usages.retain(|candidate_usage| candidate_usage.turns > 0 || candidate_usage.live);
    usages.sort_by(|left, right| right.usd.total_cmp(&left.usd));

    usages
}

fn account_event(
    live: &[String],
    usage: &mut Vec<AgentUse>,
    models: &mut HashMap<String, String>,
    event: crate::engine::store::Event,
) {
    let Some(name) = event.agent.clone() else {
        return;
    };

    let i = if let Some(i) = usage.iter().position(|candidate| candidate.agent == name) {
        i
    } else {
        // the roster's harness; a removed agent: unknown
        let harness = crate::engine::roster::get(&name).map_or_else(|| "?".into(), |entry| entry.harness);

        usage.push(AgentUse {
            agent: name.clone(),
            harness,
            live: live.contains(&name),
            ..Default::default()
        });
        usage.len() - 1
    };

    let agent_use = &mut usage[i];

    match event.kind {
        crate::engine::store::Kind::Session { model, .. } => {
            agent_use.sessions += 1;
            models.insert(name, model);
        }
        crate::engine::store::Kind::TurnEnd { .. } => account_turn(agent_use, &event, models, &name),
        _ => {}
    }
}

/// Running turn USAGE of AGENT at NOW: its tokens, $ (at the last $ per token), ctx (of WINDOW
/// tokens, when the thread has no window of the agent yet).
pub(super) fn add_running(agent: &mut AgentUse, usage: &crate::engine::harness::TurnUse, window: u64, now: u64) {
    agent.input += usage.input;
    agent.output += usage.output;
    agent.cache_read += usage.cache_read;
    agent.cache_write += usage.cache_write;
    agent.usd += agent.usd_per_token * (usage.input + usage.output + usage.cache_read + usage.cache_write) as f64;

    if usage.ctx > 0 {
        let size = agent.ctx.map_or(window, |ctx| ctx.1);

        agent.ctx = Some((usage.ctx, size));
    }

    if usage.input + usage.output > 0 {
        agent.samples.push((now, usage.input + usage.output));
    }
}

/// An agent's last turn elsewhere: its $ per token, its context window (0: none).
#[derive(Clone, Copy, Default)]
pub(super) struct PastTurn {
    pub rate: f64,
    pub window: u64,
}

/// AGENT's last turn with a $ and its last with a window, in other threads than SKIP (newest
/// threads first). Kept per process (a miss: read again after 60 s).
pub(super) fn past_turn(agent: &str, skip: &str) -> PastTurn {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<String, (PastTurn, Instant)>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);

    if let Some(&(cached, at)) = seen.lock().ok().as_ref().and_then(|past| past.get(agent))
        && ((cached.rate > 0.0 && cached.window > 0) || at.elapsed() < Duration::from_secs(60))
    {
        return cached;
    }

    let mut past = PastTurn::default();

    for thread in crate::engine::store::all()
        .into_iter()
        .filter(|thread| thread.id != skip)
        .take(RATE_THREADS)
    {
        let events = crate::engine::store::events_tail(&thread.id, 256 * 1024);

        if past.rate == 0.0 {
            past.rate = last_rate(&events, agent).unwrap_or(0.0);
        }

        if past.window == 0 {
            past.window = last_window(&events, agent).unwrap_or(0);
        }

        if past.rate > 0.0 && past.window > 0 {
            break;
        }
    }

    if let Ok(mut seen) = seen.lock() {
        seen.insert(agent.to_string(), (past, Instant::now()));
    }

    past
}

/// The context window of AGENT's last turn end with one in EVENTS.
pub(super) fn last_window(events: &[crate::engine::store::Event], agent: &str) -> Option<u64> {
    events
        .iter()
        .rev()
        .filter(|event| event.agent.as_deref() == Some(agent))
        .find_map(|event| match event.kind {
            crate::engine::store::Kind::TurnEnd { window, .. } if window > 0 => Some(window),
            _ => None,
        })
}

/// $ per token of AGENT's last turn end with a $ in EVENTS.
pub(super) fn last_rate(events: &[crate::engine::store::Event], agent: &str) -> Option<f64> {
    events
        .iter()
        .rev()
        .filter(|event| event.agent.as_deref() == Some(agent))
        .find_map(|event| match event.kind {
            crate::engine::store::Kind::TurnEnd {
                cost,
                input,
                output,
                cache_read,
                cache_write,
                ..
            } if cost > 0.0 && input + output + cache_read + cache_write > 0 => {
                Some(cost / (input + output + cache_read + cache_write) as f64)
            }
            _ => None,
        })
}

/// The cost box's numbers of thread ID.
pub(crate) fn snapshot(id: &str) -> Snap {
    fetch(id, &names())
}

pub(super) fn fetch(id: &str, names: &Names) -> Snap {
    let Some(thread) = crate::engine::store::load(id) else {
        return Snap::default();
    };

    let mut snap = Snap {
        found: true,
        id: thread.id.clone(),
        title: thread.title.clone(),
        ..Default::default()
    };

    for usage in uses(&thread.id) {
        snap.cost += usage.usd;
        let (name, color) = names
            .get(&usage.agent)
            .cloned()
            .unwrap_or_else(|| (usage.agent.clone(), None));

        snap.agents.push(AgentRow {
            name,
            color,
            ctx: usage
                .ctx
                .filter(|(_, size)| *size > 0)
                .map(|(used, size)| used * 100 / size),
            live: usage.live,
            ctx_tokens: usage.ctx.map_or(0, |(used, _)| used),
            usage,
        });
    }

    snap.limits = of_harnesses(crate::engine::limits::read(), &snap.agents);

    snap
}

pub(super) fn names() -> Names {
    agents::roster_only()
        .into_iter()
        .map(|agent| (agent.name.clone(), (agent.name, agent.color)))
        .collect()
}

fn account_turn(
    agent: &mut AgentUse,
    event: &crate::engine::store::Event,
    models: &HashMap<String, String>,
    name: &str,
) {
    let crate::engine::store::Kind::TurnEnd {
        cost,
        input,
        output,
        cache_read,
        cache_write,
        ctx,
        window,
        ..
    } = event.kind
    else {
        return;
    };

    agent.input += input;
    agent.output += output;
    agent.cache_read += cache_read;
    agent.cache_write += cache_write;
    agent.turns += 1;
    agent.usd += cost;
    let tokens = input + output + cache_read + cache_write;

    if tokens > 0 {
        agent.usd_per_token = cost / tokens as f64;
    }

    if ctx > 0 {
        // the harness' window; older events: a guess from the model name
        let size = if window > 0 {
            window
        } else {
            window_of(models.get(name).map_or("", String::as_str))
        };

        agent.ctx = Some((ctx, size));
    }

    agent.samples.push((event.ts / 1000, input + output));
}

/// The account's windows of the AGENTS' harnesses only (a codex thread: 0 claude window).
pub(super) fn of_harnesses(limits: Vec<super::Limit>, agents: &[AgentRow]) -> Vec<super::Limit> {
    limits
        .into_iter()
        .filter(|limit| agents.iter().any(|agent| agent.usage.harness == limit.harness))
        .collect()
}
