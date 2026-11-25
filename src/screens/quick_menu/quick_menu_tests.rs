use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key_event(character: KeyCode) -> KeyEvent {
    KeyEvent::new(character, KeyModifiers::NONE)
}

fn card(id: &str, title: &str) -> Card {
    Card {
        id: id.into(),
        title: title.into(),
        state: "open".into(),
        ..Default::default()
    }
}

fn agent(name: &str) -> Agent {
    Agent {
        name: name.into(),
        harness: "claude".into(),
        model: None,
        color: None,
        mode: String::new(),
        prompt: None,
        last_seen: None,
    }
}

fn menu() -> Menu {
    let items = vec![Item {
        label: "keys".into(),
        key: "?".into(),
        id: 7,
        section: "help",
    }];

    Menu::new(
        items,
        Some("t1".into()),
        vec![agent("amon"), agent("lilith")],
        vec![card("t1", "first"), card("t2", "deploy fix")],
    )
}

#[test]
fn tabs_cycle_and_esc_closes() {
    let mut message = menu();

    assert_eq!(message.tab, Tab::Quick);

    message.key(key_event(KeyCode::Right));

    assert_eq!((message.tab, message.level), (Tab::Threads, Level::Bar));

    // the tab bar keeps the keys: ←→ walk past the tabs with a tab group
    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Merge);

    message.key(key_event(KeyCode::Left));

    assert_eq!(message.tab, Tab::Threads);

    // ↓: the repo tabs (←→ there), ↓: the project tabs, ↓: the view tabs, ↓: the status
    // tabs, ↓: the list; ↑ at its top: status, views, projects, repos, the tab bar
    message.key(key_event(KeyCode::Down));

    assert_eq!(
        (message.level, message.list.focus),
        (Level::Body, thread_list::Focus::Repos)
    );

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.list.focus, thread_list::Focus::Projects);

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.list.focus, thread_list::Focus::Views);

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.list.focus, thread_list::Focus::Status);

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.list.focus, thread_list::Focus::List);

    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Threads, "the list: ←→ walk the repos");

    message.key(key_event(KeyCode::Up));
    message.key(key_event(KeyCode::Up));
    message.key(key_event(KeyCode::Up));
    message.key(key_event(KeyCode::Up));

    assert_eq!(message.list.focus, thread_list::Focus::Repos);

    assert_tabs_close(message);
}

fn assert_tabs_close(mut message: Menu) {
    message.key(key_event(KeyCode::Up));

    assert_eq!(message.level, Level::Bar);

    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Merge);

    // merge: the same 3 levels; tab walks them (bar → states → list → bar), shift+tab back
    message.key(key_event(KeyCode::Down));
    message.key(key_event(KeyCode::Down));

    assert_eq!(message.level, Level::Body);

    message.key(key_event(KeyCode::Up));
    message.key(key_event(KeyCode::Up));

    assert_eq!(message.level, Level::Bar);

    message.key(key_event(KeyCode::Tab));

    assert_eq!((message.tab, message.level), (Tab::Merge, Level::Group));

    message.key(key_event(KeyCode::Tab));

    assert_eq!(message.level, Level::Body);

    message.key(key_event(KeyCode::Tab));

    assert_eq!(message.level, Level::Bar);

    message.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert_eq!(message.level, Level::Body);

    message.key(key_event(KeyCode::Tab));
    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Search);

    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Tokens);

    // 0 agents tab (the agents box of the thread view, tab, has their actions)
    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Quick);

    message.key(key_event(KeyCode::Left));

    assert_eq!(message.tab, Tab::Tokens);
    assert_eq!(message.key(key_event(KeyCode::Esc)), Action::Close);

    // the menu key closes it too; tab: the next region (0 close)
    assert_eq!(
        message.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        Action::Close
    );
    assert_ne!(message.key(key_event(KeyCode::Tab)), Action::Close);
}

