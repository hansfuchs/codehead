use super::*;
use std::process::Stdio;

#[test]
fn a_hunk_stages_and_unstages_alone() {
    let dir = std::env::temp_dir().join(format!("codehead-hunks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    run_shell(
        &dir,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init",
    );

    // 2 hunks: line 3 changed, line 18 removed
    run_shell(&dir, "seq 1 20 | sed 's/^3$/three/' | sed '/^18$/d' > x.txt");
    run_op(&dir, &Op::StageHunk("x.txt".into(), 3)).unwrap();
    let cached = git(&dir, &["diff", "--cached", "-U0"]).unwrap();

    assert!(cached.contains("+three") && !cached.contains("-18"), "{cached}");

    let unstaged = git(&dir, &["diff", "-U0"]).unwrap();

    assert!(unstaged.contains("-18") && !unstaged.contains("+three"), "{unstaged}");

    // the removal: its line is the one after it (18 in the new file is 19)
    run_op(&dir, &Op::StageHunk("x.txt".into(), 18)).unwrap();

    assert_eq!(git(&dir, &["diff", "-U0"]).unwrap(), "");

    run_op(&dir, &Op::UnstageHunk("x.txt".into(), 3)).unwrap();
    let cached = git(&dir, &["diff", "--cached", "-U0"]).unwrap();

    assert!(!cached.contains("+three") && cached.contains("-18"), "{cached}");
    assert!(
        run_op(&dir, &Op::StageHunk("x.txt".into(), 10)).is_err(),
        "0 hunk at line 10"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn s_stages_only_the_selected_lines_p_stashes_them() {
    let (dir, id, mut view) = prepare_selected_lines();

    assert!(
        wait_op(&mut view).contains("stashed x.txt"),
        "{:?}",
        view.operation_state.note
    );

    let text = std::fs::read_to_string(dir.join("x.txt")).unwrap();

    assert!(text.contains("\nthree\n4\n5\n6\n"), "{text}");
    assert_eq!(view.stash_state.stash.len(), 1);
    assert_eq!(
        (
            view.stash_state.stash[0].path.as_str(),
            view.stash_state.stash[0].lines.as_str(),
            view.stash_state.stash[0].asked
        ),
        ("x.txt", "5 · removed 5", true)
    );

    let ask = view.take_send().unwrap();

    assert!(
        ask.contains("`stash_name`") && ask.contains(&view.stash_state.stash[0].id) && ask.contains("+five"),
        "{ask}"
    );

    // the patch puts it back
    let patch_file = dir.join("p.diff");

    std::fs::write(&patch_file, &view.stash_state.stash[0].patch).unwrap();
    run_shell(&dir, "git apply p.diff && rm p.diff");

    assert!(std::fs::read_to_string(dir.join("x.txt")).unwrap().contains("\nfive\n"));

    // restore: the message to the agent, the entry marked
    let eid = view.stash_state.stash[0].id.clone();
    let _ = view.restore(&eid);
    let msg = view.take_send().unwrap();

    assert!(
        msg.contains("`stash_restored`") && msg.contains("ask the owner"),
        "{msg}"
    );
    assert!(view.stash_state.stash[0].restoring);

    // the agent names it, then reports it restored (the MCP tools)
    stash::set_name(&id, &eid, "five as a word").unwrap();
    view.sync_stash(true);

    assert_eq!(view.stash_state.stash[0].title(), "five as a word");

    stash::restored(&id, &eid).unwrap();
    view.sync_stash(true);

    assert_eq!(view.stash_state.stash, [] as [crate::domain::changes::stash::Entry; 0]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    let _ = std::fs::remove_dir_all(dir);
}

fn prepare_selected_lines() -> (PathBuf, String, View) {
    let dir = std::env::temp_dir().join(format!("codehead-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    run_shell(
        &dir,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init",
    );

    // 1 hunk, 2 changes: 3 → three, 5 → five; a new file of 3 lines
    run_shell(
        &dir,
        "seq 1 20 | sed 's/^3$/three/;s/^5$/five/' > x.txt && printf 'a\\nb\\nc\\n' > n.txt",
    );
    let id = format!("changes-lines-{}", std::process::id());
    let mut staging = real_view(&dir, &id);

    select(&mut staging, "+ three");
    select(&mut staging, "- 3");
    select(&mut staging, "+ b");
    press(&mut staging, KeyCode::Char('s'));

    assert!(
        wait_op(&mut staging).contains("staged the selected lines"),
        "{:?}",
        staging.operation_state.note
    );

    let cached = git(&dir, &["diff", "--cached", "-U0"]).unwrap();

    assert!(
        cached.contains("-3\n+three") && !cached.contains("five") && cached.contains("+b") && !cached.contains("+a"),
        "{cached}"
    );
    assert!(staging.selection_state.sel.is_empty(), "the selection clears");

    // p: `five` out of the worktree, into the stash; its agent (0 known: the lead) asked for a name
    let mut stashing = real_view(&dir, &id);

    select(&mut stashing, "+ five");
    select(&mut stashing, "- 5");
    press(&mut stashing, KeyCode::Char('p'));

    (dir, id, stashing)
}

#[test]
fn stashed_lines_go_back_to_their_spot_in_any_order_after_a_shift() {
    for order in [[0usize, 1], [1, 0]] {
        check_restore_order(order);
    }
}

fn check_restore_order(order: [usize; 2]) {
    let (dir, want) = prepare_restore_lines(order);

    let id = format!("changes-restore-{}-{}", std::process::id(), order[0]);
    let mut staging = real_view(&dir, &id);

    select(&mut staging, "+ ten");
    select(&mut staging, "- 10");
    press(&mut staging, KeyCode::Char('s'));
    wait_op(&mut staging);

    // stash 1: B (C stays) and ten (staged: out of the index too); stash 2: the removed 25
    let mut first_stash = real_view(&dir, &id);

    select(&mut first_stash, "+ B");
    select(&mut first_stash, "+ ten");
    select(&mut first_stash, "- 10");
    press(&mut first_stash, KeyCode::Char('p'));

    assert!(
        wait_op(&mut first_stash).contains("stashed"),
        "{:?}",
        first_stash.operation_state.note
    );

    let mut second_stash = real_view(&dir, &id);

    select(&mut second_stash, "- 25");
    press(&mut second_stash, KeyCode::Char('p'));

    assert!(
        wait_op(&mut second_stash).contains("stashed"),
        "{:?}",
        second_stash.operation_state.note
    );

    let text = std::fs::read_to_string(dir.join("x.txt")).unwrap();

    assert!(
        text.contains("20\nC\n21") && text.contains("\n10\n") && text.contains("\n25\n") && text.contains("5\nA\n6"),
        "{text}"
    );
    assert!(
        !git(&dir, &["diff", "--cached"]).unwrap().contains("ten"),
        "ten out of the index"
    );

    let stash = stash::read(&id);

    assert_eq!(stash.len(), 2);

    // the agent's edits since: 3 lines on top, a line between the spots
    run_shell(
        &dir,
        "{ printf 'h1\\nh2\\nh3\\n'; sed 's/^15$/15\\nmid/' x.txt; } > y && mv y x.txt",
    );

    for i in order {
        std::fs::write(dir.join("p.diff"), &stash[i].patch).unwrap();

        // the restore's steps: `git apply`, else with 1 line of context (the context of an
        // entry can hold the spot of an other entry)
        run_shell(&dir, "{ git apply p.diff || git apply -C1 p.diff; } && rm p.diff");
    }

    let got = std::fs::read_to_string(dir.join("x.txt")).unwrap();
    let want = format!("h1\nh2\nh3\n{}", want.replace("\n15\n", "\n15\nmid\n"));

    assert_eq!(got, want);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    let _ = std::fs::remove_dir_all(dir);
}

fn prepare_restore_lines(order: [usize; 2]) -> (PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("codehead-restore-{}-{}", std::process::id(), order[0]));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    run_shell(
        &dir,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 30 > x.txt && git add -A && git commit -qm init",
    );

    // A after 5, 10 → ten, B C after 20, 25 removed; ten staged
    run_shell(
        &dir,
        "seq 1 30 | sed 's/^5$/5\\nA/;s/^10$/ten/;s/^20$/20\\nB\\nC/;/^25$/d' > x.txt",
    );
    let want = std::fs::read_to_string(dir.join("x.txt")).unwrap();

    assert!(
        want.contains("5\nA\n6") && want.contains("20\nB\nC\n21") && !want.contains("\n25\n"),
        "{want}"
    );

    (dir, want)
}

#[test]
fn git_steps_stage_unstage_commit_and_see_a_stopped_rebase() {
    let dir = std::env::temp_dir().join(format!("codehead-gitops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    run_shell(
        &dir,
        "git init -q -b main && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init",
    );
    run_shell(&dir, "printf 'a\\nB\\n' > x.rs && printf 'n\\n' > y.rs");
    run_op(&dir, &Op::Stage("x.rs".into())).unwrap();
    run_op(&dir, &Op::Stage("y.rs".into())).unwrap();
    let changes = load(&dir).unwrap();

    assert_eq!(
        (changes.staged.get("x.rs"), changes.staged.get("y.rs")),
        (Some(&true), Some(&true))
    );

    // a part staged: another edit after the stage
    run_shell(&dir, "printf 'a\\nB\\nc\\n' > x.rs");

    assert_eq!(load(&dir).unwrap().staged.get("x.rs"), Some(&false));

    run_op(&dir, &Op::Unstage("y.rs".into())).unwrap();

    assert!(!load(&dir).unwrap().staged.contains_key("y.rs"));
    assert_eq!(
        run_op(&dir, &Op::Commit("fix: b is B\n\n- body".into())).unwrap(),
        "committed"
    );

    let log = git(&dir, &["log", "-1", "--format=%B"]).unwrap();

    assert_eq!(log.trim(), "fix: b is B\n\n- body");
    assert!(load(&dir).unwrap().stopped.is_none());

    // a conflict: 2 branches change line 1, rebase stops
    run_shell(
        &dir,
        "git add -A && git commit -qm wip && git checkout -qb other HEAD~1 && printf 'Z\\nB\\n' > x.rs && git commit -qam other && git checkout -q main && printf 'Q\\nB\\nc\\n' > x.rs && git commit -qam main",
    );

    assert!(
        run_op(&dir, &Op::Update("other".into())).is_err(),
        "a conflict: the rebase stops"
    );

    let conflicted = load(&dir).unwrap();

    assert_eq!(conflicted.stopped, Some(vec!["x.rs".to_string()]));

    run_op(&dir, &Op::Abort).unwrap();

    assert!(load(&dir).unwrap().stopped.is_none());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn load_reads_a_real_repo_with_untracked_files() {
    let dir = std::env::temp_dir().join(format!("codehead-changes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    run_shell(
        &dir,
        "git init -q && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init",
    );
    run_shell(&dir, "printf 'a\\nB\\n' > x.rs && printf 'new\\n' > y.md");
    let changes = load(&dir).unwrap();

    assert_eq!(changes.base, "HEAD");

    let paths: Vec<&str> = changes.files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(paths, ["x.rs", "y.md"]);
    assert_eq!(changes.files[1].status, Status::New);

    let _ = std::fs::remove_dir_all(dir);
}

fn run_shell(dir: &Path, script: &str) {
    assert!(
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{script}"
    );
}
