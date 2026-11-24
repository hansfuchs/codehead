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
    bench_within(name, BUDGET, f);
}

/// `bench` with its own budget (a 1-off action, e.g. a form's save: 50 ms).
fn bench_within<T>(name: &str, budget: Duration, mut f: impl FnMut() -> T) {
    let worst = measure_budget(name, budget, || (), |_| f());
    assert!(worst < budget, "{name}: {:.2} ms ≥ budget {} ms", ms(worst), budget.as_millis());
}

/// `bench` without the assert: the slowest run (several cases in 1 test: each prints its line,
/// the test asserts at its end).
fn measure<T>(name: &str, mut f: impl FnMut() -> T) -> Duration {
    measure_split(name, || (), |_| f())
}

/// `measure` with an untimed PREP before each run (its value goes to F): a run's background work
/// (a host's `git worktree add`) never slows the next run.
fn measure_split<P, T>(name: &str, prep: impl FnMut() -> P, f: impl FnMut(P) -> T) -> Duration {
    measure_budget(name, BUDGET, prep, f)
}

/// `measure_split` against BUDGET (the ok / SLOW mark).
fn measure_budget<P, T>(name: &str, budget: Duration, mut prep: impl FnMut() -> P, mut f: impl FnMut(P) -> T) -> Duration {
    let mut time = || {
        let p = prep();
        let at = Instant::now();
        black_box(f(p));
        at.elapsed()
    };
    let cold = time();
    let warm = (0..RUNS).map(|_| time()).max().unwrap_or_default();
    let worst = cold.max(warm);
    let mark = if worst < budget { "ok  " } else { "SLOW" };
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
    use std::time::{Duration, Instant};

    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    use super::{bench, bench_within, fixture, measure_split, ms, BUDGET, RUNS};
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

    /// A copy key (selection, path, code block): the UI thread's part of `clip::copy` with 64 KB.
    /// The tool: `cat` (the same spawn, pipe and wait as pbcopy / wl-copy, 0 write to the
    /// owner's clipboard). The tool's own run (off the UI thread) prints for reference.
    #[test]
    #[ignore = "perf: just perf"]
    fn clipboard_copy_64k() {
        let text = fixture::RUST.chars().cycle().take(64 << 10).collect::<String>();
        let cat: [(&str, &[&str]); 1] = [("cat", &[])];
        bench("clipboard copy 64 KB", || assert!(crate::clip::copy_with(&cat, &text, false)));
        super::measure("clipboard tool run (off UI)", || crate::clip::copy_with(&cat, &text, true));
    }

    /// The MCP tab's edit form (budget 50 ms per step): its open (`a` + a frame) and its save
    /// (ctrl+s + a frame: the file, the agents picker). The secret: a `${VAR}` reference (a
    /// keychain value: its write runs off the UI thread, 0 write to the owner's keychain here).
    #[test]
    #[ignore = "perf: just perf"]
    fn mcp_edit_form_open_and_save() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::time::Duration;

        const FORM: Duration = Duration::from_millis(50);
        let key = |t: &mut crate::mcptab::McpTab, c: KeyCode, m: KeyModifiers| t.key(KeyEvent::new(c, m));
        let typed = |t: &mut crate::mcptab::McpTab, s: &str| s.chars().for_each(|c| {
            key(t, KeyCode::Char(c), KeyModifiers::NONE);
        });
        let mut term = Terminal::new(TestBackend::new(160, 50)).unwrap();
        let mut frame = |t: &mut crate::mcptab::McpTab| term.draw(|f| t.draw(f, f.area(), true)).map(|_| ()).unwrap();
        bench_within("mcp form open", FORM, || {
            let mut t = crate::mcptab::McpTab::new();
            key(&mut t, KeyCode::Char('a'), KeyModifiers::NONE);
            frame(&mut t);
        });
        let mut n = 0;
        let worst = super::measure_budget(
            "mcp form save",
            FORM,
            || {
                n += 1;
                let mut t = crate::mcptab::McpTab::new();
                key(&mut t, KeyCode::Char('a'), KeyModifiers::NONE);
                typed(&mut t, &format!("perf-{n}"));
                key(&mut t, KeyCode::Tab, KeyModifiers::NONE);
                key(&mut t, KeyCode::Tab, KeyModifiers::NONE);
                typed(&mut t, "npx -y perf-server --port 4000");
                key(&mut t, KeyCode::Tab, KeyModifiers::NONE);
                typed(&mut t, "TOKEN=${PERF_TOKEN}");
                t
            },
            |mut t| {
                key(&mut t, KeyCode::Char('s'), KeyModifiers::CONTROL);
                frame(&mut t);
                t
            },
        );
        assert!(worst < FORM, "mcp form save: {:.2} ms ≥ budget 50 ms", ms(worst));
        let saved = crate::engine::servers::load().unwrap();
        assert!(saved.iter().any(|s| s.name == "perf-1" && s.env.get("TOKEN").map(String::as_str) == Some("${PERF_TOKEN}")), "{saved:?}");
    }

    /// Archive from the ctrl+p menu: the yes (enter) + 1 frame, the card gone from the list in
    /// that step. The thread: a worktree with 5k ignored files (a node_modules); the job (host
    /// stop, stash, worktree out) runs on its own thread, its time prints for reference.
    #[test]
    #[ignore = "perf: just perf"]
    fn archive_yes_to_card_gone() {
        use crate::engine::store;
        use crate::nav::Card;
        use crate::threadlist::{Mode, ThreadList};
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::cell::RefCell;
        use std::path::PathBuf;
        use std::time::{Duration, Instant};

        let root = std::env::temp_dir().join(format!("codehead-perf-archive-{}", std::process::id()));
        let repo = fixture::repo_with_origin(&root);
        std::fs::write(repo.join(".git/info/exclude"), "node_modules/\n").unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success(), "{args:?}");
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        // the archived threads of earlier runs: their job's end
        let jobs: RefCell<Vec<(String, PathBuf, Instant)>> = RefCell::new(Vec::new());
        let mut job_max = Duration::ZERO;
        let mut settle = |jobs: &RefCell<Vec<(String, PathBuf, Instant)>>| {
            for (id, wt, at) in jobs.borrow_mut().drain(..) {
                let end = Instant::now() + Duration::from_secs(30);
                let done = || store::load(&id).is_some_and(|t| t.state == "archived" && t.worktree_pending.is_some()) && !wt.exists();
                while !done() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(2));
                }
                assert!(done(), "archive job of {id}: {:?}", store::load(&id));
                job_max = job_max.max(at.elapsed());
                let _ = store::delete(&id);
            }
        };
        let mut n = 0;
        let worst = measure_split(
            "archive: yes → card gone",
            || {
                settle(&jobs);
                n += 1;
                let wt = root.join(format!("wt-{n}"));
                git(&["worktree", "add", "-q", "-b", &format!("perf-archive-{n}"), &wt.display().to_string()]);
                for d in 0..50 {
                    let dir = wt.join(format!("node_modules/p{d}"));
                    std::fs::create_dir_all(&dir).unwrap();
                    for f in 0..100 {
                        std::fs::write(dir.join(format!("f{f}.js")), "module.exports = 1;\n").unwrap();
                    }
                }
                let mut t = store::Thread::new(&format!("perf archive {n}"), "task", &repo, vec!["zzperf".into()]);
                t.worktree = Some(wt.display().to_string());
                t.branch = Some(format!("perf-archive-{n}"));
                t.save().unwrap();
                let card = Card { id: t.id.clone(), title: t.title.clone(), state: "open".into(), ..Default::default() };
                let mut l = ThreadList::new(Mode::Switch, None, None, vec![card]);
                l.inline = false;
                l.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
                l.key(key(KeyCode::Char('a')));
                (l, t.id, wt)
            },
            |(mut l, id, wt): (ThreadList, String, PathBuf)| {
                let at = Instant::now();
                l.key(key(KeyCode::Enter));
                term.draw(|f| l.draw(f, f.area(), 0)).unwrap();
                assert!(l.visible().iter().all(|&i| l.cards[i].id != id), "card of {id} still in the active list");
                jobs.borrow_mut().push((id, wt, at));
            },
        );
        settle(&jobs);
        eprintln!("     archive job (off UI)         max {:>7.2} ms", ms(job_max));
        let _ = std::fs::remove_dir_all(&root);
        assert!(worst < BUDGET,"archive yes → card gone: {:.2} ms ≥ budget {} ms", ms(worst), BUDGET.as_millis());
    }

    /// The owner's case: an agent commits in its worktree → the card's label. 80 threads, 30
    /// worktrees of 1 repo, the real watcher + landed worker. Budget: change → read start and
    /// read end → card on screen, each < 20 ms; the git read itself prints for reference.
    #[test]
    #[ignore = "perf: just perf"]
    fn landed_refresh_after_commit() {
        use crate::engine::store;
        use crate::landed::{self, Land};
        use crate::nav::Card;
        use crate::threadlist::{Mode, ThreadList};

        let root = std::env::temp_dir().join(format!("ch-perf-landed-{}", std::process::id()));
        let repo = fixture::repo_with_origin(&root);
        let sh = |dir: &std::path::Path, c: &str| {
            let ok = std::process::Command::new("sh").arg("-c").arg(c).current_dir(dir).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success();
            assert!(ok, "{c}");
        };
        sh(&repo, "echo node_modules/ >> .gitignore && git commit -q -am ignore && git push -q origin main");
        let mut ids = Vec::new();
        for i in 0..80 {
            let mut t = store::Thread::new(&format!("perf landed {i}"), "x", &repo, vec!["ada".into()]);
            if i < 30 {
                let wt = root.join(format!("wt{i}"));
                sh(&repo, &format!("git worktree add -q -b feat{i} {} && cd {} && echo w > w.txt && git add w.txt && git commit -q -m w", wt.display(), wt.display()));
                t.checkouts.push(store::Checkout { dir: wt.display().to_string(), branch: Some(format!("feat{i}")) });
            }
            t.save().unwrap();
            ids.push(t.id);
        }
        let target = root.join("wt0");

        crate::watch::start_at(store::threads_dir());
        landed::start_at(store::threads_dir());
        let ready = Instant::now();
        while !ids.iter().all(|id| landed::of(id).is_some()) {
            assert!(ready.elapsed() < Duration::from_secs(60), "1st read of 80 threads");
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!("     1st read, 80 threads (off UI)  {:>7.2} ms", ms(ready.elapsed()));
        // the start's events (worktree setup) read: the worker waits
        while !landed::quiet() {
            assert!(ready.elapsed() < Duration::from_secs(60), "worker quiet");
            std::thread::sleep(Duration::from_millis(5));
        }

        let cards: Vec<Card> = ids.iter().map(|id| Card { id: id.clone(), title: id.clone(), state: "open".into(), land: landed::of(id), ..Default::default() }).collect();
        let mut l = ThreadList::new(Mode::Switch, None, None, cards);
        l.inline = false;
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();

        // commit N in wt0 → (commit → read start, read end → card drawn, the git read)
        let mut commit = |n: usize| {
            sh(&target, &format!("echo c{n} > c{n}.txt && git add c{n}.txt && git commit -q -m c{n}"));
            let at = Instant::now();
            while landed::of(&ids[0]) != Some(Land::Unpushed(n)) {
                assert!(at.elapsed() < Duration::from_secs(10), "commit {n}: label {:?}", landed::of(&ids[0]));
                std::thread::sleep(Duration::from_micros(200));
            }
            let total = at.elapsed();
            let r = landed::last_read();
            let at = Instant::now();
            l.poll(true);
            term.draw(|f| l.draw(f, f.area(), 0)).unwrap();
            let screen = at.elapsed();
            assert!(l.cards.iter().any(|c| c.id == ids[0] && c.land == Some(Land::Unpushed(n))));
            (total.saturating_sub(r), screen, r)
        };
        let (mut to_read, mut to_screen, mut read) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        for n in 2..2 + RUNS + 1 {
            let (a, b, r) = commit(n);
            (to_read, to_screen, read) = (to_read.max(a), to_screen.max(b), read.max(r));
        }
        let mark = |d: Duration| if d < BUDGET { "ok  " } else { "SLOW" };
        eprintln!("{} commit → read start (watcher)   max {:>7.2} ms", mark(to_read), ms(to_read));
        eprintln!("     git read of 1 checkout (off UI) max {:>7.2} ms", ms(read));
        eprintln!("{} read end → card drawn          max {:>7.2} ms", mark(to_screen), ms(to_screen));

        // idle: 0 topic, 0 read
        let topics = crate::bus::subscribe();
        let b0 = landed::batches();
        std::thread::sleep(Duration::from_secs(3));
        let seen: Vec<crate::bus::Topic> = topics.try_iter().collect();
        let (idle_topics, idle_reads) = (seen.len(), landed::batches() - b0);
        eprintln!("{} idle 3 s: topics, reads        {idle_topics}, {idle_reads}", if idle_topics + idle_reads as usize == 0 { "ok  " } else { "SLOW" });

        // install storm: 20 000 files into wt1's node_modules (ignored); a commit in wt0 meanwhile
        let nm = root.join("wt1/node_modules");
        let storm = std::thread::spawn(move || {
            for i in 0..20_000 {
                let d = nm.join(format!("p{}", i / 100));
                let _ = std::fs::create_dir_all(&d);
                let _ = std::fs::write(d.join(format!("f{i}.js")), "x");
            }
        });
        let b0 = landed::batches();
        let (storm_read, storm_screen, _) = commit(3 + RUNS);
        storm.join().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let wt1 = crate::watch::canon(&root.join("wt1"));
        let storm_work = topics.try_iter().filter(|t| matches!(t, crate::bus::Topic::Work(w) if *w == wt1)).count();
        let storm_reads = landed::batches() - b0;
        eprintln!("{} install storm: wt1 topics, reads {storm_work}, {storm_reads} (1: the commit in wt0)", if storm_work == 0 && storm_reads <= 2 { "ok  " } else { "SLOW" });
        eprintln!("{} storm: commit → read start      max {:>7.2} ms", mark(storm_read), ms(storm_read));

        let _ = std::fs::remove_dir_all(&root);
        assert!(to_read < BUDGET && to_screen < BUDGET, "commit → label: {:.2} ms to the read, {:.2} ms to the screen", ms(to_read), ms(to_screen));
        assert!(idle_topics == 0 && idle_reads == 0, "idle: {idle_topics} topics, {idle_reads} reads");
        assert!(storm_work == 0 && storm_reads <= 2, "storm: {storm_work} wt1 topics, {storm_reads} reads");
        assert!(storm_read < BUDGET && storm_screen < BUDGET, "storm: commit → label {:.2} ms to the read", ms(storm_read));
    }

    #[test]
    #[ignore = "perf: just perf"]
    fn fuzzy_rank_10k_paths() {
        let paths = fixture::paths(10_000);
        bench("fuzzy rank 10k paths", || crate::fuzzy::rank(&paths, "eng mod fil", |p| p.clone(), |_| String::new()).len());
    }
}
