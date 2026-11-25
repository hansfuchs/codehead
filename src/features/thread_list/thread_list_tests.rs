use super::*;

fn key_event(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(letter: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL)
}

fn card(id: &str, edit: impl FnOnce(&mut Card)) -> Card {
    let mut card = Card {
        id: id.into(),
        title: id.into(),
        state: "open".into(),
        ..Default::default()
    };

    edit(&mut card);
    card
}

fn screen(list: &mut ThreadList, width: u16, height: u16) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    terminal.draw(|frame| list.draw(frame, frame.area(), 0)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
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
        card("asks", |card| card.agents.blocked = true),
        card("busy", |card| card.agents.busy = true),
        card("shipped", |card| card.land = Some(Land::Landed)),
        card("talk", |card| {
            card.land = Some(Land::NoCode);
            card.completion.done = true;
        }),
        card("plan", |card| {
            card.land = Some(Land::Landed);
            card.completion.review = true;
        }),
        card("onmain", |card| {
            card.land = Some(Land::Main);
            card.completion.done = true;
            card.activity = now - hour;
        }),
        card("mainidle", |card| {
            card.land = Some(Land::Main);
            card.completion.done = true;
            card.activity = now - 4 * 24 * hour;
        }),
        card("new", |card| {
            card.land = Some(Land::NoCode);
            card.activity = now - hour;
        }),
        card("dusty", |card| {
            card.land = Some(Land::NoCode);
            card.activity = now - 5 * 24 * hour - hour;
        }),
        card("loose", |card| {
            card.land = Some(Land::Unpushed(3));
            card.activity = now - 9 * 24 * hour;
        }),
        card("waits", |card| {
            card.agents.blocked = true;
            card.land = Some(Land::NoCode);
            card.activity = now - 4 * 24 * hour;
        }),
        card("old", |card| card.state = "archived".into()),
    ];
    let mut list = ThreadList::new(Mode::Switch, None, None, cards);

    let ids = |list: &ThreadList| -> Vec<String> { list.visible().iter().map(|&i| list.cards[i].id.clone()).collect() };

    // status tab open first: the open cards only
    assert_eq!(list.stat_tab(), StatusTab::Open);
    assert_eq!(ids(&list), ["idle", "asks", "busy", "plan", "onmain", "new", "loose"]);

    // 0 code: shipped only when the agents reported done; idle 3 days: stale (code: never);
    // a step to review: open; on main: never shipped (done or not), stale after 3 days
    list.stat_pick = Some(StatusTab::All);

    check_the_all_tab(list, ids);
}

fn check_the_all_tab(mut list: ThreadList, ids: impl Fn(&ThreadList) -> Vec<String>) {
    assert_eq!(
        ids(&list),
        [
            "idle", "asks", "busy", "plan", "onmain", "new", "loose", "mainidle", "dusty", "waits", "shipped", "talk"
        ]
    );

    let text = screen(&mut list, 80, 120);

    for expected in [
        "status     open 7   stale 3   shipped 2   all 12",
        "◇ 3 unpushed",
        "⇲ shipped",
        "⇲ nothing to ship",
        "auto-archive in 25d",
        "▷ step to review",
        "◌ on main · auto-archive in 26d",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }

    // 0 section labels in the list (the status row holds them)
    for absent in [
        "open ·",
        "stale ·",
        "shipped: archive?",
        "needs you ·",
        "recent ·",
        "working ·",
        "threads ·",
    ] {
        assert!(!text.contains(absent), "{absent}: {text}");
    }

    assert_eq!(text.matches("nothing to ship").count(), 1, "{text}");
    assert!(
        text.contains("active 12") && text.contains("archived 1") && !text.contains("old"),
        "{text}"
    );

    // ↑ from the 1st card: the status tabs; ← stale (its hint on the row); ↑ the view tabs
    check_the_stale_tab_and_archived_view(list, ids);
}

