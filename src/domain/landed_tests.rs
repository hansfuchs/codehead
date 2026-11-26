use super::*;
use std::process::{Command, Stdio};

fn sh(dir: &Path, script: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();

    assert!(ok, "{script}");
}

/// A remote, its clone (main checkout) and a worktree on branch `feat` with 1 commit.
fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("ch-landed-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();
    sh(
        &base,
        "git init -q --bare -b main o.git && git clone -q o.git r 2>/dev/null && cd r && printf 'a\\nb\\nc\\n' > f && git add f && git commit -q -m 1 && git push -q origin main && git remote set-head origin main",
    );
    let repo = base.join("r");
    let wt = base.join("w");

    sh(
        &repo,
        "git worktree add -q -b feat ../w && cd ../w && echo d >> f && git commit -q -am feat",
    );

    (base, repo, wt)
}

fn land(dir: &Path, prs: &[PrSeen]) -> Land {
    checkout(dir, None, true, prs, &mut Seen::new())
}

fn pr(state: State, oid: &str) -> PrSeen {
    PrSeen {
        label: "r#7".into(),
        state,
        head: "feat".into(),
        head_oid: oid.into(),
    }
}

/// The path of a branch: dirty → unpushed → pushed → PR open → squash merged on the remote:
/// shipped. The main checkout: on main (dirty or not); a dir without git: 0 code.
#[test]
fn a_branch_goes_from_open_to_shipped() {
    let (base, repo, wt) = setup("path");

    assert_eq!(land(&repo, &[]), Land::Main, "main checkout");

    sh(&repo, "echo x > loose.txt");

    assert_eq!(land(&repo, &[]), Land::Main, "main checkout, dirty");

    sh(&repo, "rm loose.txt");

    assert!(!Land::Main.open());
    assert_eq!(land(&base, &[]), Land::NoCode, "0 git");

    // a new thread's branch, 0 commit yet: 0 code (not shipped); a branch made at commits
    // not in the base (a colleague's, fetched): open
    sh(&repo, "git worktree add -q -b fresh ../f && git branch theirs feat");

    assert_eq!(land(&base.join("f"), &[]), Land::NoCode);
    assert_eq!(
        checkout(&repo, Some("theirs"), false, &[], &mut Seen::new()),
        Land::Unpushed(1)
    );

    sh(&wt, "echo x > new.txt");

    assert_eq!(land(&wt, &[]), Land::Dirty(1));

    sh(&wt, "rm new.txt && echo e >> f && git commit -q -am 2nd");

    assert_eq!(land(&wt, &[]), Land::Unpushed(2));

    sh(&wt, "git push -q origin feat");

    assert_eq!(land(&wt, &[]), Land::Unmerged);

    let head = oid(&wt, "HEAD").unwrap();

    assert_eq!(land(&wt, &[pr(State::Open, &head)]), Land::PrOpen("r#7".into()));
    assert!(Land::PrOpen("r#7".into()).open() && !Land::Landed.open());

    // squash merge of the 2 commits on the remote: 1 other commit, same content (0 cherry
    // match: the merge-tree check finds it)
    sh(
        &base,
        "git clone -q o.git s 2>/dev/null && cd s && printf 'd\\ne\\n' >> f && git commit -q -am squash && git push -q origin main",
    );
    sh(&repo, "git fetch -q origin");

    assert_eq!(land(&wt, &[pr(State::Merged, &head)]), Land::Landed);
    assert_eq!(land(&wt, &[]), Land::Landed, "content check alone: squash found");

    std::fs::remove_dir_all(&base).unwrap();
}

/// A local merge into main (main unpushed): shipped (D2). A closed PR, branch as at the
/// close: abandoned; a new commit after the close: unpushed.
#[test]
fn local_merge_ships_closed_pr_abandons() {
    let (base, repo, wt) = setup("local");

    sh(&repo, "git merge -q --no-ff -m m feat");

    assert_eq!(land(&wt, &[]), Land::Landed);

    let (base2, _, wt2) = setup("closed");

    sh(&wt2, "git push -q origin feat");
    let head = oid(&wt2, "HEAD").unwrap();

    assert_eq!(land(&wt2, &[pr(State::Closed, &head)]), Land::Abandoned("r#7".into()));
    assert!(!Land::Abandoned("r#7".into()).open());

    sh(&wt2, "echo e >> f && git commit -q -am more");

    assert_eq!(land(&wt2, &[pr(State::Closed, &head)]), Land::Unpushed(1));

    // a merged PR, new commits after the merge: open (the old archive offer's bug)
    sh(&wt2, "git push -q origin feat");

    assert_eq!(land(&wt2, &[pr(State::Merged, &head)]), Land::Unmerged);

    std::fs::remove_dir_all(&base).unwrap();
    std::fs::remove_dir_all(&base2).unwrap();
}