#[test]
fn a_command_palette_below_the_header_centered_on_wide_screens() {
    let mut message = menu();

    for (width_value, height) in [(240u16, 60u16), (90, 30)] {
        let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width_value, height)).unwrap();

        thread.draw(|file| message.render(file, file.area())).unwrap();
        let result = message.layout.frame;

        assert_eq!(result.y, 1, "below the header row");

        let (left, right) = (result.x, width_value - result.right());

        assert!(left.abs_diff(right) <= 1, "centered: {result:?} in {width_value}");
        assert!(
            result.width <= 100 && result.width >= 76.min(width_value - 4),
            "{result:?}"
        );
    }
}

#[test]
fn quick_commands_run() {
    let mut message = menu();

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Home(Home::New));

    // home: the start screen as it was (h too)
    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Back);
    assert_eq!(message.key(key_event(KeyCode::Char('h'))), Action::Back);

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Home(Home::Settings));

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::None);
    assert_eq!(message.tab, Tab::Search);

    message.tab = Tab::Quick;
    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Command(7));

    // the last row: ↓ stays; ↑ at the 1st row: the tab bar
    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Command(7));

    for _ in 0..5 {
        message.key(key_event(KeyCode::Up));
    }

    assert_eq!(message.level, Level::Bar);

    message.key(key_event(KeyCode::Down));

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Home(Home::New));
}

#[test]
fn search_finds_messages_moves_and_jumps() {
    use crate::domain::search::Msg;
    let mut message = menu();

    // f: the search tab, its filter takes every letter (f too)
    message.key(key_event(KeyCode::Char('f')));

    assert_eq!(message.tab, Tab::Search);

    message.set_msgs(vec![
        Msg::new("t1", "first", "owner", 10, "fix the deploy"),
        Msg::new("t2", "deploy fix", "ada", 20, "deploy runs, the deploy is fixed"),
    ]);

    for character in "deploy".chars() {
        message.key(key_event(KeyCode::Char(character)));
    }

    // 2 hits for the 2nd message: it ranks first
    assert!(
        matches!(message.key(key_event(KeyCode::Enter)), Action::Jump(character_value, 20) if character_value.id == "t2")
    );

    message.key(key_event(KeyCode::Down));

    assert!(matches!(message.key(key_event(KeyCode::Enter)), Action::Jump(character_2, 10) if character_2.id == "t1"));

    let text = {
        let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();

        thread.draw(|file| message.render(file, file.area())).unwrap();
        let bytes = thread.backend().buffer().clone();

        (0..30)
            .map(|y| (0..90).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };

    assert!(
        text.contains(" search ")
            && text.contains("you · this thread")
            && text.contains("ada · deploy fix")
            && text.contains("fix the deploy"),
        "{text}"
    );

    message.key(key_event(KeyCode::Char('x')));

    assert!(matches!(message.key(key_event(KeyCode::Enter)), Action::None));
}

#[test]
fn live_cards_keep_the_cursor_on_its_thread() {
    let mut message = Menu::new(
        Vec::new(),
        None,
        Vec::new(),
        vec![card("t1", "a"), card("t2", "b"), card("t3", "c")],
    );

    message.tab = Tab::Threads;
    message.key(key_event(KeyCode::Down));

    // a read: t3 works now, it moves to the top; the cursor stays on t2
    let mut busy = card("t3", "c");

    busy.agents.busy = true;
    message.set_cards(vec![busy, card("t1", "a"), card("t2", "b")]);

    assert!(message.list.cards[0].agents.busy);
    assert!(matches!(message.key(key_event(KeyCode::Enter)), Action::Open(character) if character.id == "t2"));
}

#[test]
fn live_messages_keep_the_cursor_on_its_message() {
    use crate::domain::search::Msg;
    let mut message = menu();

    message.key(key_event(KeyCode::Char('f')));
    message.set_msgs(vec![
        Msg::new("t1", "a", "owner", 10, "deploy one"),
        Msg::new("t2", "b", "ada", 20, "deploy two"),
    ]);

    for character in "deploy".chars() {
        message.key(key_event(KeyCode::Char(character)));
    }

    message.key(key_event(KeyCode::Down));

    let Action::Jump(character_value, ts) = message.key(key_event(KeyCode::Enter)) else {
        panic!("0 hit")
    };

    // a read with a new message: the cursor stays on the same message
    message.set_msgs(vec![
        Msg::new("t1", "a", "owner", 10, "deploy one"),
        Msg::new("t2", "b", "ada", 20, "deploy two"),
        Msg::new("t3", "c", "ada", 30, "deploy three"),
    ]);

    assert_eq!(message.search_state.hits.len(), 3);
    assert!(
        matches!(message.key(key_event(KeyCode::Enter)), Action::Jump(directory, thread) if directory.id == character_value.id && thread == ts)
    );
}

#[test]
fn live_quick_items_keep_the_cursor_on_its_command() {
    let it = |label: &str, id| Item {
        label: label.into(),
        key: String::new(),
        id,
        section: "agents",
    };
    let mut message = Menu::new(
        vec![it("@a: flow (0 flow)", 100), it("@b: flow (0 flow)", 103)],
        None,
        Vec::new(),
        vec![card("t1", "a")],
    );

    for _ in 0..5 {
        message.key(key_event(KeyCode::Down));
    }

    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Command(103));

    // @a leaves, @b's flow moves on: the cursor stays on @b's row, the label is new
    message.set_items(vec![it("@b: flow (fix · step 2)", 103)]);

    assert_eq!(
        message.items.last().map(|i| i.label.as_str()),
        Some("@b: flow (fix · step 2)")
    );
    assert_eq!(message.key(key_event(KeyCode::Enter)), Action::Command(103));
}

