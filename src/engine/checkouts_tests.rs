use super::*;
use serde_json::json;

#[test]
fn edits_and_work_dirs_count_reads_do_not() {
    assert_eq!(
        edited("Edit", &json!({"file_path": "/r/a.rs"})),
        vec![PathBuf::from("/r/a.rs")]
    );
    assert_eq!(
        edited("Read", &json!({"file_path": "/r/a.rs"})),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        edited("Bash", &json!({"command": "cd rel/dir && git commit -m x"})),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        edited("fs", &json!({"_kind": "edit", "locations": [{"path": "/x/y"}]})),
        vec![PathBuf::from("/x/y")]
    );
}

fn bash(cmd: &str) -> Vec<PathBuf> {
    edited("Bash", &json!({ "command": cmd }))
}

fn paths(value: &[&str]) -> Vec<PathBuf> {
    value.iter().map(PathBuf::from).collect()
}

#[test]
fn shell_reads_add_0_dirs() {
    assert_eq!(
        bash("cd /other/repo && git status; git -C /third log"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("cd /r/notes && ls; echo ---; cat INDEX.md; grep -i deploy x.md"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("git -C ~/.cache/x/platform status -sb | head -2 2>/dev/null"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("cd /r && cargo test 2>&1 | tail -5 >/dev/null"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("cd /r && git -c a=b fetch --quiet origin main"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("cd /r && echo 'a -> b' && perl -Mstrict -ne 'print' x"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(
        bash("cd /r && git worktree add -b feat $W/r/task origin/main"),
        [] as [std::path::PathBuf; 0]
    );
}

#[test]
fn shell_writes_add_their_dirs() {
    assert_eq!(
        bash("cd /other/repo && git status; git commit -m x"),
        paths(&["/other/repo"])
    );
    assert_eq!(bash("git -C /third commit -am x"), paths(&["/third"]));
    assert_eq!(bash("cd /a && cd sub && sed -i '' s/x/y/ f.rs"), paths(&["/a/sub"]));
    assert_eq!(bash("cd /a; cd /b && git rm -rq lib"), paths(&["/b"]));
    assert_eq!(
        bash("echo x >> /n/notes/INDEX.md && tail -1 /n/notes/INDEX.md"),
        paths(&["/n/notes/INDEX.md"])
    );
    assert_eq!(bash("cd /a && echo x > out.txt"), paths(&["/a"]));
    assert_eq!(bash("cp /src/a.rs /dst/a.rs"), paths(&["/dst/a.rs"]));
    assert_eq!(bash("cd /a && perl -pi -e 's/x/y/' f"), paths(&["/a"]));
    assert_eq!(bash("cd /a && FOO=1 git push origin x"), paths(&["/a"]));
}

#[test]
fn checkouts_without_an_edit_go() {
    let co = |directory: &str| store::Checkout {
        dir: directory.into(),
        branch: None,
    };
    let thread = store::Thread {
        dir: "/w".into(),
        checkouts: vec![co("/notes"), co("/read"), co("/cache/clone"), co("/wt")],
        ..Default::default()
    };

    let tool = |name: &str, input: Value| {
        store::Event::new(
            Some("A"),
            store::Kind::Tool {
                id: "1".into(),
                name: name.into(),
                input,
            },
        )
    };

    let events = [
        tool(
            "Bash",
            json!({"command": "cd /read && grep -rn x .; git -C /cache/clone status"}),
        ),
        tool("Write", json!({"file_path": "/notes/plans/p.md"})),
        tool("Bash", json!({"command": "cd /wt && git commit -m x"})),
        tool("Read", json!({"file_path": "/read/a.rs"})),
    ];

    assert_eq!(
        unedited(&thread, &events),
        vec!["/read".to_string(), "/cache/clone".to_string()]
    );
}

#[test]
fn heredoc_bodies_are_text() {
    assert_eq!(
        bash("cd /a && cat <<'EOF'\nrm -rf /b\ngit commit\nEOF\ngit log"),
        [] as [std::path::PathBuf; 0]
    );
    assert_eq!(bash("cd /a && cat > f.md <<EOF\nrm x\nEOF"), paths(&["/a"]));
}

#[test]
fn checkouts_join_once() {
    let mut thread = store::Thread {
        dir: "/a".into(),
        branch: Some("main".into()),
        ..Default::default()
    };

    assert!(thread.add_checkout(store::Checkout {
        dir: "/b".into(),
        branch: Some("feat".into())
    }));
    assert!(!thread.add_checkout(store::Checkout {
        dir: "/b".into(),
        branch: None
    }));
    assert!(!thread.add_checkout(store::Checkout {
        dir: "/a".into(),
        branch: None
    }));

    let all = thread.all_checkouts();

    assert_eq!(
        all.iter().map(|character| character.dir.as_str()).collect::<Vec<_>>(),
        vec!["/a", "/b"]
    );
}

#[test]
fn a_worktree_inside_a_checkout_counts_as_its_own() {
    let root = std::env::temp_dir().join(format!("codehead-nested-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let wt = root.join(".claude/worktrees/x");

    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: ../../../.git/worktrees/x").unwrap();

    assert!(nested(&wt.join("src/a.rs"), &root), "under the nested worktree");
    assert!(!nested(&root.join("src/a.rs"), &root), "the checkout's own file");

    // a submodule: the checkout's own
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("lib/.git"), "gitdir: ../.git/modules/lib").unwrap();

    assert!(!nested(&root.join("lib/x.c"), &root), "a submodule");

    let _ = std::fs::remove_dir_all(&root);
}
