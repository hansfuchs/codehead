use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend};

use super::{BUDGET, fixture, measure_split, ms};
use crate::engine::{client, store};
use crate::screens::start_screen::{Target, start};

struct StartFixture {
    root: PathBuf,
    repo: PathBuf,
    executable: PathBuf,
    threads: RefCell<Vec<String>>,
    agents: Vec<String>,
}

impl Drop for StartFixture {
    fn drop(&mut self) {
        for id in self.threads.borrow().iter() {
            let _ = client::send_within(id, &serde_json::json!({"op": "stop"}), Duration::from_secs(2));
        }

        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> StartFixture {
    let executable = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/release/codehead");

    assert!(
        executable.is_file(),
        "{}: missing, `just perf` builds it",
        executable.display()
    );

    let root = std::env::temp_dir().join(format!("codehead-perf-start-{}", std::process::id()));
    let repo = fixture::repo_with_origin(&root);

    StartFixture {
        root,
        repo,
        executable,
        threads: RefCell::new(Vec::new()),
        agents: vec!["zzperf".to_string()],
    }
}

fn done(id: &str) -> bool {
    let thread = store::load(id).unwrap();
    let task = store::events(id)
        .iter()
        .any(|event| matches!(&event.kind, store::Kind::Prompt { text, .. } if text.starts_with("perf start")));
    let worktree = !thread
        .branch
        .as_deref()
        .is_some_and(|branch| branch.starts_with("perf-"))
        || thread.worktree.as_deref().is_some_and(|path| Path::new(path).is_dir());

    task && !thread.kickoff && thread.worktree_pending.is_none() && !thread.pull && worktree
}

fn settle(fixture: &StartFixture) {
    let Some(id) = fixture.threads.borrow().last().cloned() else {
        return;
    };

    let end = Instant::now() + Duration::from_secs(10);

    while !done(&id) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(done(&id), "host of {id}: {:?}", store::load(&id));
}

fn target(kind: &str, count: usize, repo: &Path) -> Target {
    match kind {
        "dir (in place)" => Target::Dir(repo.display().to_string()),
        "main + pull" => Target::Checkout {
            branch: "main".into(),
            switch: false,
            stash: false,
        },
        _ => Target::Branch(format!("perf-{count}")),
    }
}

fn start_frame(fixture: &StartFixture, kind: &str, count: usize, term: &mut Terminal<TestBackend>) {
    let target = target(kind, count, &fixture.repo);
    let message = format!("perf start {count}");
    let plan = start::plan(&fixture.repo, &fixture.repo, &target);
    let input = start::Start {
        repo: &fixture.repo,
        target: &target,
        message: &message,
        agents: &fixture.agents,
        flows: &[],
        projects: None,
        title_set: false,
    };
    let (thread, note) = start::launch(&input, &plan, |id| client::start_with(id, &fixture.executable)).expect("start");

    fixture.threads.borrow_mut().push(thread.id.clone());
    assert!(note.is_none(), "{note:?}");
    crate::screens::thread_view::open_1st_frame(term, &thread.id, &thread.work_dir()).expect("1st frame");
}

pub(super) fn run() {
    let fixture = fixture();
    let mut term = Terminal::new(TestBackend::new(160, 50)).unwrap();
    let mut count = 0;
    let mut slow = Vec::new();

    for kind in ["dir (in place)", "main + pull", "new worktree"] {
        let worst = measure_split(
            &format!("thread start: {kind}"),
            || settle(&fixture),
            |()| {
                count += 1;
                start_frame(&fixture, kind, count, &mut term);
            },
        );

        if worst >= BUDGET {
            slow.push(format!("{kind}: {:.2} ms", ms(worst)));
        }
    }

    settle(&fixture);
    assert!(
        slow.is_empty(),
        "thread start ≥ budget {} ms: {}",
        BUDGET.as_millis(),
        slow.join(", ")
    );
}
