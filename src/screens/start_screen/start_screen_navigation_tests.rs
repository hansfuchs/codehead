use super::*;

#[test]
fn settings_esc_undoes_the_field_edit_and_never_applies() {
    let mut app = test_app();
    let before = std::fs::read_to_string(&app.settings_path).unwrap_or_default();

    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.tab == Tab::Settings && !app.tabs_focus);

    // a plain field: typed, esc → the text at entry, 0 write
    let settings = app.settings.as_mut().unwrap();
    let start = settings.form.fields[0].text();

    app.paste("/tmp/elsewhere/");

    assert_ne!(app.settings.as_ref().unwrap().form.fields[0].text(), start);

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(app.settings.as_ref().unwrap().form.fields[0].text(), start);
    assert_eq!(std::fs::read_to_string(&app.settings_path).unwrap_or_default(), before);

    // an alias: unlocked, typed, esc → 0 alias, locked, 0 write
    // the menu → add path alias (its keys end the esc esc window)
    app.overlays.last_esc = None;
    app.run(Act::AddAlias);
    app.paste("project:x");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    let settings = app.settings.as_ref().unwrap();

    assert!(
        settings.form.editing.is_none() && settings.form.fields[crate::features::settings::PLAIN].text().is_empty()
    );
    assert_eq!(std::fs::read_to_string(&app.settings_path).unwrap_or_default(), before);

    // shift+tab: the sub-tabs row; tab: back to the form, on the same field; settings stays
    let at = app.settings.as_ref().unwrap().form.focus;

    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(app.settings.as_ref().unwrap().bar && !app.tabs_focus);

    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(app.tabs_focus);

    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);

    assert!(!app.tabs_focus && !app.settings.as_ref().unwrap().bar);
    assert_eq!(app.settings.as_ref().unwrap().form.focus, at);
    assert_eq!(app.tab, Tab::Settings);
}

#[test]
fn alt_digits_switch_tabs_and_tab_walks_the_regions() {
    let mut app = test_app();
    let alt = |app: &mut App, number: char| key(app, KeyCode::Char(number), KeyModifiers::ALT);

    alt(&mut app, '2');

    assert!(app.compose.screen == Screen::Pick && app.tab == Tab::Threads && !app.tabs_focus);

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.compose.screen == Screen::Pick && app.tab == Tab::Threads);

    // tab on a list: the tabs row (0 tab switch), tab again: the list
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);

    assert!(app.tab == Tab::Threads && app.tabs_focus);

    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);

    assert!(app.tab == Tab::Threads && !app.tabs_focus);

    alt(&mut app, '1');

    assert!(app.compose.screen == Screen::Compose && app.compose.field == Field::Prompt);

    // alt+2..4: threads, agents, settings
    for (tab, number) in [
        (Tab::Threads, '2'),
        (Tab::Projects, '3'),
        (Tab::Merge, '4'),
        (Tab::Agents, '5'),
        (Tab::Flows, '6'),
        (Tab::Settings, '7'),
    ] {
        alt(&mut app, number);

        assert!(app.compose.screen == Screen::Pick && app.tab == tab);
    }

    alt_digits_switch_tabs_and_tab_walks_the_regions_verify_word(app, alt);
}

fn alt_digits_switch_tabs_and_tab_walks_the_regions_verify_word(mut app: App, alt: impl Fn(&mut App, char)) {
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(app.tab, Tab::Settings);

    alt(&mut app, '1');

    assert!(app.compose.screen == Screen::Compose);

    // the agents tab loaded the machine's agents: 1 card again (↓ leaves the cards)
    app.agent_list.agents.truncate(1);
    app.compose.field = Field::Prompt;

    // tab: the tabs row, tab: back on the same field (tab never walks the fields)
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);

    assert!(app.tabs_focus && app.compose.screen == Screen::Compose);

    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);

    assert!(!app.tabs_focus && app.compose.field == Field::Prompt);

    app.compose.field = Field::Notes;

    // ↑↓ walk the fields: ↓ on the last stays, ↑ on the prompt's 1st row: the tabs
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Notes);

    for file in [Field::Agents, Field::Repo, Field::Title, Field::Prompt, Field::Drafts] {
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);

        assert!(app.compose.field == file);
    }

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.tabs_focus);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    // alt+digit in the prompt: tabs; ctrl / ⌥+←→ (alt on macOS; off macOS alt+←: row start): word jumps
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    type_text(&mut app, "one two");

    let word = if cfg!(target_os = "macos") {
        KeyModifiers::ALT
    } else {
        KeyModifiers::CONTROL
    };

    key(&mut app, KeyCode::Left, word);
    type_text(&mut app, "X");

    assert_eq!(app.compose.prompt.text(), "one Xtwo");

    // ctrl+←: a word jump too, 0 tab switch
    key(&mut app, KeyCode::Left, KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Left, KeyModifiers::CONTROL);
    type_text(&mut app, "Y");

    assert!(
        app.compose.screen == Screen::Compose && app.compose.prompt.text() == "Yone Xtwo",
        "{}",
        app.compose.prompt.text()
    );

    alt(&mut app, '2');

    assert!(app.tab == Tab::Threads && app.compose.screen == Screen::Pick);
}

