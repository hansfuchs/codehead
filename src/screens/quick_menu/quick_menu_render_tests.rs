use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn agent(name: &str, harness: &str) -> Agent {
    Agent {
        name: name.into(),
        harness: harness.into(),
        model: None,
        color: None,
        mode: String::new(),
        prompt: None,
        last_seen: None,
    }
}

fn screen(message: &mut Menu) -> String {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 34)).unwrap();

    thread.draw(|file| message.render(file, file.area())).unwrap();
    let bytes = thread.backend().buffer().clone();
    let text = (0..34)
        .map(|y| (0..90).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    if std::env::var_os("SHOW").is_some() {
        eprintln!("{text}");
    }

    text
}

/// 0 agents tab (the thread view's agents box, tab, has their actions); n starts a thread.
#[test]
fn the_menu_has_0_agents_tab_and_n_starts_a_thread() {
    let roster = vec![agent("amon", "claude"), agent("nova", "codex")];
    let mut message = Menu::new(
        Vec::new(),
        Some("t1".into()),
        roster,
        vec![Card {
            id: "t1".into(),
            ..Default::default()
        }],
    );
    let text = screen(&mut message);
    let bar = text.lines().find(|line| line.contains("tokens")).unwrap_or_default();

    assert!(bar.contains("quick") && !bar.contains("agents"), "{text}");
    assert_eq!(
        message.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        Action::Home(Home::New)
    );
}
