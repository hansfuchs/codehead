use super::*;
use ratatui::backend::TestBackend;

fn screen(view: &mut View) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(90, 40)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

/// TEXT typed into the prompt.
fn typed_text(view: &mut View, text: &str) {
    for character in text.chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
}

#[test]
fn changes_follow_the_repo_when_the_work_dir_has_0_git() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::env::temp_dir().join(format!("codehead-nogit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (plain, repo) = (root.join("plain"), root.join("plain/app"));

    std::fs::create_dir_all(&repo).unwrap();

    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    // codehead started in a dir without git; an agent edited in the repo below it
    let mut thread = store::Thread::new("no git", "task", &plain, vec!["ada".into()]);

    thread.checkouts.push(store::Checkout {
        dir: repo.display().to_string(),
        branch: None,
    });
    thread.checkouts_pruned = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &plain, "no git", tx);

    view.load_checkouts();

    assert_eq!(view.checkout_state.checkouts, std::slice::from_ref(&repo));
    assert_eq!(view.panes.changes.dir(), repo.as_path());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn changes_follow_the_worktree_an_agent_moved_the_work_to_until_the_owner_picks() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::env::temp_dir().join(format!("codehead-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (repo, wt) = (root.join("app"), root.join("app-feat"));

    std::fs::create_dir_all(&repo).unwrap();

    let sh = |script: &str| {
        assert!(
            std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success(),
            "{script}"
        );
    };

    sh(
        "git init -q && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m init && git worktree add -q -b feat ../app-feat",
    );

    // the thread's work dir: the main checkout; its agent moved to the worktree
    let mut thread = store::Thread::new("moved", "task", &repo, vec!["ada".into()]);

    thread.checkouts.push(store::Checkout {
        dir: wt.display().to_string(),
        branch: Some("feat".into()),
    });
    thread.checkouts_pruned = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &repo, "moved", tx);

    view.load_checkouts();

    assert_eq!(view.checkout_state.checkouts, vec![repo.clone(), wt.clone()]);
    assert_eq!(
        view.panes.changes.dir(),
        wt.as_path(),
        "the worktree, not the main checkout"
    );

    // the owner picks the main checkout: it stays over a reload
    view.pick_checkout(0);
    view.load_checkouts();

    assert_eq!(view.panes.changes.dir(), repo.as_path());

    // a thread in its own worktree, an agent edit in the main checkout: the worktree stays
    let mut thread = store::Thread::new("own worktree", "task", &repo, vec!["ada".into()]);

    thread.worktree = Some(wt.display().to_string());
    thread.checkouts.push(store::Checkout {
        dir: repo.display().to_string(),
        branch: None,
    });
    thread.checkouts_pruned = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &wt, "own worktree", tx);

    view.load_checkouts();

    assert_eq!(view.panes.changes.dir(), wt.as_path(), "the thread's own worktree");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_changes_panel_shows_a_commit_read_only_and_esc_brings_the_changes_back() {
    use crate::domain::changes::commits::fixture;
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = fixture::repo("thread-view");

    fixture::sh(&repo, "echo draft > x.txt");
    let thread = store::Thread::new("commits", "task", &repo, vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &repo, "commits", tx);

    let key = |view: &mut View, code: KeyCode, modifiers: KeyModifiers| {
        view.key(KeyEvent::new(code, modifiers));
    };

    let wait = wait_for_screen;

    // a draft in the prompt, then the changes view, the panel with the keys
    typed_text(&mut view, "draft");
    key(&mut view, KeyCode::Char('2'), KeyModifiers::ALT);

    // (a new read dissolves in over a few frames: each label waited for)
    wait(&mut view, "UNPUSHED");
    wait(&mut view, "COMMITS  4");
    wait(&mut view, "PUSHED    2");

    // tab: the review → the panel
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert!(view.panes.changes.panel_focus());

    // G: the last row (the oldest pushed commit); enter: its files, read only
    key(&mut view, KeyCode::Char('G'), KeyModifiers::NONE);
    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);
    let all = wait(&mut view, "c1 subject · 1 files");

    assert!(all.contains("c1.txt") && all.contains("◆"), "{all}");

    key(&mut view, KeyCode::Char('c'), KeyModifiers::NONE);

    assert!(!screen(&mut view).join("\n").contains("comment on"), "0 comment modal");

    // esc: the changes back (x.txt), the draft still in the prompt
    key(&mut view, KeyCode::Esc, KeyModifiers::NONE);
    let all = screen(&mut view).join("\n");

    assert!(!all.contains("c1 subject · ") && all.contains("x.txt"), "{all}");
    assert_eq!(view.prompt.input.text(), "draft");

    fixture::drop(&repo);
}

fn wait_for_screen(view: &mut View, pat: &str) -> String {
    for _ in 0..200 {
        let text = screen(view).join("\n");

        if text.contains(pat) {
            return text;
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    panic!("0 {pat}: {}", screen(view).join("\n"));
}