#[test]
fn title_is_optional_the_prompt_line_1_names_the_thread() {
    let mut app = test_app();

    app.agent_list.agents.truncate(1);
    app.compose.picks = vec![app.agent_list.agents[0].name.clone()];

    // 0 prompt, 0 title: 0 start
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);

    assert!(app.outcome.is_none() && app.compose.field == Field::Prompt);

    type_text(&mut app, "  ");
    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);

    assert!(app.outcome.is_none());

    app.compose
        .prompt
        .set_text("\nFix the deploy script\nit fails on arm64");

    assert_eq!(app.task_title(), "Fix the deploy script");

    // the message: the prompt first (line 1 = the title), 0 line twice
    assert_eq!(app.message(), "Fix the deploy script\nit fails on arm64");

    app.note_list.selected = vec!["infra/deploy.md".into()];
    let message = app.message();

    assert!(
        message.starts_with("Fix the deploy script\nit fails on arm64\n\nRead these notes first:\n- "),
        "{message}"
    );

    key(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);

    match &app.outcome {
        Some(Outcome::New { message, .. }) => assert_eq!(message.lines().next(), Some("Fix the deploy script")),
        _ => panic!("0 start"),
    }

    // a title: line 1, the prompt after it
    let mut app = test_app();

    app.compose.title.set_text("Deploy");
    app.compose.prompt.set_text("fix it");

    assert_eq!(app.message(), "Deploy\n\nfix it");

    // a long 1st line: the title (branch name) cut at 80 chars
    app.compose.title.set_text("");
    app.compose.prompt.set_text(&"x".repeat(120));

    assert_eq!(app.task_title().chars().count(), 80);
}

#[test]
fn esc_on_an_edited_agent_form_asks_and_no_keeps_the_form() {
    let mut app = test_app();

    app.run(Act::NewAgent);

    assert!(matches!(app.overlays.modal, Some(Modal::Agent(_))));

    // 0 edits: esc closes at once
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.overlays.modal.is_none());

    app.run(Act::NewAgent);
    type_text(&mut app, "nova");

    // esc: a confirm; n: the form back with its text; esc esc fast: still asked, 0 drop
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(matches!(app.overlays.modal, Some(Modal::Confirm { .. })));

    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);

    match &app.overlays.modal {
        Some(Modal::Agent(file)) => assert_eq!(file.name.text(), "nova"),
        _ => panic!("0 form"),
    }

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        matches!(app.overlays.modal, Some(Modal::Agent(_))),
        "esc on the confirm: no, the form stays"
    );

    // y: dropped
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    assert!(app.overlays.modal.is_none() && app.outcome.is_none());
}

#[test]
fn reset_asks_first() {
    let mut app = test_app();

    app.note_list.selected = vec!["ideas.md".into()];

    // ctrl+r: 0 reset (ctrl+r reads again on every screen); the menu command asks first
    key(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);

    assert!(app.overlays.modal.is_none());

    app.run(Act::Reset);

    assert!(matches!(app.overlays.modal, Some(Modal::Confirm { .. })));

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(app.note_list.selected, vec!["ideas.md".to_string()]);

    app.run(Act::Reset);
    key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    assert!(app.note_list.selected.is_empty() && app.overlays.modal.is_none());
}

