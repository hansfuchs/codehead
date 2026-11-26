use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key_event(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
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
    let mut menu = menu();

    assert_eq!(menu.tab, Tab::Quick);

    menu.key(key_event(KeyCode::Right));

    assert_eq!((menu.tab, menu.level), (Tab::Threads, Level::Bar));

    // the tab bar keeps the keys: ←→ walk past the tabs with a tab group
    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Merge);

    menu.key(key_event(KeyCode::Left));

    assert_eq!(menu.tab, Tab::Threads);

    // ↓: the repo tabs (←→ there), ↓: the project tabs, ↓: the view tabs, ↓: the status
    // tabs, ↓: the list; ↑ at its top: status, views, projects, repos, the tab bar
    menu.key(key_event(KeyCode::Down));

    assert_eq!((menu.level, menu.list.focus), (Level::Body, thread_list::Focus::Repos));

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.list.focus, thread_list::Focus::Projects);

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.list.focus, thread_list::Focus::Views);

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.list.focus, thread_list::Focus::Status);

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.list.focus, thread_list::Focus::List);

    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Threads, "the list: ←→ walk the repos");

    menu.key(key_event(KeyCode::Up));
    menu.key(key_event(KeyCode::Up));
    menu.key(key_event(KeyCode::Up));
    menu.key(key_event(KeyCode::Up));

    assert_eq!(menu.list.focus, thread_list::Focus::Repos);

    assert_tabs_close(menu);
}

fn assert_tabs_close(mut menu: Menu) {
    menu.key(key_event(KeyCode::Up));

    assert_eq!(menu.level, Level::Bar);

    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Merge);

    // merge: the same 3 levels; tab walks them (bar → states → list → bar), shift+tab back
    menu.key(key_event(KeyCode::Down));
    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.level, Level::Body);

    menu.key(key_event(KeyCode::Up));
    menu.key(key_event(KeyCode::Up));

    assert_eq!(menu.level, Level::Bar);

    menu.key(key_event(KeyCode::Tab));

    assert_eq!((menu.tab, menu.level), (Tab::Merge, Level::Group));

    menu.key(key_event(KeyCode::Tab));

    assert_eq!(menu.level, Level::Body);

    menu.key(key_event(KeyCode::Tab));

    assert_eq!(menu.level, Level::Bar);

    menu.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert_eq!(menu.level, Level::Body);

    menu.key(key_event(KeyCode::Tab));
    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Search);

    // 0 agents tab, 0 tokens tab (the tokens box of the thread view, ctrl+o, has them) (the agents box of the thread view, tab, has their actions)
    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Quick);

    menu.key(key_event(KeyCode::Left));

    assert_eq!(menu.tab, Tab::Search);
    assert_eq!(menu.key(key_event(KeyCode::Esc)), Action::Close);

    // the menu key closes it too; tab: the next region (0 close)
    assert_eq!(
        menu.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        Action::Close
    );
    assert_ne!(menu.key(key_event(KeyCode::Tab)), Action::Close);
}

#[test]
fn a_command_palette_below_the_header_centered_on_wide_screens() {
    let mut menu = menu();

    for (width, height) in [(240u16, 60u16), (90, 30)] {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

        terminal.draw(|frame| menu.render(frame, frame.area())).unwrap();
        let bounds = menu.layout.frame;

        assert_eq!(bounds.y, 1, "below the header row");

        let (left, right) = (bounds.x, width - bounds.right());

        assert!(left.abs_diff(right) <= 1, "centered: {bounds:?} in {width}");
        assert!(bounds.width <= 100 && bounds.width >= 76.min(width - 4), "{bounds:?}");
    }
}

#[test]
fn quick_commands_run() {
    let mut menu = menu();

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Home(Home::New));

    // home: the start screen as it was (h too)
    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Back);
    assert_eq!(menu.key(key_event(KeyCode::Char('h'))), Action::Back);

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Home(Home::Settings));

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::None);
    assert_eq!(menu.tab, Tab::Search);

    menu.tab = Tab::Quick;
    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Command(7));

    // the last row: ↓ stays; ↑ at the 1st row: the tab bar
    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Command(7));

    for _ in 0..5 {
        menu.key(key_event(KeyCode::Up));
    }

    assert_eq!(menu.level, Level::Bar);

    menu.key(key_event(KeyCode::Down));

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Home(Home::New));
}

