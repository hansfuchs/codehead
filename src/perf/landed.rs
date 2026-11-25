use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend};

use super::{BUDGET, RUNS, fixture, ms};
use crate::domain::landed::{self, Land};
use crate::engine::store;
use crate::features::thread_list::{Mode, ThreadList};
use crate::infra::bus::Topic;
use crate::ui::thread_card::Card;

struct LandedFixture {
    root: PathBuf,
    ids: Vec<String>,
    list: ThreadList,
    term: Terminal<TestBackend>,
}

#[derive(Default)]
struct Timing {
    to_read: Duration,
    to_screen: Duration,
    read: Duration,
}

fn shell(dir: &Path, command: &str) {
    let ok = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();

    assert!(ok, "{command}");
}

fn fixture() -> LandedFixture {
    let root = std::env::temp_dir().join(format!("ch-perf-landed-{}", std::process::id()));
    let repo = fixture::repo_with_origin(&root);

    shell(
        &repo,
        "echo node_modules/ >> .gitignore && git commit -q -am ignore && git push -q origin main",
    );
    let ids = threads(&root, &repo);

    crate::infra::watch::start_at(store::threads_dir());
    landed::start_at(store::threads_dir());
    wait_ready(&ids);
    let cards = ids
        .iter()
        .map(|id| Card {
            id: id.clone(),
            title: id.clone(),
            state: "open".into(),
            land: landed::of(id),
            ..Default::default()
        })
        .collect();
    let mut list = ThreadList::new(Mode::Switch, None, None, cards);

    list.inline = false;
    let term = Terminal::new(TestBackend::new(120, 40)).unwrap();

    LandedFixture { root, ids, list, term }
}

fn threads(root: &Path, repo: &Path) -> Vec<String> {
    let mut ids = Vec::new();

    for i in 0..80 {
        let mut thread = store::Thread::new(&format!("perf landed {i}"), "x", repo, vec!["ada".into()]);

        if i < 30 {
            let worktree = root.join(format!("wt{i}"));

            shell(
                repo,
                &format!(
                    "git worktree add -q -b feat{i} {} && cd {} && echo w > w.txt && git add w.txt && git commit -q -m w",
                    worktree.display(),
                    worktree.display()
                ),
            );
            thread.checkouts.push(store::Checkout {
                dir: worktree.display().to_string(),
                branch: Some(format!("feat{i}")),
            });
        }

        thread.save().unwrap();
        ids.push(thread.id);
    }

    ids
}

fn wait_ready(ids: &[String]) {
    let ready = Instant::now();

    while !ids.iter().all(|id| landed::of(id).is_some()) {
        assert!(ready.elapsed() < Duration::from_secs(60), "1st read of 80 threads");
        std::thread::sleep(Duration::from_millis(5));
    }

    eprintln!("     1st read, 80 threads (off UI)  {:>7.2} ms", ms(ready.elapsed()));

    wait_quiet();
}

