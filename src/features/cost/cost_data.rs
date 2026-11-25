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

/// Plan limits the hosts saw last: (harness, window, used %, resets at).
pub(crate) fn limits() -> Vec<(String, String, f64, Option<u64>)> {
    crate::engine::limits::read()
        .into_iter()
        .map(|limit| (limit.harness, limit.label, limit.pct, limit.resets))
        .collect()
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

    // 0 turn end with a $ in this thread yet: the agent's $ per token from its other threads
    for usage in usages.iter_mut().filter(|candidate| candidate.usd_per_token == 0.0) {
        usage.usd_per_token = past_rate(&usage.agent, id);
    }

    for (name, turn) in state.usage {
        let Some(running_agent) = usages.iter_mut().find(|candidate_agent| candidate_agent.agent == name) else {
            continue;
        };

        add_running(
            running_agent,
            &turn,
            models.get(&name).map_or("", String::as_str),
            now(),
        );
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

/// Running turn USAGE of AGENT (model MODEL) at NOW: its tokens, $ (at the last $ per token), ctx.
pub(super) fn add_running(agent: &mut AgentUse, usage: &crate::engine::harness::TurnUse, model: &str, now: u64) {
    agent.input += usage.input;
    agent.output += usage.output;
    agent.cache_read += usage.cache_read;
    agent.cache_write += usage.cache_write;
    agent.usd += agent.usd_per_token * (usage.input + usage.output + usage.cache_read + usage.cache_write) as f64;

    if usage.ctx > 0 {
        let size = agent.ctx.map_or_else(|| window_of(model), |ctx| ctx.1);

        agent.ctx = Some((usage.ctx, size));
    }

    if usage.input + usage.output > 0 {
        agent.samples.push((now, usage.input + usage.output));
    }
}

/// $ per token of AGENT's last turn with a $ in another thread than SKIP (newest threads first).
/// 0: none. Kept per process (a miss: read again after 60 s).
pub(super) fn past_rate(agent: &str, skip: &str) -> f64 {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<String, (f64, Instant)>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);

    if let Some(&(cached, at)) = seen.lock().ok().as_ref().and_then(|rates| rates.get(agent))
        && (cached > 0.0 || at.elapsed() < Duration::from_secs(60))
    {
        return cached;
    }

    let rate = crate::engine::store::all()
        .into_iter()
        .filter(|thread| thread.id != skip)
        .take(RATE_THREADS)
        .find_map(|thread| last_rate(&crate::engine::store::events_tail(&thread.id, 256 * 1024), agent))
        .unwrap_or(0.0);

    if let Ok(mut rates) = seen.lock() {
        rates.insert(agent.to_string(), (rate, Instant::now()));
    }

    rate
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

    let (_, busy) = if crate::engine::client::alive(&thread.id) {
        crate::engine::client::state(&thread.id)
    } else {
        Default::default()
    };

    let status = if busy.is_empty() {
        thread.state.clone()
    } else {
        format!("working: {}", busy.join(", "))
    };

    let mut snap = Snap {
        found: true,
        status,
        ..Default::default()
    };

    for usage in uses(&thread.id) {
        snap.cost += usage.usd;
        snap.input += usage.input;
        snap.output += usage.output;
        snap.cache_read += usage.cache_read;
        snap.cache_write += usage.cache_write;
        snap.turns += usage.turns;
        let (name, color, sprite) = names.get(&usage.agent).cloned().map_or_else(
            || (usage.agent.clone(), None, None),
            |(name, color, sprite)| (name, color, Some(sprite)),
        );

        snap.agents.push(AgentRow {
            name,
            color,
            sprite,
            tokens: usage.input + usage.output,
            usd: usage.usd,
            ctx: usage
                .ctx
                .filter(|(_, size)| *size > 0)
                .map(|(used, size)| used * 100 / size),
            live: usage.live,
            ctx_tokens: usage.ctx.map_or(0, |(used, _)| used),
            usd_per_token: usage.usd_per_token,
        });
    }

    snap.limits = crate::engine::limits::read();

    snap
}

pub(super) fn names() -> Names {
    agents::roster_only()
        .into_iter()
        .map(|agent| {
            let sprite = agents::sprite(&agent).desynced(&agent.name);

            (agent.name.clone(), (agent.name, agent.color, sprite))
        })
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
