use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::{BUDGET, fixture, measure_split, ms};
use crate::engine::store;
use crate::features::thread_list::{Mode, ThreadList};
use crate::ui::thread_card::Card;

type ArchiveJob = (String, PathBuf, Instant);
type ArchiveInput = (ThreadList, String, PathBuf);

struct ArchiveFixture {
    root: PathBuf,
    repo: PathBuf,
    jobs: RefCell<Vec<ArchiveJob>>,
    job_max: Cell<Duration>,
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fixture() -> ArchiveFixture {
    let root = std::env::temp_dir().join(format!("codehead-perf-archive-{}", std::process::id()));
    let repo = fixture::repo_with_origin(&root);

    std::fs::write(repo.join(".git/info/exclude"), "node_modules/\n").unwrap();
    ArchiveFixture {
        root,
        repo,
        jobs: RefCell::new(Vec::new()),
        job_max: Cell::new(Duration::ZERO),
    }
}

fn git(repo: &Path, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{args:?}"
    );
}

fn done(id: &str, worktree: &Path) -> bool {
    store::load(id).is_some_and(|thread| thread.state == "archived" && thread.worktree_pending.is_some())
        && !worktree.exists()
}

fn settle(fixture: &ArchiveFixture) {
    for (id, worktree, at) in fixture.jobs.borrow_mut().drain(..) {
        let end = Instant::now() + Duration::from_secs(30);

        while !done(&id, &worktree) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }

        assert!(done(&id, &worktree), "archive job of {id}: {:?}", store::load(&id));
        fixture.job_max.set(fixture.job_max.get().max(at.elapsed()));
        let _ = store::delete(&id);
    }
}

fn ignored_files(worktree: &Path) {
    for directory in 0..50 {
        let dir = worktree.join(format!("node_modules/p{directory}"));

        std::fs::create_dir_all(&dir).unwrap();
        write_package(&dir);
    }
}

fn write_package(dir: &Path) {
    for file in 0..100 {
        std::fs::write(dir.join(format!("f{file}.js")), "module.exports = 1;\n").unwrap();
    }
}

fn prepare(fixture: &ArchiveFixture, count: usize) -> ArchiveInput {
    settle(fixture);
    let worktree = fixture.root.join(format!("wt-{count}"));

    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("perf-archive-{count}"),
            &worktree.display().to_string(),
        ],
    );
    ignored_files(&worktree);
    let mut thread = store::Thread::new(
        &format!("perf archive {count}"),
        "task",
        &fixture.repo,
        vec!["zzperf".into()],
    );

    thread.worktree = Some(worktree.display().to_string());
    thread.branch = Some(format!("perf-archive-{count}"));
    thread.save().unwrap();
    let card = Card {
        id: thread.id.clone(),
        title: thread.title.clone(),
        state: "open".into(),
        ..Default::default()
    };
    let mut list = ThreadList::new(Mode::Switch, None, None, vec![card]);

    list.inline = false;
    list.key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL));
    list.key(key(KeyCode::Char('a')));

    (list, thread.id, worktree)
}

fn archive(fixture: &ArchiveFixture, input: ArchiveInput, term: &mut Terminal<TestBackend>) {
    let (mut list, id, worktree) = input;
    let at = Instant::now();

    list.key(key(KeyCode::Enter));
    term.draw(|frame| list.draw(frame, frame.area(), 0)).unwrap();
    assert!(
        list.visible().iter().all(|&i| list.cards[i].id != id),
        "card of {id} still in the active list"
    );
    fixture.jobs.borrow_mut().push((id, worktree, at));
}

pub(super) fn run() {
    let fixture = fixture();
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut count = 0;

    let worst = measure_split(
        "archive: yes → card gone",
        || {
            count += 1;
            prepare(&fixture, count)
        },
        |input| archive(&fixture, input, &mut term),
    );

    settle(&fixture);
    eprintln!(
        "     archive job (off UI)         max {:>7.2} ms",
        ms(fixture.job_max.get())
    );
    let _ = std::fs::remove_dir_all(&fixture.root);

    assert!(
        worst < BUDGET,
        "archive yes → card gone: {:.2} ms ≥ budget {} ms",
        ms(worst),
        BUDGET.as_millis()
    );
}