#[test]
fn threads_filter_and_open() {
    let mut message = menu();

    message.key(key_event(KeyCode::Right));

    // repo tabs, project tabs, view tabs, status tabs, the list, its 2nd card
    for _ in 0..6 {
        message.key(key_event(KeyCode::Down));
    }

    assert!(
        matches!(message.key(key_event(KeyCode::Enter)), Action::Open(character_value) if character_value.id == "t2")
    );

    for character in "first".chars() {
        message.key(key_event(KeyCode::Char(character)));
    }

    assert!(matches!(message.key(key_event(KeyCode::Enter)), Action::Open(character_2) if character_2.id == "t1"));
}

#[test]
fn threads_tab_marks_the_thread_in_view_and_sorts_by_section() {
    let mut done = card("t2", "older");

    done.land = Some(crate::domain::landed::Land::Landed);
    let cards = vec![card("t1", "newest"), done, card("t3", "this one")];
    let mut message = Menu::new(Vec::new(), Some("t3".into()), Vec::new(), cards);

    message.key(key_event(KeyCode::Right));

    // a letter on the tab bar: the filter takes it
    message.key(key_event(KeyCode::Char('e')));

    let ids = |message_value: &Menu| {
        message_value
            .list
            .visible()
            .iter()
            .map(|&i| message_value.list.cards[i].id.clone())
            .collect::<Vec<_>>()
    };

    // the status tab of the thread in view (open): 0 shipped cards; all: shipped last
    assert_eq!(ids(&message), ["t1", "t3"]);

    message.list.stat_pick = Some(thread_list::StatusTab::All);

    assert_eq!(ids(&message), ["t1", "t3", "t2"], "shipped last");

    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 34)).unwrap();

    thread.draw(|file| message.render(file, file.area())).unwrap();
    let bytes = thread.backend().buffer().clone();
    let text: String = (0..34)
        .map(|y| (0..90).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>() + "\n")
        .collect();

    assert!(
        text.contains(" here ") && text.contains("open 2") && text.contains("shipped 1") && text.contains("ctrl+."),
        "{text}"
    );

    message.key(key_event(KeyCode::Backspace));

    for character in "older".chars() {
        message.key(key_event(KeyCode::Char(character)));
    }

    assert_eq!(ids(&message), ["t2"]);
}

