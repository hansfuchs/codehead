use super::*;

#[test]
fn agent_form_harness_and_model_dropdowns() {
    use crate::infra::models::{Entry, Model};
    let mut app = test_app();

    app.run(Act::NewAgent);

    let model = |id: &str, name: &str| Model {
        id: id.into(),
        name: name.into(),
    };

    // fixed installed harnesses; codex: a fresh cached list; reads: a fake, 0 harness spawned
    form(&mut app).harnesses = [
        ("claude", "Claude Code", "claude"),
        ("codex", "Codex", "codex"),
        ("opencode", "OpenCode", "opencode"),
    ]
    .map(|(name, label, bin)| (name.to_string(), label.to_string(), bin.to_string()))
    .to_vec();
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    app.model_catalog.models = Some(HashMap::from([(
        "codex".to_string(),
        Entry {
            at,
            models: vec![
                model("gpt-6.1-sol", "GPT-6.1-Sol"),
                model("gpt-6.1-terra", "GPT-6.1 Terra"),
            ],
        },
    )]));

    app.model_catalog.probe = |harness| {
        if harness == "opencode" {
            Ok(vec![Model {
                id: "opencode/big-pickle".into(),
                name: String::new(),
            }])
        } else {
            Err("probe failed".into())
        }
    };

    agent_form_harness_and_model_dropdowns_verify_out(app);
}

fn agent_form_harness_and_model_dropdowns_verify_out(mut app: App) {
    type_text(&mut app, "nova");
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(form(&mut app).field, F_HARNESS);

    // enter: the installed harnesses, the current one marked
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let out = dump(&mut app, 120, 44);

    assert!(
        out.contains("harnesses · installed")
            && out.contains("● Claude Code")
            && out.contains("Codex")
            && out.contains("OpenCode"),
        "{out}"
    );

    // type: filter; enter picks → the model list of codex (from the cache: 0 read)
    type_text(&mut app, "codex");
    let out = dump(&mut app, 120, 44);

    assert!(
        out.contains("harnesses › codex") && out.contains("1 of 3") && !out.contains("OpenCode"),
        "{out}"
    );

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let agent_form = form(&mut app);

    assert_eq!(
        (agent_form.harness.text(), agent_form.field),
        ("codex".to_string(), F_MODEL)
    );
    assert!(
        agent_form
            .pick
            .as_ref()
            .is_some_and(|pick| pick.model && pick.harness == "codex")
    );
    assert_eq!(app.model_catalog.model_reads.pending(), 0, "fresh cache: 0 read");

    let out = dump(&mut app, 120, 44);

    assert!(
        out.contains("models · codex")
            && out.contains("● default")
            && out.contains("gpt-6.1-sol")
            && out.contains("GPT-6.1 Terra"),
        "{out}"
    );

    println!("--- model dropdown\n{out}");

    // ↓↓ + enter: gpt-6.1-terra → harness/model
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    agent_form_harness_and_model_dropdowns_verify_saved(app);
}

fn agent_form_harness_and_model_dropdowns_verify_saved(mut app: App) {
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let agent_form = form(&mut app);

    assert!(agent_form.pick.is_none());
    assert_eq!(agent_form.harness.text(), "codex/gpt-6.1-terra");
    assert_eq!(
        agent_form.spec(&[], None).harness,
        "codex/gpt-6.1-terra",
        "the saved value"
    );

    let out = dump(&mut app, 120, 44);

    assert!(out.contains(" gpt-6.1-terra   codex models: enter"), "{out}");

    // enter on the model field: the list again, the cursor on the current model; esc closes
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let out = dump(&mut app, 120, 44);

    assert!(out.contains("● gpt-6.1-terra"), "{out}");
    assert_eq!(form(&mut app).pick.as_ref().map(|pick| pick.cursor), Some(2));

    // r: read again (the fake fails: the note says so, the cached list stays)
    key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);

    for _ in 0..100 {
        if app.model_catalog.model_reads.pending() == 0 {
            break;
        }

        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }

    let out = dump(&mut app, 120, 44);

    assert!(
        out.contains("models: probe failed") && out.contains("gpt-6.1-sol"),
        "{out}"
    );

    // 0 match: enter takes the typed id
    type_text(&mut app, "gpt-7");
    let out = dump(&mut app, 120, 44);

    assert!(out.contains("0 matches · enter: use as typed"), "{out}");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(form(&mut app).harness.text(), "codex/gpt-7");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        form(&mut app).pick.is_none() && app.overlays.modal.is_some(),
        "esc: only the dropdown"
    );
    assert_eq!(form(&mut app).harness.text(), "codex/gpt-7");

    // opencode: 0 cache → reading models…, then the read list
    form(&mut app).field = F_HARNESS;
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "open");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(form(&mut app).harness.text(), "opencode");

    agent_form_harness_and_model_dropdowns_verify_buffer(app);
}