fn check_the_stale_tab_and_archived_view(mut list: ThreadList, ids: impl Fn(&ThreadList) -> Vec<String>) {
    list.key(key_event(KeyCode::Up));

    assert_eq!(list.focus, Focus::Status);

    list.key(key_event(KeyCode::Left));
    list.key(key_event(KeyCode::Left));

    assert_eq!(list.stat_tab(), StatusTab::Stale);
    assert_eq!(ids(&list), ["mainidle", "dusty", "waits"]);

    let stale = screen(&mut list, 100, 40);

    assert!(
        stale.contains("stale 3   shipped 2   all 12  0 code or on main, idle 3+ days"),
        "{stale}"
    );

    // the filter narrows the counts too
    list.filter.set_text("dusty");
    let filtered = screen(&mut list, 100, 40);

    assert!(filtered.contains("open 0   stale 1   shipped 0   all 1"), "{filtered}");

    list.filter.set_text("");
    list.key(key_event(KeyCode::Up));

    assert_eq!(list.focus, Focus::Views);

    list.key(key_event(KeyCode::Right));
    list.key(key_event(KeyCode::Down));

    assert_eq!(
        (list.view, list.focus),
        (View::Archived, Focus::List),
        "archived: 0 status tabs, ↓ to the list"
    );

    let archived = screen(&mut list, 70, 40);

    assert!(
        archived.contains("old")
            && archived.contains("archived · 1")
            && archived.contains("▣ archived")
            && !archived.contains("status "),
        "{archived}"
    );
}

/// The status row stays while the list scrolls; the thread in view picks its status tab.
#[test]
fn the_status_row_stays_out_of_the_scroll() {
    use crate::domain::landed::Land;
    let mut cards: Vec<Card> = (0..30).map(|i| card(&format!("t{i:02}"), |_| {})).collect();

    cards.push(card("done", |card| card.land = Some(Land::Landed)));
    let mut list = ThreadList::new(Mode::Switch, Some("done".into()), None, cards);

    assert_eq!(list.stat_tab(), StatusTab::Shipped, "the thread in view's section");
    assert_eq!(list.visible().len(), 1);

    list.stat_pick = Some(StatusTab::Open);

    for _ in 0..20 {
        list.key(key_event(KeyCode::Down));
    }

    let text = screen(&mut list, 80, 30);
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
    let mut list = ThreadList::new(
        Mode::Switch,
        None,
        None,
        vec![card(&thread.id, |card| {
            card.title = "archive from menu".into();
        })],
    );

    list.key(key_event(KeyCode::Char('a')));

    assert_eq!(list.query(), "a", "a letter filters in the menu");

    list.key(key_event(KeyCode::Backspace));
    list.key(ctrl('.'));
    let text = screen(&mut list, 80, 30);

    assert!(
        text.contains("actions") && text.contains("archive: worktree goes") && text.contains("delete"),
        "{text}"
    );

    list.key(key_event(KeyCode::Char('a')));
    let question = screen(&mut list, 80, 30);

    assert!(
        question.contains("archive 1 thread(s)?") && question.contains("enter: yes"),
        "{question}"
    );
    assert_eq!(
        crate::engine::store::load(&thread.id).unwrap().state,
        "open",
        "0 archive before the yes"
    );

    list.key(key_event(KeyCode::Enter));

    assert_eq!(crate::engine::store::load(&thread.id).unwrap().state, "archived");
    assert!(list.take_changed());

    let archived = screen(&mut list, 80, 30);

    assert!(archived.contains("archived"), "{archived}");

    let _ = crate::engine::store::delete(&thread.id);
}

/// The thread in view archived from the menu: home.
#[test]
fn archiving_the_thread_in_view_goes_home() {
    let dir = std::env::temp_dir();
    let thread = crate::engine::store::Thread::new("here", "task", &dir, vec!["ada".into()]);

    thread.save().unwrap();
    let mut list = ThreadList::new(
        Mode::Switch,
        Some(thread.id.clone()),
        None,
        vec![card(&thread.id, |_| {})],
    );

    list.key(ctrl('.'));
    list.key(key_event(KeyCode::Char('a')));

    assert_eq!(list.key(key_event(KeyCode::Char('y'))), Out::Home);

    let _ = crate::engine::store::delete(&thread.id);
}

