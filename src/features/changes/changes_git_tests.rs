use super::*;
use std::process::Stdio;

#[test]
fn a_hunk_stages_and_unstages_alone() {
    let directory = std::env::temp_dir().join(format!("codehead-hunks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    run_shell(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init",
    );

    // 2 hunks: line 3 changed, line 18 removed
    run_shell(&directory, "seq 1 20 | sed 's/^3$/three/' | sed '/^18$/d' > x.txt");
    run_op(&directory, &Op::StageHunk("x.txt".into(), 3)).unwrap();
    let cached = git(&directory, &["diff", "--cached", "-U0"]).unwrap();

    assert!(cached.contains("+three") && !cached.contains("-18"), "{cached}");

    let unstaged = git(&directory, &["diff", "-U0"]).unwrap();

    assert!(unstaged.contains("-18") && !unstaged.contains("+three"), "{unstaged}");

    // the removal: its line is the one after it (18 in the new file is 19)
    run_op(&directory, &Op::StageHunk("x.txt".into(), 18)).unwrap();

    assert_eq!(git(&directory, &["diff", "-U0"]).unwrap(), "");

    run_op(&directory, &Op::UnstageHunk("x.txt".into(), 3)).unwrap();
    let cached = git(&directory, &["diff", "--cached", "-U0"]).unwrap();

    assert!(!cached.contains("+three") && cached.contains("-18"), "{cached}");
    assert!(
        run_op(&directory, &Op::StageHunk("x.txt".into(), 10)).is_err(),
        "0 hunk at line 10"
    );

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn s_stages_only_the_selected_lines_p_stashes_them() {
    let (directory, id, mut value_value) = prepare_selected_lines();

    assert!(
        wait_op(&mut value_value).contains("stashed x.txt"),
        "{:?}",
        value_value.operation_state.note
    );

    let x = std::fs::read_to_string(directory.join("x.txt")).unwrap();

    assert!(x.contains("\nthree\n4\n5\n6\n"), "{x}");
    assert_eq!(value_value.stash_state.stash.len(), 1);
    assert_eq!(
        (
            value_value.stash_state.stash[0].path.as_str(),
            value_value.stash_state.stash[0].lines.as_str(),
            value_value.stash_state.stash[0].asked
        ),
        ("x.txt", "5 · removed 5", true)
    );

    let ask = value_value.take_send().unwrap();

    assert!(
        ask.contains("`stash_name`") && ask.contains(&value_value.stash_state.stash[0].id) && ask.contains("+five"),
        "{ask}"
    );

    // the patch puts it back
    let path_value = directory.join("p.diff");

    std::fs::write(&path_value, &value_value.stash_state.stash[0].patch).unwrap();
    run_shell(&directory, "git apply p.diff && rm p.diff");

    assert!(
        std::fs::read_to_string(directory.join("x.txt"))
            .unwrap()
            .contains("\nfive\n")
    );

    // restore: the message to the agent, the entry marked
    let eid = value_value.stash_state.stash[0].id.clone();
    let _ = value_value.restore(&eid);
    let msg = value_value.take_send().unwrap();

    assert!(
        msg.contains("`stash_restored`") && msg.contains("ask the owner"),
        "{msg}"
    );
    assert!(value_value.stash_state.stash[0].restoring);

    // the agent names it, then reports it restored (the MCP tools)
    stash::set_name(&id, &eid, "five as a word").unwrap();
    value_value.sync_stash(true);

    assert_eq!(value_value.stash_state.stash[0].title(), "five as a word");

    stash::restored(&id, &eid).unwrap();
    value_value.sync_stash(true);

    assert_eq!(
        value_value.stash_state.stash,
        [] as [crate::domain::changes::stash::Entry; 0]
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    let _ = std::fs::remove_dir_all(directory);
}

fn prepare_selected_lines() -> (PathBuf, String, View) {
    let directory = std::env::temp_dir().join(format!("codehead-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    run_shell(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 20 > x.txt && git add -A && git commit -qm init",
    );

    // 1 hunk, 2 changes: 3 → three, 5 → five; a new file of 3 lines
    run_shell(
        &directory,
        "seq 1 20 | sed 's/^3$/three/;s/^5$/five/' > x.txt && printf 'a\\nb\\nc\\n' > n.txt",
    );
    let id = format!("changes-lines-{}", std::process::id());
    let mut value = real_view(&directory, &id);

    select(&mut value, "+ three");
    select(&mut value, "- 3");
    select(&mut value, "+ b");
    press(&mut value, KeyCode::Char('s'));

    assert!(
        wait_op(&mut value).contains("staged the selected lines"),
        "{:?}",
        value.operation_state.note
    );

    let cached = git(&directory, &["diff", "--cached", "-U0"]).unwrap();

    assert!(
        cached.contains("-3\n+three") && !cached.contains("five") && cached.contains("+b") && !cached.contains("+a"),
        "{cached}"
    );
    assert!(value.selection_state.sel.is_empty(), "the selection clears");

    // p: `five` out of the worktree, into the stash; its agent (0 known: the lead) asked for a name
    let mut value_value = real_view(&directory, &id);

    select(&mut value_value, "+ five");
    select(&mut value_value, "- 5");
    press(&mut value_value, KeyCode::Char('p'));

    (directory, id, value_value)
}

#[test]
fn stashed_lines_go_back_to_their_spot_in_any_order_after_a_shift() {
    for order in [[0usize, 1], [1, 0]] {
        check_restore_order(order);
    }
}

fn check_restore_order(order: [usize; 2]) {
    let (directory, want) = prepare_restore_lines(order);

    let id = format!("changes-restore-{}-{}", std::process::id(), order[0]);
    let mut value = real_view(&directory, &id);

    select(&mut value, "+ ten");
    select(&mut value, "- 10");
    press(&mut value, KeyCode::Char('s'));
    wait_op(&mut value);

    // stash 1: B (C stays) and ten (staged: out of the index too); stash 2: the removed 25
    let mut value_value = real_view(&directory, &id);

    select(&mut value_value, "+ B");
    select(&mut value_value, "+ ten");
    select(&mut value_value, "- 10");
    press(&mut value_value, KeyCode::Char('p'));

    assert!(
        wait_op(&mut value_value).contains("stashed"),
        "{:?}",
        value_value.operation_state.note
    );

    let mut value_2 = real_view(&directory, &id);

    select(&mut value_2, "- 25");
    press(&mut value_2, KeyCode::Char('p'));

    assert!(
        wait_op(&mut value_2).contains("stashed"),
        "{:?}",
        value_2.operation_state.note
    );

    let x = std::fs::read_to_string(directory.join("x.txt")).unwrap();

    assert!(
        x.contains("20\nC\n21") && x.contains("\n10\n") && x.contains("\n25\n") && x.contains("5\nA\n6"),
        "{x}"
    );
    assert!(
        !git(&directory, &["diff", "--cached"]).unwrap().contains("ten"),
        "ten out of the index"
    );

    let stash = stash::read(&id);

    assert_eq!(stash.len(), 2);

    // the agent's edits since: 3 lines on top, a line between the spots
    run_shell(
        &directory,
        "{ printf 'h1\\nh2\\nh3\\n'; sed 's/^15$/15\\nmid/' x.txt; } > y && mv y x.txt",
    );

    for i in order {
        std::fs::write(directory.join("p.diff"), &stash[i].patch).unwrap();

        // the restore's steps: `git apply`, else with 1 line of context (the context of an
        // entry can hold the spot of an other entry)
        run_shell(&directory, "{ git apply p.diff || git apply -C1 p.diff; } && rm p.diff");
    }

    let got = std::fs::read_to_string(directory.join("x.txt")).unwrap();
    let want = format!("h1\nh2\nh3\n{}", want.replace("\n15\n", "\n15\nmid\n"));

    assert_eq!(got, want);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    let _ = std::fs::remove_dir_all(directory);
}

fn prepare_restore_lines(order: [usize; 2]) -> (PathBuf, String) {
    let directory = std::env::temp_dir().join(format!("codehead-restore-{}-{}", std::process::id(), order[0]));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    run_shell(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 30 > x.txt && git add -A && git commit -qm init",
    );

    // A after 5, 10 → ten, B C after 20, 25 removed; ten staged
    run_shell(
        &directory,
        "seq 1 30 | sed 's/^5$/5\\nA/;s/^10$/ten/;s/^20$/20\\nB\\nC/;/^25$/d' > x.txt",
    );
    let want = std::fs::read_to_string(directory.join("x.txt")).unwrap();

    assert!(
        want.contains("5\nA\n6") && want.contains("20\nB\nC\n21") && !want.contains("\n25\n"),
        "{want}"
    );

    (directory, want)
}

#[test]
fn git_steps_stage_unstage_commit_and_see_a_stopped_rebase() {
    let directory = std::env::temp_dir().join(format!("codehead-gitops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    run_shell(
        &directory,
        "git init -q -b main && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init",
    );
    run_shell(&directory, "printf 'a\\nB\\n' > x.rs && printf 'n\\n' > y.rs");
    run_op(&directory, &Op::Stage("x.rs".into())).unwrap();
    run_op(&directory, &Op::Stage("y.rs".into())).unwrap();
    let character_value = load(&directory).unwrap();

    assert_eq!(
        (character_value.staged.get("x.rs"), character_value.staged.get("y.rs")),
        (Some(&true), Some(&true))
    );

    // a part staged: another edit after the stage
    run_shell(&directory, "printf 'a\\nB\\nc\\n' > x.rs");

    assert_eq!(load(&directory).unwrap().staged.get("x.rs"), Some(&false));

    run_op(&directory, &Op::Unstage("y.rs".into())).unwrap();

    assert!(!load(&directory).unwrap().staged.contains_key("y.rs"));
    assert_eq!(
        run_op(&directory, &Op::Commit("fix: b is B\n\n- body".into())).unwrap(),
        "committed"
    );

    let (_, log) = (0, git(&directory, &["log", "-1", "--format=%B"]).unwrap());

    assert_eq!(log.trim(), "fix: b is B\n\n- body");
    assert!(load(&directory).unwrap().stopped.is_none());

    // a conflict: 2 branches change line 1, rebase stops
    run_shell(
        &directory,
        "git add -A && git commit -qm wip && git checkout -qb other HEAD~1 && printf 'Z\\nB\\n' > x.rs && git commit -qam other && git checkout -q main && printf 'Q\\nB\\nc\\n' > x.rs && git commit -qam main",
    );

    assert!(
        run_op(&directory, &Op::Update("other".into())).is_err(),
        "a conflict: the rebase stops"
    );

    let character_2 = load(&directory).unwrap();

    assert_eq!(character_2.stopped, Some(vec!["x.rs".to_string()]));

    run_op(&directory, &Op::Abort).unwrap();

    assert!(load(&directory).unwrap().stopped.is_none());

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn load_reads_a_real_repo_with_untracked_files() {
    let directory = std::env::temp_dir().join(format!("codehead-changes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    run_shell(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && printf 'a\\nb\\n' > x.rs && git add -A && git commit -qm init",
    );
    run_shell(&directory, "printf 'a\\nB\\n' > x.rs && printf 'new\\n' > y.md");
    let character_value = load(&directory).unwrap();

    assert_eq!(character_value.base, "HEAD");

    let paths: Vec<&str> = character_value.files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(paths, ["x.rs", "y.md"]);
    assert_eq!(character_value.files[1].status, Status::New);

    let _ = std::fs::remove_dir_all(directory);
}

fn run_shell(directory: &Path, script: &str) {
    assert!(
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{script}"
    );
}
