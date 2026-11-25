use super::*;
use ratatui::backend::TestBackend;

/// A thread in repo `alpha` with a checkout in repo `beta` (2 repo tabs); ONE: `alpha` only.
fn two_repos(name: &str, one: bool) -> (View, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("codehead-reporow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (agent, bytes) = (root.join("alpha"), root.join("beta"));

    for directory in [&agent, &bytes] {
        std::fs::create_dir_all(directory).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(directory)
                .status()
                .unwrap()
                .success()
        );
    }

    let mut thread = store::Thread::new(name, "task", &agent, vec!["ada".into()]);

    if !one {
        thread.checkouts.push(store::Checkout {
            dir: bytes.display().to_string(),
            branch: None,
        });
    }

    thread.checkouts_pruned = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &agent, name, tx);

    view.set_pane(Pane::Changes);
    view.panes.prompt_focus = false;

    (view, root, agent, bytes)
}

fn key(view: &mut View, character: KeyCode, message: KeyModifiers) {
    view.key(KeyEvent::new(character, message));
}

fn screen(view: &mut View) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();

    term.draw(|file| view.draw(file)).unwrap();
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
    let (mut value, root, agent, bytes) = two_repos("old-keys", false);

    assert_eq!(value.checkout_state.checkouts, vec![agent.clone(), bytes.clone()]);

    // ctrl+w: 0 repo switch
    key(&mut value, KeyCode::Char('w'), KeyModifiers::CONTROL);

    assert_eq!(value.panes.changes.dir(), agent.as_path());

    // tab tab: the panel, the repo tabs; → the next repo; shift+tab shift+tab: the review
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.checkout_state.repo_focus);

    key(&mut value, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), bytes.as_path());

    key(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);
    key(&mut value, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(!value.checkout_state.repo_focus && !value.panes.changes.panel_focus() && !value.panes.prompt_focus);

    // ← in the diff: the panel, 0 repo switch
    key(&mut value, KeyCode::Left, KeyModifiers::NONE);

    assert!(value.panes.changes.panel_focus());
    assert_eq!(value.panes.changes.dir(), bytes.as_path());

    done(&value, &root);
}

/// Tab I's background in a fresh draw.
fn tab_bg(view: &mut View, i: usize) -> ratatui::style::Color {
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();

    term.draw(|file| view.draw(file)).unwrap();
    let result = view.checkout_state.checkout_tabs.rect_of(i).unwrap();

    term.backend().buffer()[(result.x + 1, result.y)].bg
}

#[test]
fn up_from_the_diffs_top_gives_the_repo_tabs_the_arrow_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut value, root, agent, bytes) = two_repos("up-keys", false);
    let text = screen(&mut value);

    assert!(text.contains("alpha") && text.contains("beta"), "{text}");
    assert_eq!(
        tab_bg(&mut value, 0),
        theme::panel().bg.unwrap(),
        "the diff has the keys: 0 accent pill"
    );

    // ↑ at the top: the repo tabs, the accent pill, the footer names the keys
    key(&mut value, KeyCode::Up, KeyModifiers::NONE);

    assert!(value.checkout_state.repo_focus);
    assert_eq!(tab_bg(&mut value, 0), theme::pill().bg.unwrap());
    assert!(screen(&mut value).contains("←→ repo"));

    // →, l: the next repo (wrapping); ←, h: back; the tabs keep the keys
    key(&mut value, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), bytes.as_path());
    assert!(value.checkout_state.repo_focus && !value.panes.changes.panel_focus());

    key(&mut value, KeyCode::Char('l'), KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), agent.as_path());

    key(&mut value, KeyCode::Left, KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), bytes.as_path());

    key(&mut value, KeyCode::Char('h'), KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), agent.as_path());

    // a letter: 0 effect, 0 leak into the diff
    key(&mut value, KeyCode::Char('w'), KeyModifiers::NONE);

    assert!(value.checkout_state.repo_focus && !value.panes.changes.panel_focus());

    // ↓, enter, esc: the diff again
    for character in [KeyCode::Down, KeyCode::Enter, KeyCode::Esc] {
        key(&mut value, KeyCode::Up, KeyModifiers::NONE);

        assert!(value.checkout_state.repo_focus);

        key(&mut value, character, KeyModifiers::NONE);

        assert!(!value.checkout_state.repo_focus, "{character:?}");
        assert_eq!(value.panes.pane, Pane::Changes);
    }

    // tab: the prompt; tab again: the view, the tabs still have the keys
    key(&mut value, KeyCode::Up, KeyModifiers::NONE);
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.panes.prompt_focus);
    assert_eq!(
        tab_bg(&mut value, 0),
        theme::panel().bg.unwrap(),
        "the prompt has the keys"
    );

    // another view: the focus resets
    key(&mut value, KeyCode::F(1), KeyModifiers::NONE);
    key(&mut value, KeyCode::F(2), KeyModifiers::NONE);

    assert!(!value.checkout_state.repo_focus);

    done(&value, &root);
}

#[test]
fn up_keeps_the_diff_with_1_repo() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut value, root, ..) = two_repos("up-one", true);
    let _ = screen(&mut value);

    key(&mut value, KeyCode::Up, KeyModifiers::NONE);

    assert!(!value.checkout_state.repo_focus, "1 repo: 0 tabs to step");

    done(&value, &root);
}

/// The panel: ↑ on its 1st row, the commit message; ↑ there, the repo tabs; ↓, the panel.
#[test]
fn up_from_the_panels_top_gives_the_repo_tabs_the_arrow_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut value, root, agent, bytes) = two_repos("up-panel", false);
    let _ = screen(&mut value);

    key(&mut value, KeyCode::Left, KeyModifiers::NONE);

    assert!(value.panes.changes.panel_focus());

    key(&mut value, KeyCode::Up, KeyModifiers::NONE);

    assert!(
        !value.checkout_state.repo_focus,
        "the panel's 1st row: the commit message 1st"
    );

    key(&mut value, KeyCode::Up, KeyModifiers::NONE);

    assert!(value.checkout_state.repo_focus);
    assert_eq!(tab_bg(&mut value, 0), theme::pill().bg.unwrap());

    key(&mut value, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), bytes.as_path());

    key(&mut value, KeyCode::Left, KeyModifiers::NONE);

    assert_eq!(value.panes.changes.dir(), agent.as_path());

    // ↓: the panel again, 0 diff
    key(&mut value, KeyCode::Down, KeyModifiers::NONE);

    assert!(!value.checkout_state.repo_focus && value.panes.changes.panel_focus());

    done(&value, &root);
}

#[test]
fn a_click_on_a_repo_tab_picks_it_and_gives_the_tabs_the_keys() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut value, root, _, bytes) = two_repos("click", false);

    value.panes.prompt_focus = true;
    let _ = screen(&mut value);
    let result = value.checkout_state.checkout_tabs.rect_of(1).unwrap();

    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: result.x + 1,
        row: result.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(value.panes.changes.dir(), bytes.as_path());
    assert!(value.checkout_state.repo_focus && !value.panes.prompt_focus);

    // a click in the diff: the diff has the keys
    let _ = screen(&mut value);
    let directory = value.layout.view_rect;

    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: directory.x + 2,
        row: directory.y + 1,
        modifiers: KeyModifiers::NONE,
    });

    assert!(!value.checkout_state.repo_focus);

    done(&value, &root);
}