#[test]
fn search_finds_messages_moves_and_jumps() {
    use crate::domain::search::Msg;
    let mut menu = menu();

    // f: the search tab, its filter takes every letter (f too)
    menu.key(key_event(KeyCode::Char('f')));

    assert_eq!(menu.tab, Tab::Search);

    menu.set_msgs(vec![
        Msg::new("t1", "first", "owner", 10, "fix the deploy"),
        Msg::new("t2", "deploy fix", "ada", 20, "deploy runs, the deploy is fixed"),
    ]);

    for letter in "deploy".chars() {
        menu.key(key_event(KeyCode::Char(letter)));
    }

    // 2 hits for the 2nd message: it ranks first
    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Jump(card, 20) if card.id == "t2"));

    menu.key(key_event(KeyCode::Down));

    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Jump(card, 10) if card.id == "t1"));

    let text = {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();

        terminal.draw(|frame| menu.render(frame, frame.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();

        (0..30)
            .map(|y| (0..90).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>())
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

    menu.key(key_event(KeyCode::Char('x')));

    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::None));
}

#[test]
fn live_cards_keep_the_cursor_on_its_thread() {
    let mut menu = Menu::new(
        Vec::new(),
        None,
        Vec::new(),
        vec![card("t1", "a"), card("t2", "b"), card("t3", "c")],
    );

    menu.tab = Tab::Threads;
    menu.key(key_event(KeyCode::Down));

    // a read: t3 works now, it moves to the top; the cursor stays on t2
    let mut busy = card("t3", "c");

    busy.agents.busy = true;
    menu.set_cards(vec![busy, card("t1", "a"), card("t2", "b")]);

    assert!(menu.list.cards[0].agents.busy);
    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Open(card) if card.id == "t2"));
}

#[test]
fn live_messages_keep_the_cursor_on_its_message() {
    use crate::domain::search::Msg;
    let mut menu = menu();

    menu.key(key_event(KeyCode::Char('f')));
    menu.set_msgs(vec![
        Msg::new("t1", "a", "owner", 10, "deploy one"),
        Msg::new("t2", "b", "ada", 20, "deploy two"),
    ]);

    for letter in "deploy".chars() {
        menu.key(key_event(KeyCode::Char(letter)));
    }

    menu.key(key_event(KeyCode::Down));

    let Action::Jump(jumped, ts) = menu.key(key_event(KeyCode::Enter)) else {
        panic!("0 hit")
    };

    // a read with a new message: the cursor stays on the same message
    menu.set_msgs(vec![
        Msg::new("t1", "a", "owner", 10, "deploy one"),
        Msg::new("t2", "b", "ada", 20, "deploy two"),
        Msg::new("t3", "c", "ada", 30, "deploy three"),
    ]);

    assert_eq!(menu.search_state.hits.len(), 3);
    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Jump(card, at) if card.id == jumped.id && at == ts));
}

#[test]
fn live_quick_items_keep_the_cursor_on_its_command() {
    let item = |label: &str, id| Item {
        label: label.into(),
        key: String::new(),
        id,
        section: "agents",
    };
    let mut menu = Menu::new(
        vec![item("@a: flow (0 flow)", 100), item("@b: flow (0 flow)", 103)],
        None,
        Vec::new(),
        vec![card("t1", "a")],
    );

    for _ in 0..5 {
        menu.key(key_event(KeyCode::Down));
    }

    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Command(103));

    // @a leaves, @b's flow moves on: the cursor stays on @b's row, the label is new
    menu.set_items(vec![item("@b: flow (fix · step 2)", 103)]);

    assert_eq!(
        menu.items.last().map(|item| item.label.as_str()),
        Some("@b: flow (fix · step 2)")
    );
    assert_eq!(menu.key(key_event(KeyCode::Enter)), Action::Command(103));
}

#[test]
fn threads_filter_and_open() {
    let mut menu = menu();

    menu.key(key_event(KeyCode::Right));

    // repo tabs, project tabs, view tabs, status tabs, the list, its 2nd card
    for _ in 0..6 {
        menu.key(key_event(KeyCode::Down));
    }

    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Open(card) if card.id == "t2"));

    for letter in "first".chars() {
        menu.key(key_event(KeyCode::Char(letter)));
    }

    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Open(card) if card.id == "t1"));
}