fn agent_form_harness_and_model_dropdowns_verify_buffer(mut app: App) {
    let mut terminal = Terminal::new(TestBackend::new(120, 44)).unwrap();

    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    let first = (0..44)
        .map(|y| {
            (0..120)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(first.contains("reading models…"), "{first}");

    let out = dump(&mut app, 120, 44);

    assert!(out.contains("opencode/big-pickle"), "{out}");

    // a click on a row picks it
    let (row, _) = app
        .preview
        .regions
        .form_rows
        .iter()
        .copied()
        .find(|(_, j)| *j == 1)
        .unwrap();

    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(form(&mut app).harness.text(), "opencode/opencode/big-pickle");

    // claude: aliases with their exact ids; a pick → claude/<alias>
    form(&mut app).field = F_HARNESS;
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "claude");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let out = dump(&mut app, 120, 44);

    assert!(
        out.contains("models · claude") && out.contains("claude-opus-5-5") && out.contains("● default"),
        "{out}"
    );

    type_text(&mut app, "sonnet");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let agent_form = form(&mut app);

    assert_eq!(
        (agent_form.harness.text(), agent_form.harness_model()),
        ("claude".to_string(), "claude/sonnet".to_string())
    );
}

#[test]
fn fuzzy_ranks() {
    assert!(fuzzy("lci", "local ci v1").is_some());
    assert!(fuzzy("xyz", "local ci v1").is_none());
    assert!(fuzzy("local", "local ci v1") > fuzzy("local", "l o c a l"));
}

#[test]
fn thread_filter_keys() {
    let mut app = test_app();

    app.thread_list.worktrees = Some(Vec::new());
    app.thread_list.sessions = vec![
        Session {
            title: "local ci v1".into(),
            cwd: "/p".into(),
            last: "x".into(),
            ..Default::default()
        },
        Session {
            title: "Ampcode ideas".into(),
            cwd: "/p".into(),
            last: "x".into(),
            ..Default::default()
        },
    ];
    app.switch_tab(Tab::Threads);
    app.thread_list
        .tlist
        .set_view(crate::features::thread_list::View::Claude);

    // f and /: 0 filter; ctrl+f: the only key to it
    key(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);

    assert!(!app.thread_list.tlist.filtering);

    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(app.thread_list.tlist.filtering);

    for character in "amp".chars() {
        key(&mut app, KeyCode::Char(character), KeyModifiers::NONE);
    }

    assert_eq!(app.n_threads(), 1);
    assert_eq!(app.current_session().unwrap().title, "Ampcode ideas");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(!app.thread_list.tlist.filtering && app.thread_list.tlist.query() == "amp");

    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!app.thread_list.tlist.filtering);

    println!("--- thread filter\n{}", dump(&mut app, 110, 10));
}

#[test]
fn settings_tab_entry_keeps_paths_unfocused() {
    let mut app = test_app();
    let config = format!("notes = {:?}\n", format!("{}/infra/", app.cfg.cwd.display()));

    std::fs::write(&app.settings_path, &config).unwrap();

    app.switch_tab(Tab::Agents);
    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);

    assert!(app.tab == Tab::Settings && app.tabs_focus);

    let screen = dump(&mut app, 100, 36);

    assert!(!screen.contains("deploy.md"), "{screen}");

    key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);

    assert!(app.compose.screen == Screen::Compose);
    assert_eq!(std::fs::read_to_string(&app.settings_path).unwrap(), config);

    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);

    assert!(app.tab == Tab::Settings && app.tabs_focus);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(!app.tabs_focus);

    let screen = dump(&mut app, 100, 36);

    assert!(!screen.contains("deploy.md"), "{screen}");

    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    let screen = dump(&mut app, 100, 36);

    assert!(screen.contains("deploy.md"), "{screen}");

    // Re-entry must remove focus even after the path field was active.
    app.switch_tab(Tab::Agents);
    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);

    assert!(app.tab == Tab::Settings && app.tabs_focus);

    let screen = dump(&mut app, 100, 36);

    assert!(!screen.contains("deploy.md"), "{screen}");

    key(&mut app, KeyCode::Char('5'), KeyModifiers::ALT);

    assert_eq!(app.tab, Tab::Agents);
    assert_eq!(std::fs::read_to_string(&app.settings_path).unwrap(), config);
}