fn wait_quiet() {
    let ready = Instant::now();

    while !landed::quiet() {
        assert!(ready.elapsed() < Duration::from_secs(60), "worker quiet");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn commit(fixture: &mut LandedFixture, count: usize) -> Timing {
    shell(
        &fixture.root.join("wt0"),
        &format!("echo c{count} > c{count}.txt && git add c{count}.txt && git commit -q -m c{count}"),
    );
    let at = Instant::now();

    while landed::of(&fixture.ids[0]) != Some(Land::Unpushed(count)) {
        assert!(
            at.elapsed() < Duration::from_secs(10),
            "commit {count}: label {:?}",
            landed::of(&fixture.ids[0])
        );
        std::thread::sleep(Duration::from_micros(200));
    }

    let total = at.elapsed();
    let read = landed::last_read();
    let at = Instant::now();

    fixture.list.poll(true);
    fixture
        .term
        .draw(|frame| fixture.list.draw(frame, frame.area(), 0))
        .unwrap();
    let to_screen = at.elapsed();

    assert!(
        fixture
            .list
            .cards
            .iter()
            .any(|card| card.id == fixture.ids[0] && card.land == Some(Land::Unpushed(count)))
    );

    Timing {
        to_read: total.saturating_sub(read),
        to_screen,
        read,
    }
}

fn mark(duration: Duration) -> &'static str {
    if duration < BUDGET { "ok  " } else { "SLOW" }
}

fn commits(fixture: &mut LandedFixture) -> Timing {
    let mut timing = Timing::default();

    for count in 2..=2 + RUNS {
        let result = commit(fixture, count);

        timing.to_read = timing.to_read.max(result.to_read);
        timing.to_screen = timing.to_screen.max(result.to_screen);
        timing.read = timing.read.max(result.read);
    }

    eprintln!(
        "{} commit → read start (watcher)   max {:>7.2} ms",
        mark(timing.to_read),
        ms(timing.to_read)
    );
    eprintln!("     git read of 1 checkout (off UI) max {:>7.2} ms", ms(timing.read));
    eprintln!(
        "{} read end → card drawn          max {:>7.2} ms",
        mark(timing.to_screen),
        ms(timing.to_screen)
    );

    timing
}

fn idle(topics: &Receiver<Topic>) -> (usize, u64) {
    let before = landed::batches();

    std::thread::sleep(Duration::from_secs(3));
    let seen: Vec<Topic> = topics.try_iter().collect();
    let (count, reads) = (seen.len(), landed::batches() - before);

    eprintln!(
        "{} idle 3 s: topics, reads        {count}, {reads}",
        if count + reads as usize == 0 { "ok  " } else { "SLOW" }
    );

    (count, reads)
}

fn install(root: &std::path::Path) {
    for i in 0..20_000 {
        let directory = root.join(format!("p{}", i / 100));
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(directory.join(format!("f{i}.js")), "x");
    }
}

fn storm(fixture: &mut LandedFixture, topics: &Receiver<Topic>) -> (Timing, usize, u64) {
    let node_modules = fixture.root.join("wt1/node_modules");
    let storm = std::thread::spawn(move || install(&node_modules));
    let before = landed::batches();
    let timing = commit(fixture, 3 + RUNS);

    storm.join().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let worktree = crate::infra::watch::canon(&fixture.root.join("wt1"));
    let work = topics
        .try_iter()
        .filter(|topic| matches!(topic, Topic::Work(path) if *path == worktree))
        .count();
    let reads = landed::batches() - before;

    eprintln!(
        "{} install storm: wt1 topics, reads {work}, {reads} (1: the commit in wt0)",
        if work == 0 && reads <= 2 { "ok  " } else { "SLOW" }
    );
    eprintln!(
        "{} storm: commit → read start      max {:>7.2} ms",
        mark(timing.to_read),
        ms(timing.to_read)
    );

    (timing, work, reads)
}

pub(super) fn run() {
    let mut fixture = fixture();
    let timing = commits(&mut fixture);

    wait_quiet();
    let topics = crate::infra::bus::subscribe();
    let (idle_topics, idle_reads) = idle(&topics);
    let (storm, storm_work, storm_reads) = storm(&mut fixture, &topics);
    let _ = std::fs::remove_dir_all(&fixture.root);

    assert!(
        timing.to_read < BUDGET && timing.to_screen < BUDGET,
        "commit → label: {:.2} ms to the read, {:.2} ms to the screen",
        ms(timing.to_read),
        ms(timing.to_screen)
    );
    assert!(
        idle_topics == 0 && idle_reads == 0,
        "idle: {idle_topics} topics, {idle_reads} reads"
    );
    assert!(
        storm_work == 0 && storm_reads <= 2,
        "storm: {storm_work} wt1 topics, {storm_reads} reads"
    );
    assert!(
        storm.to_read < BUDGET && storm.to_screen < BUDGET,
        "storm: commit → label {:.2} ms to the read",
        ms(storm.to_read)
    );
}
