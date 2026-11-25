use super::*;

#[test]
fn cmd_or_ctrl_enter_starts() {
    for message in [KeyModifiers::CONTROL, KeyModifiers::SUPER] {
        let mut app = test_app();

        app.compose.title.set_text("Fix deploy");

        // ctrl+s saves a draft (the form starts over), never starts
        key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

        assert!(app.outcome.is_none(), "ctrl+s must not start");

        app.compose.title.set_text("Fix deploy");
        key(&mut app, KeyCode::Enter, message);

        assert!(matches!(app.outcome, Some(Outcome::New { .. })));
    }
}

#[test]
fn double_esc_quits() {
    let mut app = test_app();

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none());

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(matches!(app.outcome, Some(Outcome::Quit)));

    // another key between the 2 esc: no quit
    let mut app_value = test_app();

    key(&mut app_value, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app_value, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app_value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app_value.outcome.is_none());
}

#[test]
fn escape_never_switches_main_tabs() {
    for tab in [Tab::New, Tab::Threads, Tab::Agents, Tab::Settings] {
        let mut app = test_app();

        app.switch_tab(tab);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        assert!(app.cur_tab() == tab && app.outcome.is_none());

        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.cur_tab(), tab);
        assert!(
            matches!(app.outcome, Some(Outcome::Quit)),
            "esc esc in {tab:?}: codehead closes"
        );
    }
}

#[test]
fn escape_closes_local_controls_without_switching_tabs() {
    let mut app = test_app();

    app.switch_tab(Tab::Agents);
    app.run(Act::NewAgent);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.overlays.modal.is_none() && app.tab == Tab::Agents);

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && app.tab == Tab::Agents);

    app.switch_tab(Tab::Threads);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    app.paste("thread filter");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!app.thread_list.tlist.filtering && app.tab == Tab::Threads);

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && app.tab == Tab::Threads);

    app.open_notes();
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && app.compose.screen == Screen::Compose);

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(matches!(app.outcome, Some(Outcome::Quit)));
}

#[test]
fn prompt_at_agents_and_attachments() {
    let mut app = test_app();

    app.agent_list.agents = vec![
        Agent {
            name: "amon".into(),
            ..Default::default()
        },
        Agent {
            name: "nova".into(),
            ..Default::default()
        },
    ];
    app.compose.title.set_text("t");
    app.compose.field = Field::Prompt;

    for character in "ask @n".chars() {
        key(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }

    println!("--- @ dropdown\n{}", dump(&mut app, 80, 24));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "ask @nova ");

    // esc closes the dropdown, 0 tab switch
    key(&mut app, KeyCode::Char('@'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.compose.screen == Screen::Compose && app.compose.prompt.text() == "ask @nova @");

    // a dropped image path → marker + attachment list
    let img = std::env::temp_dir().join(format!("codehead-att-{}.png", std::process::id()));

    std::fs::write(&img, b"\x89PNG").unwrap();
    app.paste(&img.display().to_string());

    assert!(app.compose.prompt.text().ends_with("[Image #1]"));

    let message = app.message();

    assert!(
        message.ends_with(&format!(
            "Attachments (read these files):\n- [Image #1]: {}",
            img.display()
        )),
        "{message}"
    );

    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);

    assert!(!app.compose.prompt.text().contains("[Image #1]"));
    assert!(!app.message().contains(&img.display().to_string()));
    assert_eq!(app.draft().files, [] as [(std::string::String, std::string::String); 0]);

    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert!(app.message().contains(&img.display().to_string()));

    let _ = std::fs::remove_file(&img);
}

