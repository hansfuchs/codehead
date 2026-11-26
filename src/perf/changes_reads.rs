//! The changes view on an agent's worktree, the real watcher: how many git reads (and dissolves)
//! an edit, a rewrite with the same text, an atomic write (temp file + rename, as Claude Code
//! writes), a burst of edits and a nested worktree (a subagent's) cost.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend};

use super::fixture;
use crate::engine::store;
use crate::features::changes::View;

/// What the view did while it watched.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Seen {
    pub reads: usize,
    pub dissolves: u64,
    /// Frames with a row that must never show: a temp file of an atomic write, a nested worktree.
    pub stray_rows: usize,
}

impl Seen {
    fn add(&mut self, other: &Seen) {
        self.reads += other.reads;
        self.dissolves += other.dissolves;
        self.stray_rows += other.stray_rows;
    }
}

/// The cases of `run`.
#[derive(Debug, Default)]
pub(super) struct Cases {
    pub edit: Seen,
    pub same_text: Seen,
    pub atomic: Seen,
    pub burst: Seen,
    pub nested: Seen,
}

struct ReadsFixture {
    root: PathBuf,
    work: PathBuf,
    view: View,
    term: Terminal<TestBackend>,
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert!(status.success(), "git {args:?}");
}

fn fixture() -> ReadsFixture {
    let root = std::env::temp_dir().join(format!("ch-perf-reads-{}", std::process::id()));
    let repo = fixture::repo_with_origin(&root);
    let work = root.join("wt");

    git(
        &repo,
        &["worktree", "add", "-q", "-b", "task/reads", &work.display().to_string()],
    );
    crate::infra::watch::start_at(store::threads_dir());
    let view = View::new("changes-reads-0000", &work);
    let term = Terminal::new(TestBackend::new(160, 50)).unwrap();
    let mut fixture = ReadsFixture { root, work, view, term };

    // the 1st read, the watcher's roots (the view's Show), then quiet
    watch(&mut fixture, Duration::from_millis(1500));

    fixture
}

/// Draw the view every ms for TIME: its reads, dissolves, stray rows.
fn watch(fixture: &mut ReadsFixture, time: Duration) -> Seen {
    let mut seen = Seen::default();
    let mut last = fixture.view.last_read();
    let version = fixture.view.data_version();
    let started = Instant::now();

    while started.elapsed() < time {
        fixture
            .term
            .draw(|frame| fixture.view.draw(frame, frame.area()))
            .unwrap();
        let read = fixture.view.last_read();

        seen.reads += usize::from(read != last);
        last = read;
        seen.stray_rows += fixture
            .view
            .changes
            .files
            .iter()
            .filter(|file| file.path.contains(".tmp.") || file.path.starts_with(".claude/"))
            .count();
        std::thread::sleep(Duration::from_millis(1));
    }

    seen.dissolves = fixture.view.data_version() - version;

    seen
}

/// Claude Code's write: `<file>.tmp.<pid>.<12 hex>` beside the file, synced, renamed over it.
fn atomic_write(path: &Path, text: &str) {
    let temp = PathBuf::from(format!("{}.tmp.{}.0123456789ab", path.display(), std::process::id()));
    let mut file = std::fs::File::create(&temp).unwrap();

    file.write_all(text.as_bytes()).unwrap();
    file.sync_all().unwrap();
    std::fs::rename(&temp, path).unwrap();
}

fn report(name: &str, seen: &Seen) {
    eprintln!(
        "     {name:<36} reads {:>2}  dissolves {:>2}  stray rows {:>4}",
        seen.reads, seen.dissolves, seen.stray_rows
    );
}

fn atomic_writes(fixture: &mut ReadsFixture) -> Seen {
    let mut atomic = Seen::default();
    let target = fixture.work.join("src/f2.rs");

    for i in 0..10 {
        atomic_write(&target, &format!("fn f2() {{ {i} }}\n"));
        atomic.add(&watch(fixture, Duration::from_millis(300)));
    }

    atomic.add(&watch(fixture, Duration::from_secs(2)));
    atomic
}

fn burst(fixture: &mut ReadsFixture) -> Seen {
    let work = fixture.work.clone();

    let writer = std::thread::spawn(move || {
        for i in 0..20 {
            std::fs::write(work.join(format!("src/f{}.rs", 10 + i)), format!("fn g{i}() {{}}\n")).unwrap();
            std::thread::sleep(Duration::from_millis(25));
        }
    });

    let seen = watch(fixture, Duration::from_secs(3));

    writer.join().unwrap();
    seen
}

/// A subagent's worktree inside the checkout: added, 1 edit in it, removed.
fn nested(fixture: &mut ReadsFixture) -> Seen {
    let inner = fixture.work.join(".claude/worktrees/agent-1");
    let inner_text = inner.display().to_string();
    let mut seen = Seen::default();

    git(&fixture.work, &["worktree", "add", "-q", "-b", "task/sub", &inner_text]);
    seen.add(&watch(fixture, Duration::from_secs(1)));
    std::fs::write(inner.join("src/f3.rs"), "fn f3() { 3 }\n").unwrap();
    seen.add(&watch(fixture, Duration::from_secs(1)));
    git(&fixture.work, &["worktree", "remove", "--force", &inner_text]);
    seen.add(&watch(fixture, Duration::from_secs(1)));

    seen
}

pub(super) fn run() -> Cases {
    let mut fixture = fixture();
    let mut cases = Cases::default();
    let work = fixture.work.clone();

    std::fs::write(work.join("src/f1.rs"), "fn f1() { 1 }\n").unwrap();
    cases.edit = watch(&mut fixture, Duration::from_secs(2));
    report("1 edit", &cases.edit);

    // a formatter's rewrite: the same text, a new stat
    std::fs::write(work.join("src/f4.rs"), "fn f4() {}\n").unwrap();
    cases.same_text = watch(&mut fixture, Duration::from_secs(2));
    report("1 rewrite, same text", &cases.same_text);

    cases.atomic = atomic_writes(&mut fixture);
    report("10 atomic writes (300 ms apart)", &cases.atomic);
    cases.burst = burst(&mut fixture);
    report("burst: 20 edits in 500 ms", &cases.burst);
    cases.nested = nested(&mut fixture);
    report("nested worktree: add, edit, remove", &cases.nested);
    let _ = std::fs::remove_dir_all(&fixture.root);

    cases
}
