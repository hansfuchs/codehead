use super::*;

fn key_event(character: KeyCode) -> KeyEvent {
    KeyEvent::new(character, KeyModifiers::NONE)
}

fn ctrl(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

fn card(id: &str, file: impl FnOnce(&mut Card)) -> Card {
    let mut character = Card {
        id: id.into(),
        title: id.into(),
        state: "open".into(),
        ..Default::default()
    };

    file(&mut character);
    character
}

fn screen(line: &mut ThreadList, width: u16, height: u16) -> String {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    thread.draw(|file| line.draw(file, file.area(), 0)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| bytes[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sections: open (working, a question, code not landed, 0 code yet, 0 state read), stale
/// (open with 0 code, idle 3 days: a question too), shipped; archived threads only in their
/// view.
#[test]
fn sections_and_the_archived_view() {
    use crate::domain::landed::Land;
    let now = crate::engine::now_ms();
    let hour = 3600 * 1000;
    let cards = vec![
        card("idle", |_| {}),
        card("asks", |character| character.agents.blocked = true),
        card("busy", |character_value| character_value.agents.busy = true),
        card("shipped", |character_2| character_2.land = Some(Land::Landed)),
        card("talk", |character_3| {
            character_3.land = Some(Land::NoCode);
            character_3.completion.done = true;
        }),
        card("plan", |character_4| {
            character_4.land = Some(Land::Landed);
            character_4.completion.review = true;
        }),
        card("onmain", |character_5| {
            character_5.land = Some(Land::Main);
            character_5.completion.done = true;
            character_5.activity = now - hour;
        }),
        card("mainidle", |character_6| {
            character_6.land = Some(Land::Main);
            character_6.completion.done = true;
            character_6.activity = now - 4 * 24 * hour;
        }),
        card("new", |character_7| {
            character_7.land = Some(Land::NoCode);
            character_7.activity = now - hour;
        }),
        card("dusty", |character_8| {
            character_8.land = Some(Land::NoCode);
            character_8.activity = now - 5 * 24 * hour - hour;
        }),
        card("loose", |character_9| {
            character_9.land = Some(Land::Unpushed(3));
            character_9.activity = now - 9 * 24 * hour;
        }),
        card("waits", |character_10| {
            character_10.agents.blocked = true;
            character_10.land = Some(Land::NoCode);
            character_10.activity = now - 4 * 24 * hour;
        }),
        card("old", |character_11| character_11.state = "archived".into()),
    ];
    let mut line = ThreadList::new(Mode::Switch, None, None, cards);

    let ids = |line_value: &ThreadList| -> Vec<String> {
        line_value
            .visible()
            .iter()
            .map(|&i| line_value.cards[i].id.clone())
            .collect()
    };

    // status tab open first: the open cards only
    assert_eq!(line.stat_tab(), StatusTab::Open);
    assert_eq!(ids(&line), ["idle", "asks", "busy", "plan", "onmain", "new", "loose"]);

    // 0 code: shipped only when the agents reported done; idle 3 days: stale (code: never);
    // a step to review: open; on main: never shipped (done or not), stale after 3 days
    line.stat_pick = Some(StatusTab::All);

    sections_and_the_archived_view_verify_text(line, ids);
}

fn sections_and_the_archived_view_verify_text(mut line: ThreadList, ids: impl Fn(&ThreadList) -> Vec<String>) {
    assert_eq!(
        ids(&line),
        [
            "idle", "asks", "busy", "plan", "onmain", "new", "loose", "mainidle", "dusty", "waits", "shipped", "talk"
        ]
    );

    let text = screen(&mut line, 80, 120);

    for x in [
        "status     open 7   stale 3   shipped 2   all 12",
        "◇ 3 unpushed",
        "⇲ shipped",
        "⇲ nothing to ship",
        "auto-archive in 25d",
        "▷ step to review",
        "◌ on main · auto-archive in 26d",
    ] {
        assert!(text.contains(x), "{x}: {text}");
    }

    // 0 section labels in the list (the status row holds them)
    for x in [
        "open ·",
        "stale ·",
        "shipped: archive?",
        "needs you ·",
        "recent ·",
        "working ·",
        "threads ·",
    ] {
        assert!(!text.contains(x), "{x}: {text}");
    }

    assert_eq!(text.matches("nothing to ship").count(), 1, "{text}");
    assert!(
        text.contains("active 12") && text.contains("archived 1") && !text.contains("old"),
        "{text}"
    );

    // ↑ from the 1st card: the status tabs; ← stale (its hint on the row); ↑ the view tabs
    verify_sections_and_the_archived_view(line, ids);
}

fn verify_sections_and_the_archived_view(mut line: ThreadList, ids: impl Fn(&ThreadList) -> Vec<String>) {
    line.key(key_event(KeyCode::Up));

    assert_eq!(line.focus, Focus::Status);

    line.key(key_event(KeyCode::Left));
    line.key(key_event(KeyCode::Left));

    assert_eq!(line.stat_tab(), StatusTab::Stale);
    assert_eq!(ids(&line), ["mainidle", "dusty", "waits"]);

    let text_value = screen(&mut line, 100, 40);

    assert!(
        text_value.contains("stale 3   shipped 2   all 12  0 code or on main, idle 3+ days"),
        "{text_value}"
    );

    // the filter narrows the counts too
    line.filter.set_text("dusty");
    let text_2 = screen(&mut line, 100, 40);

    assert!(text_2.contains("open 0   stale 1   shipped 0   all 1"), "{text_2}");

    line.filter.set_text("");
    line.key(key_event(KeyCode::Up));

    assert_eq!(line.focus, Focus::Views);

    line.key(key_event(KeyCode::Right));
    line.key(key_event(KeyCode::Down));

    assert_eq!(
        (line.view, line.focus),
        (View::Archived, Focus::List),
        "archived: 0 status tabs, ↓ to the list"
    );

    let text_3 = screen(&mut line, 70, 40);

    assert!(
        text_3.contains("old")
            && text_3.contains("archived · 1")
            && text_3.contains("▣ archived")
            && !text_3.contains("status "),
        "{text_3}"
    );
}

/// The status row stays while the list scrolls; the thread in view picks its status tab.
#[test]
fn the_status_row_stays_out_of_the_scroll() {
    use crate::domain::landed::Land;
    let mut cards: Vec<Card> = (0..30).map(|i| card(&format!("t{i:02}"), |_| {})).collect();

    cards.push(card("done", |character| character.land = Some(Land::Landed)));
    let mut line = ThreadList::new(Mode::Switch, Some("done".into()), None, cards);

    assert_eq!(line.stat_tab(), StatusTab::Shipped, "the thread in view's section");
    assert_eq!(line.visible().len(), 1);

    line.stat_pick = Some(StatusTab::Open);

    for _ in 0..20 {
        line.key(key_event(KeyCode::Down));
    }

    let text = screen(&mut line, 80, 30);
    let lines: Vec<&str> = text.lines().collect();

    assert!(
        lines[4].contains("status") && lines[4].contains("open 30") && lines[4].contains('↓'),
        "{text}"
    );
    assert!(!text.contains("t00"), "{text}");
}

/// The menu: letters filter; ctrl+o → the actions; a → the question; enter → archived.
#[test]
fn switch_mode_archives_by_ctrl_dot_and_a_after_a_yes() {
    let dir = std::env::temp_dir();
    let thread = crate::engine::store::Thread::new("archive from menu", "task", &dir, vec!["ada".into()]);

    thread.save().unwrap();
    let mut line = ThreadList::new(
        Mode::Switch,
        None,
        None,
        vec![card(&thread.id, |character| {
            character.title = "archive from menu".into();
        })],
    );

    line.key(key_event(KeyCode::Char('a')));

    assert_eq!(line.query(), "a", "a letter filters in the menu");

    line.key(key_event(KeyCode::Backspace));
    line.key(ctrl('.'));
    let text = screen(&mut line, 80, 30);

    assert!(
        text.contains("actions") && text.contains("archive: worktree goes") && text.contains("delete"),
        "{text}"
    );

    line.key(key_event(KeyCode::Char('a')));
    let text_value = screen(&mut line, 80, 30);

    assert!(
        text_value.contains("archive 1 thread(s)?") && text_value.contains("enter: yes"),
        "{text_value}"
    );
    assert_eq!(
        crate::engine::store::load(&thread.id).unwrap().state,
        "open",
        "0 archive before the yes"
    );

    line.key(key_event(KeyCode::Enter));

    assert_eq!(crate::engine::store::load(&thread.id).unwrap().state, "archived");
    assert!(line.take_changed());

    let text_2 = screen(&mut line, 80, 30);

    assert!(text_2.contains("archived"), "{text_2}");

    let _ = crate::engine::store::delete(&thread.id);
}

/// The thread in view archived from the menu: home.
#[test]
fn archiving_the_thread_in_view_goes_home() {
    let dir = std::env::temp_dir();
    let thread = crate::engine::store::Thread::new("here", "task", &dir, vec!["ada".into()]);

    thread.save().unwrap();
    let mut line = ThreadList::new(
        Mode::Switch,
        Some(thread.id.clone()),
        None,
        vec![card(&thread.id, |_| {})],
    );

    line.key(ctrl('.'));
    line.key(key_event(KeyCode::Char('a')));

    assert_eq!(line.key(key_event(KeyCode::Char('y'))), Out::Home);

    let _ = crate::engine::store::delete(&thread.id);
}

/// Archive of a thread with work not landed: the question names the work; 2 picks, 1 open:
/// the count.
#[test]
fn archive_of_an_open_thread_names_the_work() {
    use crate::domain::landed::Land;
    let cards = vec![
        card("loose", |character| character.land = Some(Land::Unpushed(3))),
        card("done", |character_value| character_value.land = Some(Land::Landed)),
    ];
    let mut line = ThreadList::new(Mode::Manage, None, None, cards);

    line.stat_pick = Some(StatusTab::All);
    line.key(key_event(KeyCode::Char('a')));
    let text = screen(&mut line, 120, 30);

    assert!(text.contains("3 unpushed: archive anyway?"), "{text}");

    line.key(key_event(KeyCode::Esc));

    line.key(key_event(KeyCode::Char(' ')));
    line.key(key_event(KeyCode::Char(' ')));
    line.key(key_event(KeyCode::Char('a')));
    let text_value = screen(&mut line, 120, 30);

    assert!(
        text_value.contains("archive 2 thread(s), 1 open (work not landed)?"),
        "{text_value}"
    );
}

/// The start screen: space picks, a asks for every pick, esc says no; e opens the form,
/// ctrl+s saves the title.
#[test]
fn manage_mode_selects_asks_and_edits() {
    let dir = std::env::temp_dir();
    let thread = crate::engine::store::Thread::new("old title", "task", &dir, vec!["ada".into()]);

    thread.save().unwrap();
    let mut line = ThreadList::new(
        Mode::Manage,
        None,
        None,
        vec![card(&thread.id, |_| {}), card("other", |_| {})],
    );

    line.roster = vec!["ada".into(), "bob".into()];
    line.key(key_event(KeyCode::Char(' ')));
    line.key(key_event(KeyCode::Char(' ')));

    assert_eq!(line.sel.len(), 2);

    line.key(key_event(KeyCode::Char('a')));
    let text = screen(&mut line, 100, 30);

    assert!(text.contains("archive 2 thread(s)?"), "{text}");

    line.key(key_event(KeyCode::Esc));

    assert!(line.job_state.pending.is_none() && line.sel.len() == 2);

    line.key(key_event(KeyCode::Esc));

    assert!(line.sel.is_empty(), "esc: 0 picks");

    line.cursor = 0;
    line.key(key_event(KeyCode::Char('e')));
    let text_value = screen(&mut line, 100, 30);

    assert!(
        text_value.contains("edit thread")
            && text_value.contains("old title")
            && text_value.contains("auto:")
            && text_value.contains("bob"),
        "{text_value}"
    );

    for _ in 0.."old title".len() {
        line.key(key_event(KeyCode::Backspace));
    }

    for character in "new title".chars() {
        line.key(key_event(KeyCode::Char(character)));
    }

    // agents: bob on (rows: title, repo, projects: auto, 0 links, ada, bob)
    for _ in 0..4 {
        line.key(key_event(KeyCode::Down));
    }

    line.key(key_event(KeyCode::Char(' ')));
    line.key(ctrl('s'));
    let text_2 = crate::engine::store::load(&thread.id).unwrap();

    assert_eq!(
        (text_2.title.as_str(), text_2.agents.clone()),
        ("new title", vec!["ada".to_string(), "bob".to_string()])
    );
    assert!(
        text_2.owner_choices.title_set && !text_2.owner_choices.projects_set,
        "the owner's title; projects: auto"
    );

    let _ = crate::engine::store::delete(&thread.id);
}

/// Repo tabs: the thread in view's repo first and picked; ←→ steps.
#[test]
fn the_repo_tab_of_the_thread_in_view_comes_first() {
    let cards = vec![
        card("a", |character| character.repo = Some("/r/one".into())),
        card("b", |character_value| character_value.repo = Some("/r/two".into())),
        card("c", |character_2| character_2.repo = Some("/r/two".into())),
    ];
    let mut line = ThreadList::new(Mode::Switch, Some("a".into()), None, cards);

    assert_eq!(line.repo_tab(), RepoTab::Repo("/r/one".into()));
    assert_eq!(line.visible().len(), 1);

    line.key(key_event(KeyCode::Right));

    assert_eq!(line.repo_tab(), RepoTab::Repo("/r/two".into()));
    assert_eq!(line.visible().len(), 2);
}
