use super::*;
use ratatui::backend::TestBackend;

/// A thread in repo `alpha` with a checkout in repo `beta` (2 repo tabs); ONE: `alpha` only.
fn two_repos(name: &str, one: bool) -> (View, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("codehead-reporow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (alpha, beta) = (root.join("alpha"), root.join("beta"));

    for repo in [&alpha, &beta] {
        std::fs::create_dir_all(repo).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(repo)
                .status()
                .unwrap()
                .success()
        );
    }

    let mut thread = store::Thread::new(name, "task", &alpha, vec!["ada".into()]);

    if !one {
        thread.checkouts.push(store::Checkout {
            dir: beta.display().to_string(),
            branch: None,
        });
    }

    thread.checkouts_pruned = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &alpha, name, tx);

    view.set_pane(Pane::Changes);
    view.panes.prompt_focus = false;

    (view, root, alpha, beta)
}

fn key(view: &mut View, code: KeyCode, modifiers: KeyModifiers) {
    view.key(KeyEvent::new(code, modifiers));
}

fn screen(view: &mut View) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn done(view: &View, root: &Path) {
    let _ = store::delete(&view.id);
    let _ = std::fs::remove_dir_all(root);
}

/// The repo tabs: a region of the focus ring (tab), ←→ there; shift+←→ and ctrl+w: gone.
#[test]
fn the_repo_tabs_are_a_tab_stop() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut view, root, alpha, beta) = two_repos("old-keys", false);

    assert_eq!(view.checkout_state.checkouts, vec![alpha.clone(), beta.clone()]);

    // ctrl+w: 0 repo switch
    key(&mut view, KeyCode::Char('w'), KeyModifiers::CONTROL);

    assert_eq!(view.panes.changes.dir(), alpha.as_path());

    // tab tab: the panel, the repo tabs; → the next repo; shift+tab shift+tab: the review
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert!(view.checkout_state.repo_focus);

    key(&mut view, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), beta.as_path());

    key(&mut view, KeyCode::BackTab, KeyModifiers::SHIFT);
    key(&mut view, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(!view.checkout_state.repo_focus && !view.panes.changes.panel_focus() && !view.panes.prompt_focus);

    // ← in the diff: the panel, 0 repo switch
    key(&mut view, KeyCode::Left, KeyModifiers::NONE);

    assert!(view.panes.changes.panel_focus());
    assert_eq!(view.panes.changes.dir(), beta.as_path());

    done(&view, &root);
}

/// Tab I's background in a fresh draw.
fn tab_bg(view: &mut View, i: usize) -> ratatui::style::Color {
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let tab = view.checkout_state.checkout_tabs.rect_of(i).unwrap();

    term.backend().buffer()[(tab.x + 1, tab.y)].bg
}

#[test]
fn up_from_the_diffs_top_gives_the_repo_tabs_the_arrow_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut view, root, alpha, beta) = two_repos("up-keys", false);
    let text = screen(&mut view);

    assert!(text.contains("alpha") && text.contains("beta"), "{text}");
    assert_eq!(
        tab_bg(&mut view, 0),
        theme::panel().bg.unwrap(),
        "the diff has the keys: 0 accent pill"
    );

    // ↑ at the top: the repo tabs, the accent pill, the footer names the keys
    key(&mut view, KeyCode::Up, KeyModifiers::NONE);

    assert!(view.checkout_state.repo_focus);
    assert_eq!(tab_bg(&mut view, 0), theme::pill().bg.unwrap());
    assert!(screen(&mut view).contains("←→ repo"));

    // →, l: the next repo (wrapping); ←, h: back; the tabs keep the keys
    key(&mut view, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), beta.as_path());
    assert!(view.checkout_state.repo_focus && !view.panes.changes.panel_focus());

    key(&mut view, KeyCode::Char('l'), KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), alpha.as_path());

    key(&mut view, KeyCode::Left, KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), beta.as_path());

    key(&mut view, KeyCode::Char('h'), KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), alpha.as_path());

    // a letter: 0 effect, 0 leak into the diff
    key(&mut view, KeyCode::Char('w'), KeyModifiers::NONE);

    assert!(view.checkout_state.repo_focus && !view.panes.changes.panel_focus());

    // ↓, enter, esc: the diff again
    for code in [KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
        key(&mut view, KeyCode::Up, KeyModifiers::NONE);

        assert!(view.checkout_state.repo_focus);

        key(&mut view, code, KeyModifiers::NONE);

        assert!(!view.checkout_state.repo_focus, "{code:?}");
        assert_eq!(view.panes.pane, Pane::Changes);
    }

    // tab: the prompt; tab again: the view, the tabs still have the keys
    key(&mut view, KeyCode::Up, KeyModifiers::NONE);
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert!(view.panes.prompt_focus);
    assert_eq!(
        tab_bg(&mut view, 0),
        theme::panel().bg.unwrap(),
        "the prompt has the keys"
    );

    // another view: the focus resets
    key(&mut view, KeyCode::F(1), KeyModifiers::NONE);
    key(&mut view, KeyCode::F(2), KeyModifiers::NONE);

    assert!(!view.checkout_state.repo_focus);

    done(&view, &root);
}

#[test]
fn up_keeps_the_diff_with_1_repo() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut view, root, ..) = two_repos("up-one", true);
    let _ = screen(&mut view);

    key(&mut view, KeyCode::Up, KeyModifiers::NONE);

    assert!(!view.checkout_state.repo_focus, "1 repo: 0 tabs to step");

    done(&view, &root);
}

/// The panel: ↑ on its 1st row, the commit message; ↑ there, the repo tabs; ↓, the panel.
#[test]
fn up_from_the_panels_top_gives_the_repo_tabs_the_arrow_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut view, root, alpha, beta) = two_repos("up-panel", false);
    let _ = screen(&mut view);

    key(&mut view, KeyCode::Left, KeyModifiers::NONE);

    assert!(view.panes.changes.panel_focus());

    key(&mut view, KeyCode::Up, KeyModifiers::NONE);

    assert!(
        !view.checkout_state.repo_focus,
        "the panel's 1st row: the commit message 1st"
    );

    key(&mut view, KeyCode::Up, KeyModifiers::NONE);

    assert!(view.checkout_state.repo_focus);
    assert_eq!(tab_bg(&mut view, 0), theme::pill().bg.unwrap());

    key(&mut view, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), beta.as_path());

    key(&mut view, KeyCode::Left, KeyModifiers::NONE);

    assert_eq!(view.panes.changes.dir(), alpha.as_path());

    // ↓: the panel again, 0 diff
    key(&mut view, KeyCode::Down, KeyModifiers::NONE);

    assert!(!view.checkout_state.repo_focus && view.panes.changes.panel_focus());

    done(&view, &root);
}

#[test]
fn a_click_on_a_repo_tab_picks_it_and_gives_the_tabs_the_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut view, root, _, beta) = two_repos("click", false);

    view.panes.prompt_focus = true;
    let _ = screen(&mut view);
    let tab = view.checkout_state.checkout_tabs.rect_of(1).unwrap();

    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: tab.x + 1,
        row: tab.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(view.panes.changes.dir(), beta.as_path());
    assert!(view.checkout_state.repo_focus && !view.panes.prompt_focus);

    // a click in the diff: the diff has the keys
    let _ = screen(&mut view);
    let diff = view.layout.view_rect;

    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: diff.x + 2,
        row: diff.y + 1,
        modifiers: KeyModifiers::NONE,
    });

    assert!(!view.checkout_state.repo_focus);

    done(&view, &root);
}
