//! Perf suite: every case must stay under BUDGET (20 ms), cold call included. Run: `just perf`
//! (release build, 1 thread). A new hot path: 1 more `#[test]` with `bench(name, || ...)`.
//! The cases are `#[ignore]`: `just check` (debug build) skips them.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// The budget of 1 step: a key, a draw, a tick, a screen change.
const BUDGET: Duration = Duration::from_millis(20);
/// Warm runs after the cold 1st run.
const RUNS: usize = 5;

/// Time F: 1 cold run, RUNS warm runs. Fail if the slowest run reaches BUDGET.
fn bench<T>(name: &str, f: impl FnMut() -> T) {
    let worst = measure(name, f);
    assert!(worst < BUDGET, "{name}: {:.2} ms ≥ budget {} ms", ms(worst), BUDGET.as_millis());
}

/// `bench` without the assert: the slowest run (several cases in 1 test: each prints its line,
/// the test asserts at its end).
fn measure<T>(name: &str, mut f: impl FnMut() -> T) -> Duration {
    measure_split(name, || (), |_| f())
}

/// `measure` with an untimed PREP before each run (its value goes to F): a run's background work
/// (a host's `git worktree add`) never slows the next run.
fn measure_split<P, T>(name: &str, mut prep: impl FnMut() -> P, mut f: impl FnMut(P) -> T) -> Duration {
    let mut time = || {
        let p = prep();
        let at = Instant::now();
        black_box(f(p));
        at.elapsed()
    };
    let cold = time();
    let warm = (0..RUNS).map(|_| time()).max().unwrap_or_default();
    let worst = cold.max(warm);
    let mark = if worst < BUDGET { "ok  " } else { "SLOW" };
    eprintln!("{mark} {name:<28} cold {:>7.2} ms  warm max {:>7.2} ms", ms(cold), ms(warm));
    worst
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// Fixtures: deterministic, sized like a long real session.
mod fixture {
    use crate::engine::store::{Event, Kind};
    use serde_json::json;

    /// Real Rust source: 1 big file of this repo.
    pub const RUST: &str = include_str!("app.rs");

    pub fn markdown(blocks: usize) -> String {
        let mut s = String::new();
        for i in 0..blocks {
            s.push_str(&format!(
                "## step {i}\n\nRead `src/app.rs` and **fixed** the [link](https://codehead.sh) for @ada.\n\n\
                 - item one with *emphasis*\n- item two with `code`\n\n\
                 | col | val |\n|---|---|\n| a | {i} |\n\n```rust\nfn f{i}(x: u32) -> u32 {{ x + {i} }}\n```\n\n"
            ));
        }
        s
    }

    /// N turns: prompt, thought, 3 tool calls + results, text.
    pub fn events(turns: usize) -> Vec<Event> {
        let base = 1_795_090_200_000u64;
        let mut v = Vec::new();
        let ev = |ts: u64, agent: Option<&str>, kind: Kind| Event { ts, agent: agent.map(Into::into), kind };
        for i in 0..turns {
            let ts = base + i as u64 * 60_000;
            v.push(ev(ts, None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: format!("turn {i}: fix the build") }));
            v.push(ev(ts + 1, Some("ada"), Kind::TurnStart));
            v.push(ev(ts + 2, Some("ada"), Kind::Thought { text: "check the failing test first".into() }));
            for c in 0..3 {
                let id = format!("t{i}-{c}");
                v.push(ev(ts + 3, Some("ada"), Kind::Tool { id: id.clone(), name: "Bash".into(), input: json!({"command": "cargo test", "description": "run tests"}) }));
                v.push(ev(ts + 4, Some("ada"), Kind::ToolResult { id, ok: true, output: "test result: ok. 120 passed\n".repeat(20) }));
            }
            v.push(ev(ts + 5, Some("ada"), Kind::Text { text: markdown(1) }));
        }
        v
    }

    /// N files, each 4 hunks of 40 changed lines.
    pub fn diff(files: usize) -> String {
        let mut s = String::new();
        for f in 0..files {
            s.push_str(&format!("diff --git a/src/f{f}.rs b/src/f{f}.rs\nindex 1111111..2222222 100644\n--- a/src/f{f}.rs\n+++ b/src/f{f}.rs\n"));
            for h in 0..4 {
                s.push_str(&format!("@@ -{0},40 +{0},40 @@ fn h{h}()\n", h * 100 + 1));
                for l in 0..20 {
                    s.push_str(&format!("-    let old_{l} = compute({l}, \"old\");\n+    let new_{l} = compute({l}, \"new\");\n"));
                }
            }
        }
        s
    }

    /// A repo on main with an origin (bare, local: 0 network), 50 files, an ignored `.env`: ROOT
    /// holds `repo/` and `origin.git/`.
    pub fn repo_with_origin(root: &std::path::Path) -> std::path::PathBuf {
        let _ = std::fs::remove_dir_all(root);
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        for i in 0..50 {
            std::fs::write(repo.join(format!("src/f{i}.rs")), format!("fn f{i}() {{}}\n")).unwrap();
        }
        std::fs::write(repo.join(".gitignore"), ".env*\n").unwrap();
        std::fs::write(repo.join(".env"), "KEY=1\n").unwrap();
        let sh = |dir: &std::path::Path, c: &str| {
            let ok = std::process::Command::new("sh").arg("-c").arg(c).current_dir(dir).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success();
            assert!(ok, "{c}");
        };
        sh(root, "git init -q --bare origin.git");
        sh(&repo, "git init -q -b main && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 0 && git add -A && git -c user.email=t@t -c user.name=t commit -qm init && git remote add origin ../origin.git && git push -q -u origin main && git remote set-head origin main");
        repo
    }

    pub fn paths(n: usize) -> Vec<String> {
        let dirs = ["src", "src/engine", "src/changes", "site/src/content/docs", "vendor/hyperpolyglot/src"];
        (0..n).map(|i| format!("{}/module_{i}/file_{}.rs", dirs[i % dirs.len()], i * 7 % 101)).collect()
    }
}

