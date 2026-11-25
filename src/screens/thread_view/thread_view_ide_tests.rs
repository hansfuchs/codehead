use super::*;
use ratatui::backend::TestBackend;

fn draw(view: &mut View) -> String {
    let mut thread = Terminal::new(TestBackend::new(120, 40)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..40)
        .map(|y| (0..120).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_ide_command_opens_the_repo_root_and_several_repos_get_a_picker() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::env::temp_dir().join(format!("codehead-ide-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (agent, bytes) = (root.join("alpha"), root.join("beta"));

    for directory in [&agent, &bytes] {
        std::fs::create_dir_all(directory.join("src")).unwrap();

        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(directory)
                .status()
                .unwrap()
                .success()
        );
    }

    // 1 repo, the work dir below its root: the IDE on the root, from the chat and the changes view
    let thread = store::Thread::new("ide", "task", &agent.join("src"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &agent.join("src"), "ide", tx);

    view.prompt.input.set_text("draft");
    view.menu_action(crate::screens::quick_menu::Action::Command(2));
    view.set_pane(Pane::Changes);
    view.panes.prompt_focus = false;
    view.menu_action(crate::screens::quick_menu::Action::Command(2));

    assert_eq!(view.ide.ide_opened, [agent.clone(), agent.clone()]);
    assert!(view.ide.ide_pick.is_none() && view.prompt.input.text() == "draft");

    // 2 repos: the picker above the repos line; ↓ enter: the 2nd
    let thread = store::load(&thread.id).unwrap();

    assert_ide_picker(&(root), &(agent), &(bytes), view, thread);
}

fn assert_ide_picker(root: &PathBuf, agent: &PathBuf, bytes: &PathBuf, mut view: View, mut thread: store::Thread) {
    thread.checkouts.push(store::Checkout {
        dir: bytes.display().to_string(),
        branch: None,
    });
    thread.checkouts_pruned = true;
    thread.save().unwrap();
    view.set_pane(Pane::Chat);
    view.menu_action(crate::screens::quick_menu::Action::Command(2));

    assert_eq!(view.ide.ide_pick, Some((0, 0)));

    let text = draw(&mut view);

    assert!(
        text.contains("open in IDE") && text.contains("alpha") && text.contains("beta"),
        "{text}"
    );

    let pick = view.ide.ide_rows.iter().map(|(result, _)| result.y).max().unwrap();

    assert!(pick < view.layout.hint_rect.y, "above the repos line");

    view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(view.ide.ide_opened.last(), Some(bytes));
    assert!(view.ide.ide_pick.is_none());

    // a click on a row opens it; esc closes the picker (0 interrupt)
    view.menu_action(crate::screens::quick_menu::Action::Command(2));
    let _ = draw(&mut view);
    let (row, i) = view.ide.ide_rows[0];

    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!((view.ide.ide_opened.last(), i), (Some(agent), 0));

    view.menu_action(crate::screens::quick_menu::Action::Command(2));
    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(view.ide.ide_pick.is_none() && view.last_esc.is_none());

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(root);
}