#[test]
fn ctrl_s_keeps_the_task_as_a_draft_and_starts_over() {
    let mut app = test_app();
    let dir = app.cfg.drafts.clone();

    // 0 text: 0 draft
    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert_eq!(
        drafts::load(&dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    app.compose.field = Field::Title;
    type_text(&mut app, "idea for later");
    key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);

    assert!(app.outcome.is_none());

    // the form: empty; the draft: kept, listed, marked
    assert!(app.compose.title.text().is_empty() && app.compose.field == Field::Prompt);

    let all = drafts::load(&dir, &app.draft_selection.draft_id);

    assert_eq!(
        all.iter()
            .map(|directory| (directory.title.as_str(), directory.kept))
            .collect::<Vec<_>>(),
        [("idea for later", true)]
    );

    app.compose.field = Field::Drafts;
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(dump(&mut app, 120, 40).contains("★ saved"));

    // loaded and edited: it stays kept
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "!");
    let directory_2 = drafts::load(&dir, "")
        .into_iter()
        .find(|directory_value| directory_value.title.starts_with("idea"))
        .unwrap();

    assert!(directory_2.kept, "{directory_2:?}");
}

#[test]
fn save_as_draft_button_and_menu_new_starts_empty() {
    let mut app = test_app();
    let dir = app.cfg.drafts.clone();

    // 0 drafts: the save button only
    let text = dump(&mut app, 120, 40);

    assert!(
        text.contains("save as draft") && !text.contains("view drafts"),
        "{text}"
    );

    type_text(&mut app, "first idea");
    app.compose.field = Field::Drafts;

    // ← with 0 drafts: stays on save; enter: kept, the form empty
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "");
    assert_eq!(
        drafts::load(&dir, &app.draft_selection.draft_id)
            .iter()
            .map(|directory| (directory.prompt.as_str(), directory.kept))
            .collect::<Vec<_>>(),
        [("first idea", true)]
    );
    assert!(dump(&mut app, 120, 40).contains("view drafts (1)"));

    // a loaded draft, then menu "n": the form empty, the draft kept in the list
    app.compose.field = Field::Drafts;
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "first idea");

    type_text(&mut app, " more");
    app.go_home(&(crate::screens::quick_menu::Home::New));

    assert!(app.compose.prompt.text().is_empty() && app.compose.title.text().is_empty());
    assert_eq!(
        app.draft_selection
            .drafts
            .iter()
            .map(|directory_value| directory_value.prompt.as_str())
            .collect::<Vec<_>>(),
        ["first idea more"]
    );

    // an empty form: "n" again, 0 new draft
    app.go_home(&(crate::screens::quick_menu::Home::New));

    assert_eq!(drafts::load(&dir, "").len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drafts_backup_list_load_delete() {
    let mut app = test_app();
    let dir = app.cfg.drafts.clone();

    // 0 text: 0 draft file, 0 drafts field
    assert_eq!(
        drafts::load(&dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );
    assert!(!dump(&mut app, 120, 40).contains("view drafts"));

    // every key: the draft file has the text
    app.compose.field = Field::Title;
    type_text(&mut app, "fix the build");

    assert_eq!(
        drafts::load(&dir, "")
            .iter()
            .map(|directory| directory.title.clone())
            .collect::<Vec<_>>(),
        ["fix the build"]
    );

    app.compose.field = Field::Prompt;
    type_text(&mut app, "- a");

    // enter continues the list: "- " comes by itself
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "b");

    assert_eq!(drafts::load(&dir, "")[0].prompt, "- a\n- b");

    // its own draft: not in its list
    assert_eq!(
        app.draft_selection.drafts,
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    // another codehead (the 1st one died): the draft shows, ↑ from the prompt reaches the field
    let mut app_value = app_with(Some(&dir));

    assert!(dump(&mut app_value, 120, 40).contains("view drafts (1)"));
    assert!(app_value.compose.field == Field::Prompt);

    key(&mut app_value, KeyCode::Up, KeyModifiers::NONE);

    assert!(app_value.compose.field == Field::Drafts);

    // enter: the dropdown; esc: back to the task (0 quit)
    key(&mut app_value, KeyCode::Enter, KeyModifiers::NONE);
    let text = dump(&mut app_value, 120, 40);

    assert!(
        text.contains(" drafts ") && text.contains("fix the build") && text.contains("d d: delete"),
        "{text}"
    );

    key(&mut app_value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app_value.draft_selection.drafts_menu.is_none() && app_value.outcome.is_none());

    // enter enter: loaded, the draft is this task's own now
    key(&mut app_value, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app_value, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        (
            app_value.compose.title.text().as_str(),
            app_value.compose.prompt.text().as_str()
        ),
        ("fix the build", "- a\n- b")
    );
    assert!(app_value.draft_selection.drafts.is_empty() && app_value.compose.field == Field::Prompt);
    assert_eq!(drafts::load(&dir, "").len(), 1);

    // a 3rd codehead: d d deletes the draft, the field goes
    let mut app_2 = app_with(Some(&dir));

    key(&mut app_2, KeyCode::Up, KeyModifiers::NONE);
    key(&mut app_2, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app_2, KeyCode::Char('d'), KeyModifiers::NONE);

    assert_eq!(app_2.draft_selection.drafts.len(), 1, "a 1st d only asks");

    key(&mut app_2, KeyCode::Char('d'), KeyModifiers::NONE);

    drafts_backup_list_load_delete_verify_old(&dir, app_2);
}

fn drafts_backup_list_load_delete_verify_old(dir: &std::path::Path, mut app_2: App) {
    assert!(
        app_2.draft_selection.drafts.is_empty()
            && app_2.draft_selection.drafts_menu.is_none()
            && app_2.compose.field == Field::Prompt
    );
    assert_eq!(
        drafts::load(dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    // a started thread drops its draft; an emptied task too
    type_text(&mut app_2, "x");

    assert_eq!(drafts::load(dir, "").len(), 1);

    key(&mut app_2, KeyCode::Backspace, KeyModifiers::NONE);

    assert_eq!(
        drafts::load(dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    type_text(&mut app_2, "y");
    app_2.drop_draft();

    assert_eq!(
        drafts::load(dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    // a started thread clears the form: the next new task starts empty, 0 old draft
    app_2.compose.field = Field::Prompt;
    type_text(&mut app_2, "the prompt");
    let old = app_2.draft_selection.draft_id.clone();

    app_2.clear_task();

    assert_eq!(
        (
            app_2.compose.title.text().as_str(),
            app_2.compose.prompt.text().as_str()
        ),
        ("", "")
    );
    assert!(
        app_2.note_list.selected.is_empty()
            && app_2.compose.files.is_empty()
            && app_2.compose.carry.is_empty()
            && app_2.compose.field == Field::Prompt
            && app_2.draft_selection.draft_id != old
    );
    assert_eq!(
        drafts::load(dir, ""),
        [] as [crate::screens::start_screen::drafts::Draft; 0]
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn prompt_command_dropdown_inserts_a_file() {
    let mut app = test_app();

    app.compose.field = Field::Prompt;
    type_text(&mut app, "read >");
    let text = dump(&mut app, 120, 40);

    assert!(text.contains("┌ commands ") && text.contains("mention"), "{text}");

    // mention → files (of the test dir, 0 git) → the note file, by fuzzy text
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let text_value = dump(&mut app, 120, 40);

    assert!(
        text_value.contains(" mention ") && text_value.contains("artifacts"),
        "{text_value}"
    );

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let text_2 = dump(&mut app, 120, 40);

    assert!(
        text_2.contains(" mention › files ") && text_2.contains("ideas.md"),
        "{text_2}"
    );

    type_text(&mut app, "dply");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "read infra/deploy.md ");

    // esc on the dropdown: closed, 0 quit, the text stays
    type_text(&mut app, ">");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.outcome.is_none() && app.compose.screen == Screen::Compose);

    let text_3 = dump(&mut app, 120, 40);

    assert!(!text_3.contains("┌ commands "), "{text_3}");
}

#[test]
fn up_from_the_task_top_reaches_the_tabs() {
    let mut app = test_app();

    assert!(
        app.compose.screen == Screen::Compose
            && app.compose.field == Field::Prompt
            && app.draft_selection.drafts.is_empty()
    );

    // prompt → drafts row ("save as draft") → tabs
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Drafts && !app.tabs_focus);

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.tabs_focus);
}
