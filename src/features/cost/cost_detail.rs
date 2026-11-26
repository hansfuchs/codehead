use super::{AllThreads, SPARK, ThreadCost, now};

/// Threads of the every-thread list (the costliest).
pub(super) const TOP: usize = 8;

/// Every thread's cost: the top 8 by $, the sums, the last 24h (turn ends since).
pub(crate) fn all_threads() -> AllThreads {
    let mut all = AllThreads::default();
    let since = now().saturating_sub(86_400);
    let mut rows: Vec<ThreadCost> = Vec::new();

    for thread in crate::engine::store::all() {
        let mut row = ThreadCost {
            id: thread.id.clone(),
            title: thread.title.clone(),
            ..Default::default()
        };

        for event in crate::engine::store::events(&thread.id) {
            add_thread_turn(&mut row, &event, since, &mut all.day_usd);
        }

        if row.turns > 0 {
            rows.push(row);
        }
    }

    rows.sort_by(|left, right| right.usd.total_cmp(&left.usd));
    all.usd = rows.iter().map(|row| row.usd).sum();
    all.tokens = rows.iter().map(|row| row.tokens).sum();
    rows.truncate(TOP);
    all.threads = rows;

    all
}

/// BUCKETS buckets of SPAN seconds up to NOW; empty: `·`.
pub(super) fn spark_over(samples: &[(u64, u64)], buckets: usize, span: u64, now: u64) -> String {
    if buckets == 0 || span == 0 {
        return String::new();
    }

    let mut totals = vec![0u64; buckets];
    let start = now.saturating_sub(span);

    for &(at, tok) in samples {
        if at > start {
            let i = ((u128::from(at - start) * buckets as u128 / u128::from(span)) as usize).min(buckets - 1);

            totals[i] += tok;
        }
    }

    let max = totals.iter().copied().max().unwrap_or(0).max(1);

    totals
        .iter()
        .map(|&total| {
            if total == 0 {
                '·'
            } else {
                SPARK[((total * 7) / max) as usize]
            }
        })
        .collect()
}

fn add_thread_turn(row: &mut ThreadCost, event: &crate::engine::store::Event, since: u64, day_usd: &mut f64) {
    let crate::engine::store::Kind::TurnEnd {
        cost, input, output, ..
    } = event.kind
    else {
        return;
    };

    row.usd += cost;
    row.tokens += input + output;
    row.turns += 1;

    if event.ts / 1000 > since {
        *day_usd += cost;
    }
}