/// Archive of a thread with work not landed: the question names the work; 2 picks, 1 open:
/// the count.
#[test]
fn archive_of_an_open_thread_names_the_work() {
    use crate::domain::landed::Land;
    let cards = vec![
        card("loose", |card| card.land = Some(Land::Unpushed(3))),
        card("done", |card| card.land = Some(Land::Landed)),
    ];
    let mut list = ThreadList::new(Mode::Manage, None, None, cards);

    list.stat_pick = Some(StatusTab::All);
    list.key(key_event(KeyCode::Char('a')));
    let text = screen(&mut list, 120, 30);

    assert!(text.contains("3 unpushed: archive anyway?"), "{text}");

    list.key(key_event(KeyCode::Esc));

    list.key(key_event(KeyCode::Char(' ')));
    list.key(key_event(KeyCode::Char(' ')));
    list.key(key_event(KeyCode::Char('a')));
    let question = screen(&mut list, 120, 30);

    assert!(
        question.contains("archive 2 thread(s), 1 open (work not landed)?"),
        "{question}"
    );
}

/// The start screen: space picks, a asks for every pick, esc says no; e opens the form,
/// ctrl+s saves the title.
#[test]
fn manage_mode_selects_asks_and_edits() {
    let dir = std::env::temp_dir();
    let thread = crate::engine::store::Thread::new("old title", "task", &dir, vec!["ada".into()]);

    thread.save().unwrap();
    let mut list = ThreadList::new(
        Mode::Manage,
        None,
        None,
        vec![card(&thread.id, |_| {}), card("other", |_| {})],
    );

    list.roster = vec!["ada".into(), "bob".into()];
    list.key(key_event(KeyCode::Char(' ')));
    list.key(key_event(KeyCode::Char(' ')));

    assert_eq!(list.sel.len(), 2);

    list.key(key_event(KeyCode::Char('a')));
    let text = screen(&mut list, 100, 30);

    assert!(text.contains("archive 2 thread(s)?"), "{text}");

    list.key(key_event(KeyCode::Esc));

    assert!(list.job_state.pending.is_none() && list.sel.len() == 2);

    list.key(key_event(KeyCode::Esc));

    assert!(list.sel.is_empty(), "esc: 0 picks");

    list.cursor = 0;
    list.key(key_event(KeyCode::Char('e')));
    let form = screen(&mut list, 100, 30);

    assert!(
        form.contains("edit thread") && form.contains("old title") && form.contains("auto:") && form.contains("bob"),
        "{form}"
    );

    for _ in 0.."old title".len() {
        list.key(key_event(KeyCode::Backspace));
    }

    for letter in "new title".chars() {
        list.key(key_event(KeyCode::Char(letter)));
    }

    // agents: bob on (rows: title, repo, projects: auto, 0 links, ada, bob)
    for _ in 0..4 {
        list.key(key_event(KeyCode::Down));
    }

    list.key(key_event(KeyCode::Char(' ')));
    list.key(ctrl('s'));
    let saved = crate::engine::store::load(&thread.id).unwrap();

    assert_eq!(
        (saved.title.as_str(), saved.agents.clone()),
        ("new title", vec!["ada".to_string(), "bob".to_string()])
    );
    assert!(
        saved.owner_choices.title_set && !saved.owner_choices.projects_set,
        "the owner's title; projects: auto"
    );

    let _ = crate::engine::store::delete(&thread.id);
}

/// Repo tabs: the thread in view's repo first and picked; ←→ steps.
#[test]
fn the_repo_tab_of_the_thread_in_view_comes_first() {
    let cards = vec![
        card("a", |card| card.repo = Some("/r/one".into())),
        card("b", |card| card.repo = Some("/r/two".into())),
        card("c", |card| card.repo = Some("/r/two".into())),
    ];
    let mut list = ThreadList::new(Mode::Switch, Some("a".into()), None, cards);

    assert_eq!(list.repo_tab(), RepoTab::Repo("/r/one".into()));
    assert_eq!(list.visible().len(), 1);

    list.key(key_event(KeyCode::Right));

    assert_eq!(list.repo_tab(), RepoTab::Repo("/r/two".into()));
    assert_eq!(list.visible().len(), 2);
}