#[test]
fn threads_repo_tabs_start_on_the_current_threads_repo() {
    let repo = |character: &str, result: &str| Card {
        repo: Some(result.into()),
        ..card(character, character)
    };
    let cards = vec![
        repo("t1", "/x/alpha"),
        repo("b1", "/x/beta"),
        repo("b2", "/x/beta"),
        repo("g1", "/x/gamma"),
        card("o1", "o1"),
    ];
    let mut message = Menu::new(Vec::new(), Some("t1".into()), Vec::new(), cards);

    message.key(key_event(KeyCode::Right));

    assert_eq!(message.tab, Tab::Threads);

    message.key(key_event(KeyCode::Down));

    let draw = |message_value: &mut Menu| {
        let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();

        thread.draw(|file| message_value.render(file, file.area())).unwrap();
        let bytes = thread.backend().buffer().clone();

        (0..30)
            .map(|y| (0..90).map(|x| bytes[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
    };

    // the current thread's repo 1st, then the others by thread count, outside git, all
    let text = draw(&mut message);
    let row = text.iter().find(|line| line.contains("repo ")).expect("repo tabs");

    let at = |thread_value: &str| {
        row.find(thread_value)
            .unwrap_or_else(|| panic!("{thread_value}: {row}"))
    };

    assert!(
        at(" alpha ") < at(" beta ")
            && at(" beta ") < at(" gamma ")
            && at(" gamma ") < at(" outside git ")
            && at(" outside git ") < at(" all "),
        "{row}"
    );

    assert_repository_tabs(message, draw, &(text));
}

fn assert_repository_tabs(mut message: Menu, draw: impl Fn(&mut Menu) -> Vec<String>, text: &[String]) {
    assert!(!text.join("\n").contains("ctrl+r"));

    let ids = |message_2: &Menu| {
        message_2
            .list
            .visible()
            .iter()
            .map(|&i| message_2.list.cards[i].id.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(ids(&message), ["t1"], "the current thread's repo by default");

    // →: beta (2 threads); the name filter applies on top
    message.key(key_event(KeyCode::Right));

    assert_eq!(ids(&message), ["b1", "b2"]);

    message.key(key_event(KeyCode::Char('2')));

    assert!(matches!(message.key(key_event(KeyCode::Enter)), Action::Open(character) if character.id == "b2"));

    message.key(key_event(KeyCode::Backspace));

    // ←← wraps: all
    message.key(key_event(KeyCode::Left));
    message.key(key_event(KeyCode::Left));

    assert_eq!(ids(&message).len(), 5);

    // a click on a tab picks it
    draw(&mut message);
    let result_value = message.list.repo_rect(1).expect("tab 1 drawn");

    message.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: result_value.x,
        row: result_value.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(ids(&message), ["b1", "b2"]);
}

#[test]
#[ignore = "reads the real threads"]
fn live_threads_tab() {
    let cards = thread_card::load();
    let mut message = Menu::new(
        Vec::new(),
        cards.first().map(|character| character.id.clone()),
        crate::domain::agents::cached(),
        cards,
    );

    message.key(key_event(KeyCode::Right));
    let mut thread_value = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 34)).unwrap();

    thread_value.draw(|file| message.render(file, file.area())).unwrap();
    let bytes = thread_value.backend().buffer().clone();
    let text: Vec<String> = (0..34)
        .map(|y| (0..100).map(|x| bytes[(x, y)].symbol().to_string()).collect())
        .collect();

    println!("{}", text.join("\n"));
    message.key(key_event(KeyCode::Right));
    message.tokens.detail = Some(crate::features::cost::detail(message.thread.as_deref()));
    thread_value
        .draw(|file_value| message.render(file_value, file_value.area()))
        .unwrap();
    let bytes_value = thread_value.backend().buffer().clone();
    let text_value: Vec<String> = (0..34)
        .map(|y| (0..100).map(|x| bytes_value[(x, y)].symbol().to_string()).collect())
        .collect();

    println!("{}", text_value.join("\n"));
}