#[test]
fn threads_tab_marks_the_thread_in_view_and_sorts_by_section() {
    let mut done = card("t2", "older");

    done.land = Some(crate::domain::landed::Land::Landed);
    let cards = vec![card("t1", "newest"), done, card("t3", "this one")];
    let mut menu = Menu::new(Vec::new(), Some("t3".into()), Vec::new(), cards);

    menu.key(key_event(KeyCode::Right));

    // a letter on the tab bar: the filter takes it
    menu.key(key_event(KeyCode::Char('e')));

    let ids = |menu: &Menu| {
        menu.list
            .visible()
            .iter()
            .map(|&i| menu.list.cards[i].id.clone())
            .collect::<Vec<_>>()
    };

    // the status tab of the thread in view (open): 0 shipped cards; all: shipped last
    assert_eq!(ids(&menu), ["t1", "t3"]);

    menu.list.stat_pick = Some(thread_list::StatusTab::All);

    assert_eq!(ids(&menu), ["t1", "t3", "t2"], "shipped last");

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 34)).unwrap();

    terminal.draw(|frame| menu.render(frame, frame.area())).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text: String = (0..34)
        .map(|y| (0..90).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>() + "\n")
        .collect();

    assert!(
        text.contains(" here ") && text.contains("open 2") && text.contains("shipped 1") && text.contains("ctrl+."),
        "{text}"
    );

    menu.key(key_event(KeyCode::Backspace));

    for letter in "older".chars() {
        menu.key(key_event(KeyCode::Char(letter)));
    }

    assert_eq!(ids(&menu), ["t2"]);
}

#[test]
fn threads_repo_tabs_start_on_the_current_threads_repo() {
    let repo = |id: &str, repo: &str| Card {
        repo: Some(repo.into()),
        ..card(id, id)
    };
    let cards = vec![
        repo("t1", "/x/alpha"),
        repo("b1", "/x/beta"),
        repo("b2", "/x/beta"),
        repo("g1", "/x/gamma"),
        card("o1", "o1"),
    ];
    let mut menu = Menu::new(Vec::new(), Some("t1".into()), Vec::new(), cards);

    menu.key(key_event(KeyCode::Right));

    assert_eq!(menu.tab, Tab::Threads);

    menu.key(key_event(KeyCode::Down));

    let draw = |menu: &mut Menu| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();

        terminal.draw(|frame| menu.render(frame, frame.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();

        (0..30)
            .map(|y| (0..90).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
    };

    // the current thread's repo 1st, then the others by thread count, outside git, all
    let text = draw(&mut menu);
    let row = text.iter().find(|line| line.contains("repo ")).expect("repo tabs");

    let at = |label: &str| row.find(label).unwrap_or_else(|| panic!("{label}: {row}"));

    assert!(
        at(" alpha ") < at(" beta ")
            && at(" beta ") < at(" gamma ")
            && at(" gamma ") < at(" outside git ")
            && at(" outside git ") < at(" all "),
        "{row}"
    );

    assert_repository_tabs(menu, draw, &text);
}

fn assert_repository_tabs(mut menu: Menu, draw: impl Fn(&mut Menu) -> Vec<String>, text: &[String]) {
    assert!(!text.join("\n").contains("ctrl+r"));

    let ids = |menu: &Menu| {
        menu.list
            .visible()
            .iter()
            .map(|&i| menu.list.cards[i].id.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(ids(&menu), ["t1"], "the current thread's repo by default");

    // →: beta (2 threads); the name filter applies on top
    menu.key(key_event(KeyCode::Right));

    assert_eq!(ids(&menu), ["b1", "b2"]);

    menu.key(key_event(KeyCode::Char('2')));

    assert!(matches!(menu.key(key_event(KeyCode::Enter)), Action::Open(card) if card.id == "b2"));

    menu.key(key_event(KeyCode::Backspace));

    // ←← wraps: all
    menu.key(key_event(KeyCode::Left));
    menu.key(key_event(KeyCode::Left));

    assert_eq!(ids(&menu).len(), 5);

    // a click on a tab picks it
    draw(&mut menu);
    let rect = menu.list.repo_rect(1).expect("tab 1 drawn");

    menu.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(ids(&menu), ["b1", "b2"]);
}

#[test]
#[ignore = "reads the real threads"]
fn live_threads_tab() {
    let cards = thread_card::load();
    let mut menu = Menu::new(
        Vec::new(),
        cards.first().map(|card| card.id.clone()),
        crate::domain::agents::cached(),
        cards,
    );

    menu.key(key_event(KeyCode::Right));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 34)).unwrap();

    terminal.draw(|frame| menu.render(frame, frame.area())).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text: Vec<String> = (0..34)
        .map(|y| (0..100).map(|x| buffer[(x, y)].symbol().to_string()).collect())
        .collect();

    println!("{}", text.join("\n"));
}