mod cases {
    use std::collections::HashSet;

    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    use super::{bench, fixture, measure_split, ms, BUDGET};
    use crate::ask::Picker;
    use crate::engine::store::Event;
    use crate::stream::{self, Colors, Fold, Look, Thoughts, Thread};

    fn thread(turns: usize) -> Thread {
        let mut t = Thread::default();
        for e in &fixture::events(turns) {
            t.push(e);
        }
        t
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn log_parse_2k_events() {
        let lines: String = fixture::events(250).iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();
        bench("log parse 2k events", || lines.lines().filter_map(|l| serde_json::from_str::<Event>(l).ok()).count());
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn thread_fold_2k_events() {
        let events = fixture::events(250);
        bench("thread fold 2k events", || {
            let mut t = Thread::default();
            for e in &events {
                t.push(e);
            }
            t
        });
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn thread_draw_250_turns() {
        let t = thread(250);
        let none = HashSet::new();
        let fold = Fold { runs: &none, all_runs: false, thoughts: Thoughts::Hidden, open_thoughts: &none, all_thoughts: false };
        let colors = Colors::from([("ada".to_string(), ratatui::style::Color::Cyan)]);
        for look in [Look::Bubbles, Look::Lines] {
            bench(&format!("thread draw 250 turns {look:?}"), || stream::draw(&t, look, 120, &colors, &[], &fold, &Picker::default()).rows.len());
        }
    }

    /// ctrl+f in a long chat: 1 key of the query runs the match over every message (cold: the
    /// texts of the items built once).
    #[test]
    #[ignore = "perf: just perf"]
    fn chat_search_1k_turns() {
        let t = thread(1000);
        let mut s = crate::chatsearch::ChatSearch::default();
        s.open();
        let mut n = 0u32;
        bench(&format!("chat search {} items (1 key)", t.items.len()), || {
            n += 1;
            s.ta.set_text(if n.is_multiple_of(2) { "deploy fix" } else { "deploy fi" });
            s.refresh(&t)
        });
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn frame_paint_200x60() {
        let lines: Vec<_> = crate::render::markdown(&fixture::markdown(20), 200, &[]).into_iter().map(|r| r.line).collect();
        let mut term = Terminal::new(TestBackend::new(200, 60)).unwrap();
        bench("frame paint 200x60", || term.draw(|f| f.render_widget(Paragraph::new(lines.clone()), f.area())).map(|_| ()));
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn markdown_100_blocks() {
        let md = fixture::markdown(100);
        bench("markdown 100 blocks", || crate::render::markdown(&md, 100, &[]).len());
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn highlight_rust_200_lines() {
        // 1st call: the syntax set loads; a new text each run: 0 cache hits
        let block: String = fixture::RUST.lines().take(200).collect::<Vec<_>>().join("\n");
        let mut n = 0;
        bench("highlight rust 200 lines", || {
            n += 1;
            crate::code::highlight(&format!("{block}\n// {n}"), "rust")
        });
    }

    /// The 1st code block of a session: the syntax set loads. Valid only as the 1st case that
    /// highlights: libtest runs the cases by name, `a_` sorts it 1st.
    #[test]
    #[ignore = "perf: just perf"]
    fn a_syntax_set_load() {
        bench("syntax set load (1st block)", || crate::code::highlight("fn main() {}", "rust"));
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn diff_parse_50_files() {
        let d = fixture::diff(50);
        bench("diff parse 50 files", || crate::changes::parse(&d).len());
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn word_marks_long_line() {
        let old: Vec<char> = "let value = compute(alpha, beta, gamma); ".repeat(40).chars().collect();
        let new: Vec<char> = "let value = compute(alpha, delta, gamma); ".repeat(40).chars().collect();
        bench("word marks 1.6k chars", || crate::changes::word_marks(&old, &new));
    }

    /// The new-thread form's submit → the thread view's 1st frame: the plan, thread.json, the host
    /// start, the view (as `main.rs` runs it). 1 case per target kind. The host: the release
    /// binary (`just perf` builds it), 0 roster agent (its delivery fails, 0 harness runs).
    /// Between runs (untimed): the last host logged its task, made its worktree, pulled main.
    #[test]
    #[ignore = "perf: just perf"]
    fn thread_start_submit_to_1st_frame() {
        use crate::app::Target;
        use crate::engine::{client, store};
        use std::cell::RefCell;
        use std::path::Path;
        use std::time::{Duration, Instant};

        let exe = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/release/codehead");
        assert!(exe.is_file(), "{}: missing, `just perf` builds it", exe.display());
        let root = std::env::temp_dir().join(format!("codehead-perf-start-{}", std::process::id()));
        let repo = fixture::repo_with_origin(&root);
        // the hosts stop, the fixture goes: also after a failed case
        struct Clean(std::path::PathBuf, RefCell<Vec<String>>);
        impl Drop for Clean {
            fn drop(&mut self) {
                for id in self.1.borrow().iter() {
                    let _ = client::send_within(id, &serde_json::json!({"op": "stop"}), Duration::from_secs(2));
                }
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let clean = Clean(root.clone(), RefCell::new(Vec::new()));
        // the work the UI left to the host of thread ID: done
        let done = |id: &str| {
            let t = store::load(id).unwrap();
            let task = store::events(id).iter().any(|e| matches!(&e.kind, store::Kind::Prompt { text, .. } if text.starts_with("perf start")));
            let wt = !t.branch.as_deref().is_some_and(|b| b.starts_with("perf-")) || t.worktree.as_deref().is_some_and(|w| Path::new(w).is_dir());
            task && !t.kickoff && t.worktree_pending.is_none() && !t.pull && wt
        };
        let settle = || {
            let Some(id) = clean.1.borrow().last().cloned() else { return };
            let end = Instant::now() + Duration::from_secs(10);
            while !done(&id) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(done(&id), "host of {id}: {:?}", store::load(&id));
        };
        let agents = vec!["zzperf".to_string()];
        let mut term = Terminal::new(TestBackend::new(160, 50)).unwrap();
        let mut n = 0;
        let mut slow = Vec::new();
        for kind in ["dir (in place)", "main + pull", "new worktree"] {
            let worst = measure_split(&format!("thread start: {kind}"), settle, |()| {
                n += 1;
                let target = match kind {
                    "dir (in place)" => Target::Dir(repo.display().to_string()),
                    "main + pull" => Target::Checkout { branch: "main".into(), switch: false, stash: false },
                    _ => Target::Branch(format!("perf-{n}")),
                };
                let message = format!("perf start {n}");
                let plan = crate::start::plan(&repo, &repo, &target);
                let s = crate::start::Start { repo: &repo, target: &target, message: &message, agents: &agents, flows: &[] };
                let (t, note) = crate::start::launch(&s, &plan, |id| client::start_with(id, &exe)).expect("start");
                clean.1.borrow_mut().push(t.id.clone());
                assert!(note.is_none(), "{note:?}");
                crate::view::open_1st_frame(&mut term, &t.id, &t.work_dir()).expect("1st frame");
            });
            if worst >= BUDGET {
                slow.push(format!("{kind}: {:.2} ms", ms(worst)));
            }
        }
        settle();
        assert!(slow.is_empty(), "thread start ≥ budget {} ms: {}", BUDGET.as_millis(), slow.join(", "));
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn fuzzy_rank_10k_paths() {
        let paths = fixture::paths(10_000);
        bench("fuzzy rank 10k paths", || crate::fuzzy::rank(&paths, "eng mod fil", |p| p.clone(), |_| String::new()).len());
    }
}