#[test]
fn settings_path_escape_closes_dropdown_before_tab_navigation() {
    let mut app = test_app();

    app.switch_tab(Tab::Settings);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    app.paste("local");
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    app.paste(&format!("{}/infra/", app.cfg.cwd.display()));
    let screen = dump(&mut app, 100, 100);

    assert!(screen.contains("deploy.md"), "{screen}");

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.tab == Tab::Settings && app.outcome.is_none());
    assert!(
        app.overlays.last_esc.is_none(),
        "dropdown Escape must not arm double Escape"
    );

    let screen = dump(&mut app, 100, 100);

    assert!(!screen.contains("deploy.md"), "{screen}");

    // tab: the next field (the alias box locked), still settings; alt+1: the task tab
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);

    assert!(app.compose.screen == Screen::Pick && app.tab == Tab::Settings);

    key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);

    assert!(app.compose.screen == Screen::Compose);
}

#[test]
fn settings_sub_tabs_reach_the_mcp_servers_and_its_add_form() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let server = crate::engine::servers::Server {
        name: "docs-x".into(),
        kind: "stdio".into(),
        command: Some("npx".into()),
        args: vec!["-y".into(), "docs".into()],
        auto_allow: true,
        ..Default::default()
    };

    crate::engine::servers::save(&[server]).unwrap();
    let mut app = test_app();

    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    // ↑ from the 1st field: the sub-tab bar; → →: mcp servers
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.settings.as_ref().unwrap().bar && !app.tabs_focus);

    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    let screen = dump(&mut app, 100, 30);

    assert!(screen.contains("harnesses") && screen.contains("model"), "{screen}");

    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    let screen = dump(&mut app, 100, 30);

    assert!(
        screen.contains("codehead · every harness")
            && screen.contains("docs-x")
            && screen.contains("0 agents · auto-allow"),
        "{screen}"
    );
    assert!(screen.contains("a add"), "help bar: {screen}");

    // a: the add form; a typed name; esc: asks, esc again: gone, 0 write
    key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
    app.paste("new-one");
    let screen = dump(&mut app, 100, 30);

    assert!(
        screen.contains("add MCP server") && screen.contains("new-one"),
        "{screen}"
    );

    // 1 read time for both escs: a slow CI runner never puts 2 s between them
    crate::ui::textarea::stamp(Some(std::time::Instant::now()));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        dump(&mut app, 100, 30).contains("add MCP server"),
        "1 esc: the edits stay"
    );

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    crate::ui::textarea::stamp(None);
    let screen = dump(&mut app, 100, 30);

    assert!(!screen.contains("add MCP server"), "{screen}");
    assert_eq!(crate::engine::servers::load().unwrap().len(), 1);

    // the filter: ctrl+f then text, a miss shows 0 rows
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    app.paste("zzqq");
    let screen = dump(&mut app, 100, 30);

    assert!(screen.contains("0 servers match the filter"), "{screen}");

    crate::engine::servers::save(&[]).unwrap();
}

#[test]
fn settings_tab_keeps_form_edits_and_task_text() {
    let mut app = test_app();

    app.compose.title.set_text("task title");
    app.compose.prompt.set_text("task prompt");
    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);

    assert_eq!(app.tab, Tab::Settings);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    app.paste("project:platform");
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    app.paste("~/notes/platform/");
    let screen = dump(&mut app, 80, 18);

    assert!(
        screen.lines().next().unwrap().contains("settings") && screen.contains("mcp servers"),
        "{screen}"
    );
    assert!(screen.contains("not applied"), "{screen}");
    assert!(screen.contains("~/notes/platform/"), "{screen}");

    // alt+1 / alt+4: away and back, the form edits and the task text stay
    key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);

    assert!(app.compose.screen == Screen::Compose);
    assert_eq!(app.compose.title.text(), "task title");
    assert_eq!(app.compose.prompt.text(), "task prompt");

    key(&mut app, KeyCode::Char('7'), KeyModifiers::ALT);
    let screen = dump(&mut app, 100, 24);

    assert!(screen.contains("~/notes/platform/"), "{screen}");

    key(&mut app, KeyCode::Char('5'), KeyModifiers::ALT);

    assert_eq!(app.tab, Tab::Agents);
}

fn form(app: &mut App) -> &mut AgentForm {
    match &mut app.overlays.modal {
        Some(Modal::Agent(file)) => file,
        _ => panic!("0 agent form"),
    }
}
