use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use ratatui::crossterm::event::KeyEventState;

pub(super) fn test_app() -> App {
    app_with(None)
}

/// DRAFTS: the drafts dir of another test app (its drafts show here). None: a new, empty dir.
pub(crate) fn app_with(drafts: Option<&std::path::Path>) -> App {
    let dir = std::env::temp_dir().join(format!("ch-render-{}", std::process::id()));

    std::fs::create_dir_all(dir.join("infra")).unwrap();
    std::fs::write(dir.join("infra/deploy.md"), "# Deploy\n\nuse `kubectl rollout`\n").unwrap();
    std::fs::write(dir.join("ideas.md"), "# Ideas\n\nfish shell tips\n").unwrap();
    let cfg = Config {
        notes: dir.clone(),
        repo: None,
        cwd: dir.clone(),
        agent: "amon".into(),
        picks: vec![],
        branch: None,
        prefix: "task".into(),
        copy_mode: false,
        logs: dir.join("logs"),
        drafts: drafts.map_or_else(
            || {
                std::env::temp_dir().join(format!(
                    "codehead-drafts-app-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ))
            },
            std::path::Path::to_path_buf,
        ),
    };

    if drafts.is_none() {
        let _ = std::fs::remove_dir_all(&cfg.drafts);
    }

    let mut app = App::new(cfg, "");
    let _ = std::fs::remove_file(&app.settings_path);

    // 1 fixed agent: the machine's agents must not change the tests
    app.agent_list.agents = vec![Agent {
        name: "amon".into(),
        harness: "claude".into(),
        ..Default::default()
    }];
    app.compose.picks = vec!["amon".into()];
    app.agent_list.agent_reads = Reads::default();
    app.thread_list.thread_reads = Reads::default();
    app.thread_list.session_reads = Reads::default();
    app.thread_list.worktrees = Some(Vec::new());
    app.thread_list.sessions.clear();

    app
}

pub(super) fn key(app: &mut App, code: KeyCode, message: KeyModifiers) {
    app.key(KeyEvent {
        code,
        modifiers: message,
        kind: ratatui::crossterm::event::KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

pub(super) fn dump(app: &mut App, width: u16, height: u16) -> String {
    let mut thread = Terminal::new(TestBackend::new(width, height)).unwrap();

    for _ in 0..30 {
        thread.draw(|file| app.draw(file)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.tick();
    }

    thread.draw(|file_value| app.draw(file_value)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| bytes[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        key(app, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

/// Type "hi", undo (ctrl+z / ⌘z), redo (ctrl+shift+z / ⌘⇧z); TEXT reads the field.
fn undo_redo(app: &mut App, text: impl Fn(&App) -> String) {
    let before = text(app);

    for character in "hi".chars() {
        key(app, KeyCode::Char(character), KeyModifiers::NONE);
    }

    let after = text(app);

    assert_eq!(after, format!("{before}hi"));
    key(app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(text(app), before);
    key(app, KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(text(app), after);
    key(app, KeyCode::Char('z'), KeyModifiers::SUPER);
    assert_eq!(text(app), before);
    key(app, KeyCode::Char('z'), KeyModifiers::SUPER | KeyModifiers::SHIFT);
    assert_eq!(text(app), after);
}

pub(super) fn card(id: &str, file: impl FnOnce(&mut crate::ui::thread_card::Card)) -> crate::ui::thread_card::Card {
    let mut character = crate::ui::thread_card::Card {
        id: id.into(),
        title: format!("thread {id}"),
        state: "open".into(),
        ..Default::default()
    };

    file(&mut character);
    character
}

#[path = "start_screen_repository_tests.rs"]
mod repository;

#[path = "start_screen_branch_choices_tests.rs"]
mod branch_choices;

#[path = "start_screen_draft_controls_tests.rs"]
mod draft_controls;

#[path = "start_screen_agent_cards_tests.rs"]
mod agent_cards;

#[path = "start_screen_form_settings_tests.rs"]
mod form_settings;

#[path = "start_screen_navigation_tests.rs"]
mod navigation;

#[path = "start_screen_tab_content_tests.rs"]
mod tab_content;
