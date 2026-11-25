use super::*;

#[test]
#[ignore = "manual draw benchmark"]
fn bench_draw() {
    let Ok(id) = std::env::var("CODEHEAD_BENCH") else {
        return;
    };

    let mut thread = Thread::default();

    for error in crate::engine::store::events(&id) {
        thread.push(&error);
    }

    let all: HashSet<usize> = (0..=thread.runs).collect();
    let none = HashSet::new();

    for (name, runs) in [("folded", &none), ("open runs", &all)] {
        for look in [Look::Bubbles, Look::Lines] {
            benchmark_case(&thread, name, runs, &none, look);
        }
    }
}

fn bench_draw_color(thread: &Thread, look: Look, fold: Fold<'_>, ms: &mut Vec<f64>, rows: &mut usize) {
    let at = std::time::Instant::now();
    let color = draw(thread, look, 120, &Colors::new(), &[], &fold, &Picker::default());

    ms.push(at.elapsed().as_secs_f64() * 1000.0);
    *rows = color.rows.len();
}

fn benchmark_case(thread: &Thread, name: &str, runs: &HashSet<usize>, none: &HashSet<usize>, look: Look) {
    let fold = Fold {
        runs,
        all_runs: false,
        thoughts: Thoughts::Folded,
        open_thoughts: none,
        all_thoughts: false,
    };
    let mut ms = Vec::new();
    let mut rows = 0;

    for _ in 0..3 {
        bench_draw_color(thread, look, fold, &mut ms, &mut rows);
    }

    eprintln!(
        "{look:?} {name}: {} items, {rows} rows, ms per draw: {:.1} / {:.1} / {:.1}",
        thread.items.len(),
        ms[0],
        ms[1],
        ms[2]
    );
}