/// Stale: 0 code, agents not done, idle 3+ days; the days left to the auto-archive.
#[test]
fn stale_after_three_days_archived_after_thirty() {
    let (now, day) = (100 * DAY_MS, DAY_MS);

    assert!(stale(Some(&Land::NoCode), false, now - 3 * day - 1, now));
    assert!(!stale(Some(&Land::NoCode), false, now - 2 * day, now), "recent");
    assert!(!stale(Some(&Land::NoCode), true, now - 9 * day, now), "done: shipped");
    assert!(
        stale(Some(&Land::Main), true, now - 3 * day - 1, now),
        "on main, done: stale"
    );
    assert!(!stale(Some(&Land::Main), false, now - 2 * day, now), "on main, recent");
    assert!(
        !stale(Some(&Land::Unpushed(1)), false, now - 9 * day, now),
        "code: open"
    );
    assert!(!stale(None, false, 0, now), "0 state read");
    assert_eq!(archive_in_days(now - 5 * day - 1, now), 25);
    assert_eq!(archive_in_days(now - 31 * day, now), 0);
}

/// A thread: the worst of its checkouts; archived: its kept branch in the repo, 0 dirty read.
#[test]
fn thread_takes_the_worst_checkout() {
    let (base, repo, wt) = setup("thread");
    let mut open = store::Thread::new("t", "x", &repo, vec!["ada".into()]);

    assert_eq!(thread(&open, &[], &mut Seen::new()), Land::Main, "on main");

    open.checkouts.push(store::Checkout {
        dir: wt.display().to_string(),
        branch: Some("feat".into()),
    });

    assert_eq!(thread(&open, &[], &mut Seen::new()), Land::Unpushed(1));

    let mut archived = store::Thread::new("a", "x", &repo, vec!["ada".into()]);

    archived.state = "archived".into();
    archived.repo = Some(repo.display().to_string());
    archived.worktree_pending = Some("feat".into());
    sh(&wt, "echo x > new.txt");

    assert_eq!(
        thread(&archived, &[], &mut Seen::new()),
        Land::Unpushed(1),
        "archived: 0 dirty read"
    );

    std::fs::remove_dir_all(&base).unwrap();
}

/// The worker reads only what a topic names: a work tree edit → dirty, a git dir change →
/// refs (a commit, a push); an own action's expected state shows at once.
#[test]
fn worker_reads_what_a_topic_names() {
    let (base, repo, wt) = setup("worker");
    let threads = base.join("threads");
    let mut thread = store::Thread::new("w", "x", &repo, vec!["ada".into()]);

    thread.checkouts.push(store::Checkout {
        dir: wt.display().to_string(),
        branch: Some("feat".into()),
    });
    std::fs::create_dir_all(threads.join(&thread.id)).unwrap();
    std::fs::write(
        threads.join(&thread.id).join("thread.json"),
        serde_json::to_vec(&thread).unwrap(),
    )
    .unwrap();

    let mut worker = Worker::new(threads.clone());

    worker.init();

    assert_eq!(of(&thread.id), Some(Land::Unpushed(1)));

    let (top, common, _) = crate::infra::watch::git_dirs(&wt).unwrap();
    let (top, common) = (canon(&top), canon(&common));

    sh(&wt, "echo x > new.txt");
    let ids = worker.apply(vec![In::Bus(Topic::Work(top.clone()))]);

    assert!(worker.publish(&ids));
    assert_eq!(of(&thread.id), Some(Land::Dirty(1)), "edit: status only");

    sh(&wt, "git add new.txt && git commit -q -m 2");
    let ids = worker.apply(vec![
        In::Bus(Topic::Git(common.clone(), bus::Git::All)),
        In::Bus(Topic::Work(top.clone())),
    ]);

    worker.publish(&ids);

    assert_eq!(of(&thread.id), Some(Land::Unpushed(2)), "commit");

    sh(&wt, "git push -q origin feat");
    let ids = worker.apply(vec![In::Bus(Topic::Git(common.clone(), bus::Git::All))]);

    worker.publish(&ids);

    assert_eq!(of(&thread.id), Some(Land::Unmerged), "push");

    // another repo's change: 0 read, 0 change
    assert_eq!(
        worker.apply(vec![In::Bus(Topic::Git(
            PathBuf::from("/elsewhere/.git"),
            bus::Git::All
        ))]),
        [] as [std::string::String; 0]
    );

    // optimistic: at once; a refresh puts the truth back
    set(&thread.id, Land::Unpushed(9));

    assert_eq!(of(&thread.id), Some(Land::Unpushed(9)));

    let ids = worker.apply(vec![In::Refresh(thread.id.clone())]);

    worker.publish(&ids);

    assert_eq!(of(&thread.id), Some(Land::Unmerged));

    std::fs::remove_dir_all(&base).unwrap();
}

/// Dirty count: a subagent's worktree and a nested repo inside the checkout, temp files of
/// atomic writes: 0; each real untracked file and edit: 1.
#[test]
fn nested_worktrees_and_temp_files_are_not_dirty() {
    let dir = std::env::temp_dir().join(format!("ch-landed-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();
    sh(
        &dir,
        "git init -q -b main && echo a > a.txt && git add -A && git commit -qm init \
         && git worktree add -q -b sub .claude/worktrees/agent-1 && mkdir -p vendor/lib && git -C vendor/lib init -q \
         && echo t > a.txt.tmp.4821.0123456789ab",
    );

    assert_eq!(dirty_count(&dir), 0);

    sh(&dir, "echo b >> a.txt && mkdir -p docs && echo n > docs/new.md");

    assert_eq!(dirty_count(&dir), 2);

    let _ = std::fs::remove_dir_all(&dir);
}
