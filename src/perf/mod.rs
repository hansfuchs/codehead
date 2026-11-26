//! Perf suite: every case must stay under BUDGET (20 ms), cold call included. Run: `just perf`
//! (release build, 1 thread). A new hot path: 1 more `#[test]` with `bench(name, || ...)`.
//! The cases are `#[ignore]`: `just check` (debug build) skips them.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// The budget of 1 step: a key, a draw, a tick, a screen change.
const BUDGET: Duration = Duration::from_millis(20);
/// Warm runs after the cold 1st run.
const RUNS: usize = 5;

/// Time RUN: 1 cold run, RUNS warm runs. Fail if the slowest run reaches BUDGET.
fn bench<T>(name: &str, run: impl FnMut() -> T) {
    bench_within(name, BUDGET, run);
}

/// `bench` with its own budget (a 1-off action, e.g. a form's save: 50 ms).
fn bench_within<T>(name: &str, budget: Duration, mut run: impl FnMut() -> T) {
    let worst = measure_budget(name, budget, || (), |()| run());

    assert!(
        worst < budget,
        "{name}: {:.2} ms ≥ budget {} ms",
        ms(worst),
        budget.as_millis()
    );
}

/// `bench` without the assert: the slowest run (several cases in 1 test: each prints its line,
/// the test asserts at its end).
fn measure<T>(name: &str, mut run: impl FnMut() -> T) -> Duration {
    measure_split(name, || (), |()| run())
}

/// `measure` with an untimed PREP before each run (its value goes to RUN): a run's background work
/// (a host's `git worktree add`) never slows the next run.
fn measure_split<P, T>(name: &str, prep: impl FnMut() -> P, run: impl FnMut(P) -> T) -> Duration {
    measure_budget(name, BUDGET, prep, run)
}

/// `measure_split` against BUDGET (the ok / SLOW mark).
fn measure_budget<P, T>(
    name: &str,
    budget: Duration,
    mut prep: impl FnMut() -> P,
    mut run: impl FnMut(P) -> T,
) -> Duration {
    let mut time = || {
        let input = prep();
        let at = Instant::now();

        black_box(run(input));
        at.elapsed()
    };

    let cold = time();
    let warm = (0..RUNS).map(|_| time()).max().unwrap_or_default();
    let worst = cold.max(warm);
    let mark = if worst < budget { "ok  " } else { "SLOW" };

    eprintln!(
        "{mark} {name:<28} cold {:>7.2} ms  warm max {:>7.2} ms",
        ms(cold),
        ms(warm)
    );

    worst
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Fixtures: deterministic, sized like a long real session.
mod fixture;

#[path = "perf_cases_tests.rs"]
mod cases;

mod archive;
mod changes_reads;
mod landed;
mod start;