#[test]
fn several_contexts_go_into_message() {
    let mut app = test_app();

    app.pick_session(Session {
        id: "s1".into(),
        cwd: "/tmp/x".into(),
        title: "Old work".into(),
        last: "DONE. 3 files".into(),
        ..Default::default()
    });
    app.compose.carry.push(Session {
        id: "s2".into(),
        thread: Some("abcdef0123456789".into()),
        title: "Thread work".into(),
        last: "OK. shipped".into(),
        ..Default::default()
    });

    // same session twice: 1 entry
    app.pick_session(Session {
        id: "s1".into(),
        cwd: "/tmp/x".into(),
        title: "Old work".into(),
        last: "DONE. 3 files".into(),
        ..Default::default()
    });

    assert_eq!(app.compose.carry.len(), 2);

    let message = app.message();

    assert!(message.contains("Claude Code session s1"));
    assert!(message.contains("codehead thread \"Thread work\" (abcdef012345"));
    assert!(message.ends_with("OK. shipped"));
}

#[test]
fn carry_goes_into_message() {
    let mut app = test_app();

    app.pick_session(Session {
        id: "s1".into(),
        cwd: "/tmp/x".into(),
        title: "Old work".into(),
        last: "DONE. 3 files".into(),
        ..Default::default()
    });

    assert!(matches!(app.target(), Ok(Target::Dir(directory)) if directory == "/tmp/x"));

    let message = app.message();

    assert!(message.starts_with("Old work\n\nContext: the last response of Claude Code session s1"));
    assert!(message.ends_with("DONE. 3 files"));
}

/// The start screen's threads tab: the menu's cards in sections, the view tabs; a asks
/// first, esc says no (0 quit, 0 tab change), y archives.
#[test]
fn start_threads_tab_shows_cards_in_sections_and_archives_after_a_yes() {
    let mut app = test_app();
    let thread = crate::engine::store::Thread::new("archive me", "task", &std::env::temp_dir(), vec!["ada".into()]);

    thread.save().unwrap();
    app.thread_list.worktrees = Some(Vec::new());
    app.thread_list.tlist.set_cards(vec![
        card("settled", |_| {}),
        card("asking", |card| card.agents.blocked = true),
        card("working", |card| card.agents.busy = true),
        card(&thread.id, |card| {
            card.land = Some(crate::domain::landed::Land::Landed);
            card.title = "archive me".into();
        }),
    ]);
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    let text = dump(&mut app, 130, 40);

    for line in [
        "status ",
        "open 3",
        "shipped 1",
        "all 4",
        "show",
        "active 4",
        "archived 0",
        "worktrees 0",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }

    assert!(!text.contains("archive me"), "status tab open: 0 shipped cards: {text}");

    // status tab all: the shipped card 4th (3 open, shipped)
    app.thread_list.tlist.stat_pick = Some(crate::features::thread_list::StatusTab::All);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(
        app.thread_list.tlist.current().map(|card| card.id.clone()),
        Some(thread.id.clone())
    );

    key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
    let asked = dump(&mut app, 120, 40);

    assert!(asked.contains("archive 1 thread(s)?"), "{asked}");

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && app.tab == Tab::Threads);
    assert_eq!(crate::engine::store::load(&thread.id).unwrap().state, "open");

    key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    assert_eq!(crate::engine::store::load(&thread.id).unwrap().state, "archived");

    let _ = crate::engine::store::delete(&thread.id);
}

/// e on a card: the edit form; ctrl+s saves the title; the palette's command runs it too.
#[test]
fn start_threads_tab_edits_a_thread() {
    let mut app = test_app();
    let thread = crate::engine::store::Thread::new("before", "task", &std::env::temp_dir(), vec!["ada".into()]);

    thread.save().unwrap();
    app.thread_list.worktrees = Some(Vec::new());
    app.thread_list
        .tlist
        .set_cards(vec![card(&thread.id, |card| card.title = "before".into())]);
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    app.run(Act::Thread(crate::features::thread_list::Act::Edit));
    let text = dump(&mut app, 120, 40);

    assert!(
        text.contains("edit thread") && text.contains("linked repos") && text.contains("agents"),
        "{text}"
    );

    // esc in the form: the form closes, 0 quit
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && !app.thread_list.tlist.modal());

    key(&mut app, KeyCode::Char('e'), KeyModifiers::NONE);

    for _ in 0.."before".len() {
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }

    for letter in "after".chars() {
        key(&mut app, KeyCode::Char(letter), KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert_eq!(crate::engine::store::load(&thread.id).unwrap().title, "after");

    let _ = crate::engine::store::delete(&thread.id);
}
